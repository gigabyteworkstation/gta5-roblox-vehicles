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

-- Returns the built mesh and the bbox centre of its vertices (Roblox space).
local function buildGeometry(buf: buffer, g: VehicleWire.Geometry): (EditableMesh, Vector3)
	local mesh = AssetService:CreateEditableMesh()

	local vids = table.create(g.vertexCount) :: { number }
	local lo = Vector3.new(math.huge, math.huge, math.huge)
	local hi = Vector3.new(-math.huge, -math.huge, -math.huge)
	for v = 0, g.vertexCount - 1 do
		local p = ragePos(buf, g.posOffset + v * 12)
		vids[v] = mesh:AddVertex(p)
		lo = Vector3.new(math.min(lo.X, p.X), math.min(lo.Y, p.Y), math.min(lo.Z, p.Z))
		hi = Vector3.new(math.max(hi.X, p.X), math.max(hi.Y, p.Y), math.max(hi.Z, p.Z))
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

	return mesh, (lo + hi) * 0.5
end

-- Load a vehicle and parent a Model to `parent` (default workspace).
-- baseUrl example: "http://192.168.1.50:5000"
function VehicleLoader.load(name: string, baseUrl: string, parent: Instance?): Model
	local url = `{baseUrl}/rpf/readVehicle?v={name}`
	local data = HttpService:GetAsync(url)
	local vehicle = VehicleWire.parse(data)

	local model = Instance.new("Model")
	model.Name = name

	local options = { CollisionFidelity = Enum.CollisionFidelity.Box }
	for i, g in vehicle.geometries do
		local mesh, center = buildGeometry(vehicle.buffer, g)
		local part = AssetService:CreateMeshPartAsync(Content.fromObject(mesh), options)
		part.Anchored = true
		part.Name = `geom{i}_shader{g.shaderIndex}`
		-- Undo the centering that CreateMeshPartAsync applies.
		part.CFrame = CFrame.new(center)
		part.Parent = model
	end

	model.Parent = parent or workspace
	return model
end

return VehicleLoader
