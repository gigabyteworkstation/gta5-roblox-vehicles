--!strict
-- Builds a vehicle as rigid parts (GTA bone grouping) with textures.
--
-- Body components are anchored MeshParts, split by texture (one MeshPart per
-- texture so each can carry its own EditableImage). Articulated parts (doors/
-- hood/boot) stay as a single merged MeshPart with their dominant texture and a
-- Servo HingeConstraint (GTA axis + angle).

local AssetService = game:GetService("AssetService")
local HttpService = game:GetService("HttpService")

local VehicleWire = require(script.Parent.VehicleWire)

local VehicleLoader = {}

local STUDS_PER_METER = 4
local BAKE_OPTIONS = {
	CollisionFidelity = Enum.CollisionFidelity.Box,
	RenderFidelity = Enum.RenderFidelity.Precise,
}

local function ragePosWorld(buf: buffer, off: number): Vector3
	local x = buffer.readf32(buf, off)
	local y = buffer.readf32(buf, off + 4)
	local z = buffer.readf32(buf, off + 8)
	return Vector3.new(x, z, -y) * STUDS_PER_METER
end

local function rageVec(v: Vector3): Vector3
	return Vector3.new(v.X, v.Z, -v.Y) * STUDS_PER_METER
end

local function rageDir(v: Vector3): Vector3
	return Vector3.new(v.X, v.Z, -v.Y)
end

local function axisCFrame(pos: Vector3, axis: Vector3): CFrame
	axis = axis.Unit
	local ref = if math.abs(axis.Y) < 0.99 then Vector3.yAxis else Vector3.xAxis
	local y = ref:Cross(axis).Unit
	local z = axis:Cross(y).Unit
	return CFrame.fromMatrix(pos, axis, y, z)
end

-- Bake each texture into static content once (keyed by 0-based wire index).
local function buildTextureCache(vehicle: VehicleWire.Vehicle): { [number]: Content }
	local cache: { [number]: Content } = {}
	for i, t in vehicle.textures do
		local img = AssetService:CreateEditableImage({ Size = Vector2.new(t.width, t.height) })
		if img then
			local size = t.width * t.height * 4
			local buf = buffer.create(size)
			buffer.copy(buf, 0, vehicle.buffer, t.rgbaOffset, size)
			img:WritePixelsBuffer(Vector2.zero, Vector2.new(t.width, t.height), buf)
			local res, content = AssetService:CreateDataModelContentAsync(Content.fromObject(img))
			img:Destroy()
			if res == Enum.CreateContentResult.Success then
				cache[i - 1] = content -- wire textureIndex is 0-based
			end
		end
	end
	return cache
end

local function groupByTexture(geometries: { VehicleWire.Geometry }): { [number]: { VehicleWire.Geometry } }
	local groups: { [number]: { VehicleWire.Geometry } } = {}
	for _, g in geometries do
		local ti = g.textureIndex
		if not groups[ti] then
			groups[ti] = {}
		end
		table.insert(groups[ti], g)
	end
	return groups
end

local function dominantTexture(geometries: { VehicleWire.Geometry }): number
	local tris: { [number]: number } = {}
	for _, g in geometries do
		tris[g.textureIndex] = (tris[g.textureIndex] or 0) + g.indexCount
	end
	local best, bestN = -1, -1
	for ti, n in tris do
		if n > bestN then
			best, bestN = ti, n
		end
	end
	return best
end

-- Build ONE MeshPart from a list of geometries merged together.
local function buildPartMesh(buf: buffer, geometries: { VehicleWire.Geometry }): MeshPart?
	local mesh: EditableMesh? = AssetService:CreateEditableMesh()
	if not mesh then
		return nil
	end
	for _, geom in geometries do
		local vids = table.create(geom.vertexCount) :: { number }
		for v = 0, geom.vertexCount - 1 do
			vids[v] = mesh:AddVertex(ragePosWorld(buf, geom.posOffset + v * 12))
		end
		local nids: { number }? = nil
		if geom.hasNormals and geom.normalOffset then
			local off = geom.normalOffset
			local arr = table.create(geom.vertexCount) :: { number }
			for v = 0, geom.vertexCount - 1 do
				local x = buffer.readf32(buf, off + v * 12)
				local y = buffer.readf32(buf, off + v * 12 + 4)
				local z = buffer.readf32(buf, off + v * 12 + 8)
				arr[v] = mesh:AddNormal(Vector3.new(x, z, -y))
			end
			nids = arr
		end
		local uvids: { number }? = nil
		if geom.hasUVs and geom.uvOffset then
			local off = geom.uvOffset
			local arr = table.create(geom.vertexCount) :: { number }
			for v = 0, geom.vertexCount - 1 do
				arr[v] = mesh:AddUV(Vector2.new(buffer.readf32(buf, off + v * 8), buffer.readf32(buf, off + v * 8 + 4)))
			end
			uvids = arr
		end
		local io = geom.indexOffset
		local t = 0
		while t + 2 < geom.indexCount do
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
	end

	local res, content = AssetService:CreateDataModelContentAsync(Content.fromObject(mesh))
	mesh:Destroy()
	if res ~= Enum.CreateContentResult.Success then
		return nil
	end
	return AssetService:CreateMeshPartAsync(content, BAKE_OPTIONS)
end

local function applyTexture(part: MeshPart, cache: { [number]: Content }, ti: number)
	if cache[ti] then
		part.TextureContent = cache[ti]
	end
end

function VehicleLoader.build(data: buffer | string, name: string, parent: Instance?, pivot: CFrame?): Model
	local vehicle = VehicleWire.parse(data)
	local cache = buildTextureCache(vehicle)

	local model = Instance.new("Model")
	model.Name = name

	-- Body: anchored, split by texture.
	local bodyBase: BasePart? = nil
	for _, p in vehicle.parts do
		if not p.articulated then
			for ti, geoms in groupByTexture(p.geometries) do
				local part = buildPartMesh(vehicle.buffer, geoms)
				if part then
					part.Anchored = true
					applyTexture(part, cache, ti)
					part.Name = p.name
					part.Parent = model
					if not bodyBase then
						bodyBase = part
					end
				end
			end
		end
	end

	-- Articulated: split by texture (welded into one assembly), Servo hinge on
	-- the root sub-part.
	local articulated: { BasePart } = {}
	for _, p in vehicle.parts do
		if p.articulated and p.hinge and bodyBase then
			local h = p.hinge
			local root: BasePart? = nil
			for ti, geoms in groupByTexture(p.geometries) do
				local part = buildPartMesh(vehicle.buffer, geoms)
				if part then
					part.Anchored = true
					part.CanCollide = false
					applyTexture(part, cache, ti)
					part.Name = p.name
					part.Parent = model
					table.insert(articulated, part)
					if not root then
						root = part
					else
						local weld = Instance.new("WeldConstraint")
						weld.Part0 = root
						weld.Part1 = part
						weld.Parent = root
					end
				end
			end

			if root then
				local cf = axisCFrame(rageVec(h.pos), rageDir(h.axis))
				local a0 = Instance.new("Attachment")
				a0.WorldCFrame = cf
				a0.Parent = bodyBase
				local a1 = Instance.new("Attachment")
				a1.WorldCFrame = cf
				a1.Parent = root

				local hinge = Instance.new("HingeConstraint")
				hinge.Name = p.name
				hinge.Attachment0 = a0
				hinge.Attachment1 = a1
				hinge.LimitsEnabled = true
				hinge.LowerAngle = math.min(0, math.deg(h.minAngle))
				hinge.UpperAngle = math.max(5, math.deg(h.maxAngle))
				hinge.ActuatorType = Enum.ActuatorType.Servo
				hinge.ServoMaxTorque = 100000
				hinge.AngularSpeed = 2
				hinge.TargetAngle = 0
				hinge.Parent = root
			end
		end
	end

	model.Parent = parent or workspace
	if pivot then
		model:PivotTo(pivot)
	end
	for _, r in articulated do
		r.Anchored = false
	end

	return model
end

function VehicleLoader.setHinge(model: Model, partName: string, angleDeg: number)
	for _, d in model:GetDescendants() do
		if d:IsA("HingeConstraint") and d.Name == partName then
			d.TargetAngle = angleDeg
		end
	end
end

function VehicleLoader.openAll(model: Model)
	for _, d in model:GetDescendants() do
		if d:IsA("HingeConstraint") then
			d.TargetAngle = if math.abs(d.UpperAngle) >= math.abs(d.LowerAngle) then d.UpperAngle else d.LowerAngle
		end
	end
end

function VehicleLoader.closeAll(model: Model)
	for _, d in model:GetDescendants() do
		if d:IsA("HingeConstraint") then
			d.TargetAngle = 0
		end
	end
end

function VehicleLoader.load(name: string, baseUrl: string, parent: Instance?): Model
	local data = HttpService:GetAsync(`{baseUrl}/rpf/readVehicle?v={name}&tex=1`)
	return VehicleLoader.build(data, name, parent)
end

return VehicleLoader
