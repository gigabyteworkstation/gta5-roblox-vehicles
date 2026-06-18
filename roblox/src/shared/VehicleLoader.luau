--!strict
-- Builds a vehicle as rigid parts (matching GTA's bone grouping): the "body"
-- as anchored MeshParts, and each articulated part (doors/hood/boot) as ONE
-- merged MeshPart attached to the body with a HingeConstraint (GTA hinge).
--
-- Articulated parts: authored relative to their hinge (so physics rotates about
-- the hinge), built anchored, positioned with the car, then released so the
-- hinge takes over. One merged part per door avoids a pile of welded bodies.
--
-- RAGE (X right, Y fwd, Z up) → Roblox (X right, Y up, Z back): (x, z, -y) * scale.

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

-- Build ONE MeshPart from a list of geometries merged together, with vertices
-- offset by `origin`. Returns nil on budget/limit failure.
local function buildPartMesh(buf: buffer, geometries: { VehicleWire.Geometry }, origin: Vector3): MeshPart?
	local mesh: EditableMesh? = AssetService:CreateEditableMesh()
	if not mesh then
		return nil
	end

	for _, geom in geometries do
		local vids = table.create(geom.vertexCount) :: { number }
		for v = 0, geom.vertexCount - 1 do
			vids[v] = mesh:AddVertex(ragePosWorld(buf, geom.posOffset + v * 12) - origin)
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

function VehicleLoader.build(data: buffer | string, name: string, parent: Instance?, pivot: CFrame?): Model
	local vehicle = VehicleWire.parse(data)
	local model = Instance.new("Model")
	model.Name = name

	-- Body: anchored, one MeshPart per geometry (keeps each under mesh limits).
	local bodyBase: BasePart? = nil
	for _, p in vehicle.parts do
		if not p.articulated then
			for i, geom in p.geometries do
				local part = buildPartMesh(vehicle.buffer, { geom }, Vector3.zero)
				if part then
					part.Anchored = true
					part.Name = `body_{i}`
					part.Parent = model
					if not bodyBase then
						bodyBase = part
					end
				end
			end
		end
	end

	-- Articulated parts: ONE merged part each, built anchored for now.
	local articulatedRoots: { BasePart } = {}
	for _, p in vehicle.parts do
		if p.articulated and p.hinge and bodyBase then
			local h = p.hinge
			local hingePos = rageVec(h.pos)
			local hingeAxis = rageDir(h.axis)

			local part = buildPartMesh(vehicle.buffer, p.geometries, hingePos)
			if part then
				part.CFrame = CFrame.new(hingePos)
				part.Anchored = true -- released after positioning
				part.CanCollide = false
				part.Name = p.name
				part.Parent = model

				local cf = axisCFrame(hingePos, hingeAxis)
				local a0 = Instance.new("Attachment")
				a0.WorldCFrame = cf
				a0.Parent = bodyBase
				local a1 = Instance.new("Attachment")
				a1.WorldCFrame = cf
				a1.Parent = part

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
				hinge.Parent = part

				table.insert(articulatedRoots, part)
			end
		end
	end

	model.Parent = parent or workspace
	if pivot then
		model:PivotTo(pivot)
	end
	-- Release the hinged parts now that everything is positioned.
	for _, r in articulatedRoots do
		r.Anchored = false
	end

	return model
end

-- Open/close an articulated part by driving its hinge to an angle (degrees).
function VehicleLoader.setHinge(model: Model, partName: string, angleDeg: number)
	for _, d in model:GetDescendants() do
		if d:IsA("HingeConstraint") and d.Name == partName then
			d.TargetAngle = angleDeg
		end
	end
end

function VehicleLoader.load(name: string, baseUrl: string, parent: Instance?): Model
	local data = HttpService:GetAsync(`{baseUrl}/rpf/readVehicle?v={name}`)
	return VehicleLoader.build(data, name, parent)
end

return VehicleLoader
