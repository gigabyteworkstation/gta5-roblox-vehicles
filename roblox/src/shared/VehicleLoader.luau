--!strict
-- Fetches a vehicle and builds it as skinned MeshParts.
--
-- Each geometry becomes one EditableMesh (kept well under limits). Per-vertex
-- GTA bone weights/indices are applied via AddBone + SetVertexBones/Weights,
-- then the mesh is baked into STATIC content (so the EditableMesh can be freed)
-- and matching Bone instances are created under the MeshPart so it can be posed.
--
-- Coordinates convert RAGE (X right, Y forward, Z up) → Roblox (X right, Y up,
-- Z back): (x, z, -y), scaled to studs. The SAME transform is applied to bone
-- rest positions so vertices and bones stay in one space.

local AssetService = game:GetService("AssetService")
local HttpService = game:GetService("HttpService")

local VehicleWire = require(script.Parent.VehicleWire)

local VehicleLoader = {}

local STUDS_PER_METER = 4

local function ragePos(buf: buffer, off: number): Vector3
	local x = buffer.readf32(buf, off)
	local y = buffer.readf32(buf, off + 4)
	local z = buffer.readf32(buf, off + 8)
	return Vector3.new(x, z, -y) * STUDS_PER_METER
end

-- The RAGE→Roblox axis swap (x,y,z)→(x,z,-y) is a -90° rotation about X.
-- Applied to a full bone CFrame: orientation rotates by AXIS_SWAP, position
-- swaps + scales — consistent with ragePos() on the vertices.
local AXIS_SWAP = CFrame.Angles(-math.rad(90), 0, 0)

local function toRobloxCF(rageCF: CFrame): CFrame
	local c = AXIS_SWAP * rageCF
	return CFrame.new(c.Position * STUDS_PER_METER) * c.Rotation
end

type UsedBone = { name: string, cframe: CFrame }

local function buildGeometry(
	buf: buffer,
	g: VehicleWire.Geometry,
	bones: { VehicleWire.Bone }
): (EditableMesh?, { UsedBone })
	local used: { UsedBone } = {}
	local mesh: EditableMesh? = AssetService:CreateEditableMesh()
	if not mesh then
		return nil, used
	end

	local vids = table.create(g.vertexCount) :: { number }
	for v = 0, g.vertexCount - 1 do
		vids[v] = mesh:AddVertex(ragePos(buf, g.posOffset + v * 12))
	end

	local nids: { number }? = nil
	if g.hasNormals and g.normalOffset then
		local off = g.normalOffset
		local arr = table.create(g.vertexCount) :: { number }
		for v = 0, g.vertexCount - 1 do
			arr[v] = mesh:AddNormal((ragePos(buf, off + v * 12) / STUDS_PER_METER))
		end
		nids = arr
	end

	local uvids: { number }? = nil
	if g.hasUVs and g.uvOffset then
		local off = g.uvOffset
		local arr = table.create(g.vertexCount) :: { number }
		for v = 0, g.vertexCount - 1 do
			local u = buffer.readf32(buf, off + v * 8)
			local w = buffer.readf32(buf, off + v * 8 + 4)
			arr[v] = mesh:AddUV(Vector2.new(u, w))
		end
		uvids = arr
	end

	-- Skinning (best-effort: a failure still renders the geometry rigidly).
	if g.skinned and g.skinOffset then
		local so = g.skinOffset
		local ok, err = pcall(function()
			local boneMap: { [number]: number } = {} -- skeleton index (0-based) → editable bone id
			local function getBone(skelIdx: number): number
				local id = boneMap[skelIdx]
				if id then
					return id
				end
				local b = bones[skelIdx + 1]
				local cf = if b then toRobloxCF(b.cframe) else CFrame.identity
				local name = if b then b.name else `bone_{skelIdx}`
				-- Omit ParentId entirely: a root bone (flat rig, world-space rest
				-- CFrames). Passing -1 errors "Unknown id"; root = omitted/0.
				id = (mesh :: any):AddBone({ Name = name, CFrame = cf, Virtual = false })
				boneMap[skelIdx] = id
				table.insert(used, { name = name, cframe = cf })
				return id
			end
			for v = 0, g.vertexCount - 1 do
				local o = so + v * 12
				local idx = {
					buffer.readu16(buf, o),
					buffer.readu16(buf, o + 2),
					buffer.readu16(buf, o + 4),
					buffer.readu16(buf, o + 6),
				}
				local wt = { buffer.readu8(buf, o + 8), buffer.readu8(buf, o + 9), buffer.readu8(buf, o + 10), buffer.readu8(buf, o + 11) }
				local ids, ws = {}, {}
				for k = 1, 4 do
					if wt[k] > 0 then
						table.insert(ids, getBone(idx[k]))
						table.insert(ws, wt[k] / 255)
					end
				end
				if #ids == 0 then
					table.insert(ids, getBone(idx[1]))
					table.insert(ws, 1)
				end
				;(mesh :: any):SetVertexBones(vids[v], ids)
				;(mesh :: any):SetVertexBoneWeights(vids[v], ws)
			end
		end)
		if not ok then
			warn(`[GtaVehicle] skinning failed (rigid fallback): {err}`)
			used = {}
		end
	end

	local io = g.indexOffset
	local t = 0
	while t + 2 < g.indexCount do
		local a = buffer.readu16(buf, io + t * 2)
		local b = buffer.readu16(buf, io + (t + 1) * 2)
		local c = buffer.readu16(buf, io + (t + 2) * 2)
		local fid = mesh:AddTriangle(vids[a], vids[b], vids[c])
		if nids then
			mesh:SetFaceNormals(fid, { nids[a], nids[b], nids[c] })
		end
		if uvids then
			mesh:SetFaceUVs(fid, { uvids[a], uvids[b], uvids[c] })
		end
		t += 3
	end

	return mesh, used
end

function VehicleLoader.build(data: buffer | string, name: string, parent: Instance?): Model
	local vehicle = VehicleWire.parse(data)

	local model = Instance.new("Model")
	model.Name = name

	local options = {
		CollisionFidelity = Enum.CollisionFidelity.Box,
		RenderFidelity = Enum.RenderFidelity.Precise,
	}
	for i, g in vehicle.geometries do
		local mesh, used = buildGeometry(vehicle.buffer, g, vehicle.bones)
		if not mesh then
			warn(`[GtaVehicle] skipped geometry {i}: EditableMesh budget exhausted`)
			continue
		end
		local res, staticContent = AssetService:CreateDataModelContentAsync(Content.fromObject(mesh))
		mesh:Destroy()
		if res ~= Enum.CreateContentResult.Success then
			warn(`[GtaVehicle] geometry {i}: bake failed ({res})`)
			continue
		end

		local part = AssetService:CreateMeshPartAsync(staticContent, options)
		part.Anchored = true
		part.Name = `geom{i}_shader{g.shaderIndex}`
		part.Parent = model

		-- Bone instances so the skinned mesh can be posed at runtime.
		for _, ub in used do
			local bone = Instance.new("Bone")
			bone.Name = ub.name
			bone.Parent = part
			bone.CFrame = ub.cframe
		end
	end

	model.Parent = parent or workspace
	return model
end

-- Pose a bone across the whole vehicle: sets Transform on every Bone with this
-- name in any of the model's MeshParts. transform is the LOCAL offset from rest.
function VehicleLoader.poseBone(model: Model, boneName: string, transform: CFrame)
	for _, d in model:GetDescendants() do
		if d:IsA("Bone") and d.Name == boneName then
			d.Transform = transform
		end
	end
end

function VehicleLoader.load(name: string, baseUrl: string, parent: Instance?): Model
	local data = HttpService:GetAsync(`{baseUrl}/rpf/readVehicle?v={name}`)
	return VehicleLoader.build(data, name, parent)
end

return VehicleLoader
