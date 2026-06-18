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

-- Make a Content for each texture (keyed by 0-based wire index). NOTE: static
-- DataModel content isn't supported for EditableImage yet, so we keep the live
-- EditableImage via Content.fromObject (switch to CreateDataModelContentAsync
-- once supported, to free the image budget).
local function buildTextureCache(vehicle: VehicleWire.Vehicle): { [number]: Content }
	local cache: { [number]: Content } = {}
	for i, t in vehicle.textures do
		local img = AssetService:CreateEditableImage({ Size = Vector2.new(t.width, t.height) })
		if img then
			local size = t.width * t.height * 4
			local buf = buffer.create(size)
			buffer.copy(buf, 0, vehicle.buffer, t.rgbaOffset, size)
			img:WritePixelsBuffer(Vector2.zero, Vector2.new(t.width, t.height), buf)
			cache[i - 1] = Content.fromObject(img) -- wire textureIndex is 0-based
		end
	end
	return cache
end

-- Group a part's geometries by diffuse texture (one MeshPart per visible
-- texture, each gets its own SurfaceAppearance).
local function groupByTexture(geometries: { VehicleWire.Geometry }): { [number]: { VehicleWire.Geometry } }
	local groups: { [number]: { VehicleWire.Geometry } } = {}
	for _, g in geometries do
		local ti = g.diffuseIndex
		if not groups[ti] then
			groups[ti] = {}
		end
		table.insert(groups[ti], g)
	end
	return groups
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

-- Apply a material via SurfaceAppearance: diffuse ColorMap, optional NormalMap,
-- and per-pixel transparency for glass (uses the diffuse alpha channel).
local function applyMaterial(part: MeshPart, cache: { [number]: Content }, geom: VehicleWire.Geometry)
	local diffuse = cache[geom.diffuseIndex]
	if diffuse then
		local saData: { [string]: any } = { ColorMap = diffuse }
		local normal = cache[geom.normalMapIndex]
		if normal then
			saData.NormalMap = normal
		end
		local sa = AssetService:CreateSurfaceAppearanceAsync(saData)
		sa.AlphaMode = if geom.glass then Enum.AlphaMode.Transparency else Enum.AlphaMode.Overlay
		sa.Parent = part
	else
		-- No diffuse = paint surface (runtime colour). Give it a glossy look.
		part.Material = Enum.Material.SmoothPlastic
		part.Reflectance = 0.08
	end
	if geom.glass then
		part.Transparency = 0.02
	end
end

-- Instance the centered "wheel" mesh at each wheel bone. Right-side wheels are
-- rotated 180° about up so the outer face points out (GTA mirrors the mesh).
-- Each wheel attaches to the body via a free HingeConstraint about the axle
-- (the car's lateral axis) so it can spin/roll.
local function buildWheels(
	vehicle: VehicleWire.Vehicle,
	container: Instance,
	bodyBase: BasePart?,
	cache: { [number]: Content }
): { BasePart }
	local out: { BasePart } = {}
	if not bodyBase then
		return out
	end
	local wheelGeoms: { VehicleWire.Geometry }? = nil
	for _, p in vehicle.parts do
		if p.name == "wheel" then
			wheelGeoms = p.geometries
			break
		end
	end
	if not wheelGeoms then
		return out
	end

	for _, wheel in vehicle.wheels do
		local part = buildPartMesh(vehicle.buffer, wheelGeoms)
		if part then
			applyMaterial(part, cache, wheelGeoms[1])
			part.Anchored = true
			part.CanCollide = false
			part.Name = "wheel"
			local pos = rageVec(wheel.pos)
			local rot = if wheel.mirror then CFrame.Angles(0, math.pi, 0) else CFrame.identity
			part.CFrame = CFrame.new(pos) * rot
			part.Parent = container

			-- Spin hinge about the axle (car lateral axis = world X).
			local cf = axisCFrame(pos, Vector3.xAxis)
			local a0 = Instance.new("Attachment")
			a0.WorldCFrame = cf
			a0.Parent = bodyBase
			local a1 = Instance.new("Attachment")
			a1.WorldCFrame = cf
			a1.Parent = part
			local hinge = Instance.new("HingeConstraint")
			hinge.Name = "wheel_spin"
			hinge.Attachment0 = a0
			hinge.Attachment1 = a1
			hinge.ActuatorType = Enum.ActuatorType.None -- free spin
			hinge.Parent = part

			table.insert(out, part)
		end
	end
	return out
end

function VehicleLoader.build(data: buffer | string, name: string, parent: Instance?, pivot: CFrame?): Model
	local vehicle = VehicleWire.parse(data)
	local cache = buildTextureCache(vehicle)

	local model = Instance.new("Model")
	model.Name = name

	-- One Model per bone-part (clean names + GTA hierarchy nesting). Texture
	-- sub-meshes live inside their part's Model.
	local partModels: { [string]: Model } = {}
	local function partModel(pname: string): Model
		local m = partModels[pname]
		if not m then
			m = Instance.new("Model")
			m.Name = pname
			m.Parent = model
			partModels[pname] = m
		end
		return m
	end

	-- Body: anchored, split by texture. The "wheel" part is instanced separately.
	local bodyBase: BasePart? = nil
	for _, p in vehicle.parts do
		if not p.articulated and p.name ~= "wheel" then
			local container = partModel(p.name)
			for _, geoms in groupByTexture(p.geometries) do
				local part = buildPartMesh(vehicle.buffer, geoms)
				if part then
					part.Anchored = true
					part.CanCollide = false
					applyMaterial(part, cache, geoms[1])
					part.Name = p.name
					part.Parent = container
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
			local container = partModel(p.name)
			local root: BasePart? = nil
			for _, geoms in groupByTexture(p.geometries) do
				local part = buildPartMesh(vehicle.buffer, geoms)
				if part then
					part.Anchored = true
					part.CanCollide = false
					applyMaterial(part, cache, geoms[1])
					part.Name = p.name
					part.Parent = container
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

	local wheels = buildWheels(vehicle, partModel("wheels"), bodyBase, cache)

	-- Nest part Models per the GTA bone hierarchy (chassis → doors, etc.).
	for _, p in vehicle.parts do
		local m = partModels[p.name]
		if m and p.parent ~= "" and partModels[p.parent] and p.parent ~= p.name then
			m.Parent = partModels[p.parent]
		end
	end

	model.Parent = parent or workspace
	if pivot then
		model:PivotTo(pivot)
	end
	-- Release dynamic parts now that everything is positioned.
	for _, r in articulated do
		r.Anchored = false
	end
	for _, w in wheels do
		w.Anchored = false
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
