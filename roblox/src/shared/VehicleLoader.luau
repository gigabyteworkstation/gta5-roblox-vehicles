--!strict
-- Fetches a vehicle from the backend and builds it as EditableMesh MeshParts.
--
-- One MeshPart per geometry (keeps each well under EditableMesh limits and lets
-- us bind per-geometry textures later). Coordinates are converted from RAGE
-- (X right, Y forward, Z up) to Roblox (X right, Y up, Z back): (x, z, -y).

local AssetService = game:GetService("AssetService")
local HttpService = game:GetService("HttpService")

local VehicleWire = require(script.Parent.VehicleWire)

local VehicleLoader = {}

-- RAGE Z-up → Roblox Y-up.
local function ragePos(buf: buffer, off: number): Vector3
	local x = buffer.readf32(buf, off)
	local y = buffer.readf32(buf, off + 4)
	local z = buffer.readf32(buf, off + 8)
	return Vector3.new(x, z, -y)
end

local function buildGeometry(buf: buffer, g: VehicleWire.Geometry): EditableMesh
	local mesh = AssetService:CreateEditableMesh()

	local vids = table.create(g.vertexCount)
	for v = 0, g.vertexCount - 1 do
		vids[v] = mesh:AddVertex(ragePos(buf, g.posOffset + v * 12))
	end

	local nids: { number }? = nil
	if g.hasNormals and g.normalOffset then
		local off = g.normalOffset
		nids = table.create(g.vertexCount)
		for v = 0, g.vertexCount - 1 do
			(nids :: any)[v] = mesh:AddNormal(ragePos(buf, off + v * 12))
		end
	end

	local uvids: { number }? = nil
	if g.hasUVs and g.uvOffset then
		local off = g.uvOffset
		uvids = table.create(g.vertexCount)
		for v = 0, g.vertexCount - 1 do
			local u = buffer.readf32(buf, off + v * 8)
			local w = buffer.readf32(buf, off + v * 8 + 4)
			(uvids :: any)[v] = mesh:AddUV(Vector2.new(u, w))
		end
	end

	local io = g.indexOffset
	for t = 0, g.indexCount - 3, 3 do
		local a = buffer.readu16(buf, io + t * 2)
		local b = buffer.readu16(buf, io + (t + 1) * 2)
		local c = buffer.readu16(buf, io + (t + 2) * 2)
		local fid = mesh:AddTriangle(vids[a], vids[b], vids[c])
		if nids then
			mesh:SetFaceNormals(fid, { (nids :: any)[a], (nids :: any)[b], (nids :: any)[c] })
		end
		if uvids then
			mesh:SetFaceUVs(fid, { (uvids :: any)[a], (uvids :: any)[b], (uvids :: any)[c] })
		end
	end

	return mesh
end

-- Load a vehicle and parent a Model to `parent` (default workspace).
-- baseUrl example: "http://192.168.1.50:5000"
function VehicleLoader.load(name: string, baseUrl: string, parent: Instance?): Model
	local url = `{baseUrl}/rpf/readVehicle?v={name}`
	local data = HttpService:GetAsync(url)
	local vehicle = VehicleWire.parse(data)

	local model = Instance.new("Model")
	model.Name = name

	for i, g in vehicle.geometries do
		local mesh = buildGeometry(vehicle.buffer, g)
		local part = AssetService:CreateMeshPartAsync(Content.fromObject(mesh))
		part.Anchored = true
		part.Name = `geom{i}_shader{g.shaderIndex}`
		part.Parent = model
	end

	model.Parent = parent or workspace
	return model
end

return VehicleLoader
