--!strict
-- Fetches a vehicle from the backend and builds it as EditableMesh MeshParts.
--
-- One MeshPart per geometry (each well under EditableMesh limits; lets us bind
-- per-geometry textures later). Coordinates convert RAGE (X right, Y forward,
-- Z up) → Roblox (X right, Y up, Z back): (x, z, -y), scaled to studs.
--
-- IMPORTANT: AssetService:CreateMeshPartAsync centers the mesh on the part's
-- origin, so each part is repositioned to its geometry's bounding-box center —
-- otherwise all parts collapse onto one point and look like a blob of blocks.

local AssetService = game:GetService("AssetService")
local HttpService = game:GetService("HttpService")

local VehicleWire = require(script.Parent.VehicleWire)

local VehicleLoader = {}

-- RAGE units are ~metres. Roblox characters read ~3 studs/metre, so scale up
-- to make the car proportional and clearly visible.
local STUDS_PER_METER = 4

local function ragePos(buf: buffer, off: number): Vector3
	local x = buffer.readf32(buf, off)
	local y = buffer.readf32(buf, off + 4)
	local z = buffer.readf32(buf, off + 8)
	return Vector3.new(x, z, -y) * STUDS_PER_METER
end

local function rageDir(buf: buffer, off: number): Vector3
	local x = buffer.readf32(buf, off)
	local y = buffer.readf32(buf, off + 4)
	local z = buffer.readf32(buf, off + 8)
	return Vector3.new(x, z, -y) -- direction: no scale
end

local function buildGeometry(buf: buffer, g: VehicleWire.Geometry): EditableMesh?
	local mesh: EditableMesh? = AssetService:CreateEditableMesh()
	if not mesh then
		return nil -- client EditableMesh memory budget exhausted
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
			arr[v] = mesh:AddNormal(rageDir(buf, off + v * 12))
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

	return mesh
end

-- Build a vehicle Model from already-fetched wire bytes (buffer or string).
-- Pure local build — safe to run on the CLIENT (EditableMesh built on the
-- server does NOT replicate, so clients must build their own).
function VehicleLoader.build(data: buffer | string, name: string, parent: Instance?): Model
	local vehicle = VehicleWire.parse(data)

	local model = Instance.new("Model")
	model.Name = name

	-- RenderFidelity.Precise is required: a runtime EditableMesh has no
	-- precomputed LOD, so Automatic fidelity shows a placeholder box instead
	-- of the actual faces. CreateMeshPartAsync preserves authored vertex
	-- coordinates (it does NOT re-center), so parts stay at the origin and the
	-- mesh data places each panel correctly.
	local options = {
		CollisionFidelity = Enum.CollisionFidelity.Box,
		RenderFidelity = Enum.RenderFidelity.Precise,
	}
	for i, g in vehicle.geometries do
		local mesh = buildGeometry(vehicle.buffer, g)
		if not mesh then
			warn(`[GtaVehicle] skipped geometry {i}: EditableMesh budget exhausted`)
			continue
		end
		local part = AssetService:CreateMeshPartAsync(Content.fromObject(mesh), options)
		-- Free the EditableMesh immediately: the MeshPart is now independent,
		-- and EditableMeshes count against a tight client memory budget. Keeping
		-- all of them alive at once is what exhausted the budget.
		mesh:Destroy()
		part.Anchored = true
		part.Name = `geom{i}_shader{g.shaderIndex}`
		part.Parent = model
	end

	model.Parent = parent or workspace
	return model
end

-- Server-side convenience: fetch over HTTP and build directly (e.g. "Run" mode,
-- where there is no client). HttpService is server-only.
function VehicleLoader.load(name: string, baseUrl: string, parent: Instance?): Model
	local data = HttpService:GetAsync(`{baseUrl}/rpf/readVehicle?v={name}`)
	return VehicleLoader.build(data, name, parent)
end

return VehicleLoader
