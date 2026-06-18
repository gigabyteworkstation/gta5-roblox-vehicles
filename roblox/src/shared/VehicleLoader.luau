--!strict
-- Builds a vehicle as rigid parts (GTA bone grouping) with textures.
--
-- Body components are anchored MeshParts, split by texture (one MeshPart per
-- texture so each can carry its own EditableImage). Articulated parts (doors/
-- hood/boot) stay as a single merged MeshPart with their dominant texture and a
-- Servo HingeConstraint (GTA axis + angle).

local AssetService = game:GetService("AssetService")
local HttpService = game:GetService("HttpService")
local ContextActionService = game:GetService("ContextActionService")
local Players = game:GetService("Players")

local VehicleWire = require(script.Parent.VehicleWire)
local VehiclePhysics = require(script.Parent.VehiclePhysics)

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
local function buildPartMesh(buf: buffer, geometries: { VehicleWire.Geometry }, mirror: boolean?): MeshPart?
	local mesh: EditableMesh? = AssetService:CreateEditableMesh()
	if not mesh then
		return nil
	end
	-- GTA mirrors the right-side wheel by negating the lateral (X) scale. A
	-- reflection isn't a rigid CFrame, so we bake it into the mesh: negate X on
	-- positions + normals and reverse triangle winding (keeps faces outward).
	local sx = if mirror then -1 else 1
	for _, geom in geometries do
		local vids = table.create(geom.vertexCount) :: { number }
		for v = 0, geom.vertexCount - 1 do
			local p = ragePosWorld(buf, geom.posOffset + v * 12)
			vids[v] = mesh:AddVertex(if mirror then Vector3.new(-p.X, p.Y, p.Z) else p)
		end
		local nids: { number }? = nil
		if geom.hasNormals and geom.normalOffset then
			local off = geom.normalOffset
			local arr = table.create(geom.vertexCount) :: { number }
			for v = 0, geom.vertexCount - 1 do
				local x = buffer.readf32(buf, off + v * 12)
				local y = buffer.readf32(buf, off + v * 12 + 4)
				local z = buffer.readf32(buf, off + v * 12 + 8)
				arr[v] = mesh:AddNormal(Vector3.new(x * sx, z, -y))
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
			-- Reverse winding when mirrored so the reflected faces stay outward.
			local fid = if mirror then mesh:AddTriangle(vids[a], vids[c], vids[b]) else mesh:AddTriangle(vids[a], vids[b], vids[c])
			if nids then
				if mirror then
					mesh:SetFaceNormals(fid, { nids[a], nids[c], nids[b] })
				else
					mesh:SetFaceNormals(fid, { nids[a], nids[b], nids[c] })
				end
			end
			if uvids then
				if mirror then
					mesh:SetFaceUVs(fid, { uvids[a], uvids[c], uvids[b] })
				else
					mesh:SetFaceUVs(fid, { uvids[a], uvids[b], uvids[c] })
				end
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

-- Suspension rest sag as a fraction of travel; MUST match VehiclePhysics so the
-- wheel rest position lands on its bone.
local SUSP_REST_RATIO = 0.5

-- Body RAGE-space AABB -> Roblox-space (center, size) in studs. The mapping is
-- an axis permutation with a sign flip, so size is taken as absolute extents.
local function bodyAABB(vehicle: VehicleWire.Vehicle): (Vector3, Vector3)
	local mn, mx = vehicle.bodyMin, vehicle.bodyMax
	local center = rageVec((mn + mx) * 0.5)
	local size = Vector3.new(math.abs(mx.X - mn.X), math.abs(mx.Z - mn.Z), math.abs(mx.Y - mn.Y))
		* STUDS_PER_METER
	return center, size
end

-- Set a part's mass (approx) via density, clamped to Roblox's [0.01, 100] range.
local function setMass(part: BasePart, mass: number, friction: number)
	local vol = part.Size.X * part.Size.Y * part.Size.Z
	local density = math.clamp(mass / math.max(vol, 0.01), 0.01, 100)
	part.CustomPhysicalProperties = PhysicalProperties.new(density, friction, 0, 1, 1)
end

-- Build the visual wheels (mirrored on the right) as massless meshes welded to
-- the chassis; the Weld's C0 is driven each frame by VehiclePhysics. Returns the
-- per-wheel config the physics needs (strut hardpoint, front/left flags).
local function buildPhysWheels(
	vehicle: VehicleWire.Vehicle,
	chassis: BasePart,
	chassisCenter: Vector3,
	suspLen: number,
	cache: { [number]: Content }
): { VehiclePhysics.WheelCfg }
	local cfgs: { VehiclePhysics.WheelCfg } = {}
	local wheelGeoms: { VehicleWire.Geometry }? = nil
	for _, p in vehicle.parts do
		if p.name == "wheel" then
			wheelGeoms = p.geometries
			break
		end
	end
	if not wheelGeoms then
		return cfgs
	end

	for _, wheel in vehicle.wheels do
		local part = buildPartMesh(vehicle.buffer, wheelGeoms, wheel.mirror)
		if part then
			applyMaterial(part, cache, wheelGeoms[1])
			part.Name = "wheel"
			part.Massless = true
			part.CanCollide = false
			part.Anchored = false
			part.Parent = chassis

			local posLocal = rageVec(wheel.pos) - chassisCenter
			local weld = Instance.new("Weld")
			weld.Part0 = chassis
			weld.Part1 = part
			weld.C0 = CFrame.new(posLocal)
			weld.Parent = part

			cfgs[#cfgs + 1] = {
				part = part,
				weld = weld,
				hardpoint = posLocal + Vector3.yAxis * (suspLen * SUSP_REST_RATIO),
				front = posLocal.Z < 0, -- RAGE +Y (forward) maps to Roblox -Z
				left = not wheel.mirror,
			}
		end
	end
	return cfgs
end

-- Invisible VehicleSeat (input + seating) welded at the driver position.
local function createSeat(vehicle: VehicleWire.Vehicle, chassis: BasePart, size: Vector3): VehicleSeat
	local seat = Instance.new("VehicleSeat")
	seat.Name = "DriverSeat"
	seat.Size = Vector3.new(4, 1, 3)
	seat.Transparency = 1
	seat.CanCollide = false
	seat.Massless = true
	seat.MaxSpeed = 0 -- we read Throttle/Steer; the seat doesn't actuate
	seat.Torque = 0
	seat.TurnSpeed = 0
	seat.HeadsUpDisplay = false

	-- Driver seat: left side, just above the floor, plus GTA's fSeatOffsetDist.
	local floorY = -size.Y * 0.5
	local seatLocal = Vector3.new(-size.X * 0.18, floorY + 2.4, -size.Z * 0.05)
	if vehicle.handling then
		seatLocal += rageVec(vehicle.handling.seatOffset)
	end
	seat.CFrame = chassis.CFrame * CFrame.new(seatLocal)
	local weld = Instance.new("Weld")
	weld.Part0 = chassis
	weld.Part1 = seat
	weld.C0 = CFrame.new(seatLocal)
	weld.Parent = seat
	seat.Parent = chassis
	return seat
end

-- ProximityPrompt to enter (sit) on the driver door; handbrake (Space) + exit
-- (F) bound while the local player is the occupant. Runs client-side.
local function setupInteraction(
	model: Model,
	chassis: BasePart,
	seat: VehicleSeat,
	rep: { [string]: BasePart },
	handbrake: { v: boolean }
)
	local anchorPart: BasePart = rep["door_dside_f"] or rep["door_dside"] or chassis
	local prompt = Instance.new("ProximityPrompt")
	prompt.ActionText = "Enter"
	prompt.ObjectText = model.Name
	prompt.HoldDuration = 0
	prompt.MaxActivationDistance = 12
	prompt.RequiresLineOfSight = false
	prompt.Parent = anchorPart

	prompt.Triggered:Connect(function(player)
		local char = player.Character
		local hum = char and char:FindFirstChildOfClass("Humanoid")
		if hum and not seat.Occupant then
			seat:Sit(hum)
		end
	end)

	local function bind()
		ContextActionService:BindAction("VehHandbrake", function(_, state)
			handbrake.v = state == Enum.UserInputState.Begin
			return Enum.ContextActionResult.Sink -- swallow the seat's jump-exit
		end, false, Enum.KeyCode.Space)
		ContextActionService:BindAction("VehExit", function(_, state)
			if state == Enum.UserInputState.Begin and seat.Occupant then
				seat.Occupant.Sit = false
			end
			return Enum.ContextActionResult.Sink
		end, false, Enum.KeyCode.F)
	end
	local function unbind()
		handbrake.v = false
		ContextActionService:UnbindAction("VehHandbrake")
		ContextActionService:UnbindAction("VehExit")
	end

	seat:GetPropertyChangedSignal("Occupant"):Connect(function()
		local char = Players.LocalPlayer and Players.LocalPlayer.Character
		local localHum = char and char:FindFirstChildOfClass("Humanoid")
		if seat.Occupant and seat.Occupant == localHum then
			prompt.Enabled = false
			bind()
		else
			prompt.Enabled = true
			unbind()
		end
	end)
end

function VehicleLoader.build(data: buffer | string, name: string, parent: Instance?, pivot: CFrame?): Model
	local vehicle = VehicleWire.parse(data)
	local cache = buildTextureCache(vehicle)
	local h = vehicle.handling

	local model = Instance.new("Model")
	model.Name = name

	local center, size = bodyAABB(vehicle)

	-- Chassis collider: the invisible, mass-bearing physics root. Inset and
	-- floored so the visible body overhangs it and the box never scrapes (the
	-- raycast wheels hold the car up).
	local chassis = Instance.new("Part")
	chassis.Name = "ChassisCollider"
	chassis.Size = Vector3.new(size.X * 0.85, size.Y * 0.55, size.Z * 0.95)
	chassis.CFrame = CFrame.new(center - Vector3.yAxis * size.Y * 0.18)
	chassis.Transparency = 1
	chassis.CanCollide = true
	chassis.Anchored = true -- held until everything is positioned
	chassis.TopSurface = Enum.SurfaceType.Smooth
	chassis.BottomSurface = Enum.SurfaceType.Smooth
	local mass = if h then h.mass else 1400
	setMass(chassis, mass * 0.45, 0.2)
	chassis.Parent = model
	model.PrimaryPart = chassis
	local chassisCenter = chassis.Position

	-- Ballast: carries most of the mass low for GTA-like roll stability (the
	-- real low centre of mass), nudged by handling vecCentreOfMassOffset.
	local ballast = Instance.new("Part")
	ballast.Name = "Ballast"
	ballast.Size = Vector3.new(size.X * 0.5, 1, size.Z * 0.6)
	ballast.Transparency = 1
	ballast.CanCollide = false
	ballast.Anchored = false
	ballast.CFrame = CFrame.new(center - Vector3.yAxis * (size.Y * 0.5 - 1))
	if h then
		ballast.CFrame += rageVec(h.comOffset)
	end
	setMass(ballast, mass * 0.55, 0)
	ballast.Parent = model
	do
		local w = Instance.new("WeldConstraint")
		w.Part0 = chassis
		w.Part1 = ballast
		w.Parent = ballast
	end

	-- Each bone-part's FIRST MeshPart is its representative; sub-meshes parent
	-- under it, child bone-parts under the parent bone's rep (GTA hierarchy).
	local rep: { [string]: BasePart } = {}
	local function place(pname: string, part: BasePart)
		local r = rep[pname]
		if r then
			part.Parent = r
		else
			rep[pname] = part
			part.Parent = model
		end
	end
	local function weldToChassis(part: BasePart)
		local w = Instance.new("WeldConstraint")
		w.Part0 = chassis
		w.Part1 = part
		w.Parent = part
	end

	-- Body: massless visual meshes welded rigidly to the chassis.
	for _, p in vehicle.parts do
		if not p.articulated and p.name ~= "wheel" then
			for _, geoms in groupByTexture(p.geometries) do
				local part = buildPartMesh(vehicle.buffer, geoms)
				if part then
					part.Anchored = false
					part.CanCollide = false
					part.Massless = true
					applyMaterial(part, cache, geoms[1])
					part.Name = p.name
					place(p.name, part)
					weldToChassis(part)
				end
			end
		end
	end

	-- Articulated parts: Servo hinge to the chassis (doors/hood/boot). Held
	-- anchored until release so they don't droop before the servo engages.
	local articulated: { BasePart } = {}
	for _, p in vehicle.parts do
		if p.articulated and p.hinge then
			local hh = p.hinge
			local root: BasePart? = nil
			for _, geoms in groupByTexture(p.geometries) do
				local part = buildPartMesh(vehicle.buffer, geoms)
				if part then
					part.Anchored = true
					part.CanCollide = false
					applyMaterial(part, cache, geoms[1])
					part.Name = p.name
					place(p.name, part)
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
				local cf = axisCFrame(rageVec(hh.pos), rageDir(hh.axis))
				local a0 = Instance.new("Attachment")
				a0.WorldCFrame = cf
				a0.Parent = chassis
				local a1 = Instance.new("Attachment")
				a1.WorldCFrame = cf
				a1.Parent = root

				local hinge = Instance.new("HingeConstraint")
				hinge.Name = p.name
				hinge.Attachment0 = a0
				hinge.Attachment1 = a1
				hinge.LimitsEnabled = true
				hinge.LowerAngle = math.min(0, math.deg(hh.minAngle))
				hinge.UpperAngle = math.max(5, math.deg(hh.maxAngle))
				hinge.ActuatorType = Enum.ActuatorType.Servo
				hinge.ServoMaxTorque = 100000
				hinge.AngularSpeed = 2
				hinge.TargetAngle = 0
				hinge.Parent = root
			end
		end
	end

	local suspLen =
		math.max((if h then (h.suspensionUpperLimit - h.suspensionLowerLimit) else 0.25) * STUDS_PER_METER, 0.3)
	local wheelCfgs = buildPhysWheels(vehicle, chassis, chassisCenter, suspLen, cache)
	local seat = createSeat(vehicle, chassis, size)

	-- Parent each part's representative under its parent (GTA bone hierarchy).
	for _, p in vehicle.parts do
		local r = rep[p.name]
		if r and p.parent ~= "" and rep[p.parent] and p.parent ~= p.name then
			r.Parent = rep[p.parent]
		end
	end

	model.Parent = parent or workspace
	if pivot then
		model:PivotTo(pivot)
	end

	-- Release: unanchor the chassis (welded body/wheels/ballast follow) and the
	-- hinged parts, so the car drops onto its suspension.
	chassis.Anchored = false
	for _, r in articulated do
		r.Anchored = false
	end

	-- Drive it: raycast suspension physics + character interaction.
	if h then
		local rp = RaycastParams.new()
		rp.FilterType = Enum.RaycastFilterType.Exclude
		rp.FilterDescendantsInstances = { model }
		local handbrake = { v = false }
		local controller = VehiclePhysics.start({
			chassis = chassis,
			seat = seat,
			wheels = wheelCfgs,
			handling = h,
			wheelRadius = vehicle.wheelRadius * STUDS_PER_METER,
			suspensionLength = suspLen,
			raycastParams = rp,
			getHandbrake = function()
				return handbrake.v
			end,
		})
		model:SetAttribute("HasPhysics", true)
		model.Destroying:Connect(function()
			controller:destroy()
		end)
		setupInteraction(model, chassis, seat, rep, handbrake)
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
