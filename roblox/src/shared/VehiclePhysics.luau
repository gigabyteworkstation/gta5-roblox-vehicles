--!strict
-- Raycast-suspension vehicle physics, modelled on RAGE (GTA5): each wheel is a
-- downward probe (CWheel::ProbeSurface) carrying a spring+damper strut, a
-- longitudinal drive/brake force, and a lateral grip force clamped to a friction
-- circle from the traction curve. All forces are applied to the single chassis
-- assembly; the visual wheel meshes are positioned/spun kinematically via a Weld.
--
-- Inputs come from a VehicleSeat (Throttle/Steer = WASD) plus an optional
-- handbrake getter. Numbers come straight from handling.meta (see VehicleWire).

local RunService = game:GetService("RunService")

local VehicleWire = require(script.Parent.VehicleWire)

local VehiclePhysics = {}
VehiclePhysics.__index = VehiclePhysics

-- Tuning that maps GTA's normalized handling units into Roblox force land.
-- These scale the *whole* class of vehicles; the per-vehicle feel still comes
-- from handling.meta. Tweak here if everything is uniformly too soft/strong.
local DRIVE_SCALE = 1.0
local BRAKE_SCALE = 1.3
local REVERSE_SCALE = 0.45
local HANDBRAKE_SCALE = 2.0
local TOPSPEED_MPS = 0.34 -- fInitialDriveMaxFlatVel -> m/s
local SUSP_REST_RATIO = 0.5 -- target sag (fraction of travel) at rest
local DAMP_SCALE = 0.12 -- maps GTA comp/rebound damp to a force coefficient
local GRIP_SCALE = 1.0
local ANTIROLL_SCALE = 1.0
local DRAG_SCALE = 0.0016
local STEER_SPEED = 4.0 -- rad/s the road wheels turn toward target
local STEER_SPEED_FALLOFF = 0.65 -- how much lock is cut at high speed

local SPM = 4 -- studs per metre (matches VehicleLoader)

export type WheelCfg = {
	part: BasePart,
	weld: Weld,
	hardpoint: Vector3, -- chassis-local strut-top position
	front: boolean,
	left: boolean,
}

export type Config = {
	chassis: BasePart,
	seat: VehicleSeat,
	wheels: { WheelCfg },
	handling: VehicleWire.Handling,
	wheelRadius: number, -- studs
	suspensionLength: number, -- studs (total travel)
	raycastParams: RaycastParams,
	getHandbrake: (() -> boolean)?,
}

type WheelState = {
	cfg: WheelCfg,
	stiffness: number,
	lastLength: number,
	spin: number,
	-- per-frame scratch
	length: number,
	compression: number,
	grounded: boolean,
	hpWorld: Vector3,
	hitPos: Vector3,
	vel: number,
}

function VehiclePhysics.start(config: Config)
	local self = setmetatable({}, VehiclePhysics) :: any
	self.config = config
	self.steerAngle = 0 -- current road-wheel angle (radians)

	local h = config.handling
	local mass = config.chassis.AssemblyMass
	local g = workspace.Gravity
	local n = #config.wheels
	local perWheelLoad = (mass * g) / math.max(n, 1)
	local restComp = config.suspensionLength * SUSP_REST_RATIO
	local baseStiffness = perWheelLoad / math.max(restComp, 0.01)

	-- Drive/brake distribution (front vs rear) and their normalisers.
	self.driveShareSum = 0
	self.brakeShareSum = 0
	local states: { WheelState } = {}
	for _, w in config.wheels do
		-- Suspension bias shifts stiffness front/rear (sum stays ~2*base).
		local biasF = h.suspensionBiasFront
		local stiff = baseStiffness * 2 * (if w.front then biasF else (1 - biasF))
		states[#states + 1] = {
			cfg = w,
			stiffness = stiff,
			lastLength = config.suspensionLength,
			spin = 0,
			length = config.suspensionLength,
			compression = 0,
			grounded = false,
			hpWorld = Vector3.zero,
			hitPos = Vector3.zero,
			vel = 0,
		}
		self.driveShareSum += (if w.front then h.driveBiasFront else (1 - h.driveBiasFront))
		self.brakeShareSum += (if w.front then h.brakeBiasFront else (1 - h.brakeBiasFront))
	end
	self.states = states
	self.baseStiffness = baseStiffness
	self.mass = mass

	self.conn = RunService.Stepped:Connect(function(_, dt)
		self:step(dt)
	end)
	return self
end

function VehiclePhysics.step(self: any, dt: number)
	if dt <= 0 or dt > 0.2 then
		return -- skip absurd frames (alt-tab, first frame)
	end
	local cfg = self.config
	local chassis: BasePart = cfg.chassis
	local h = cfg.handling
	local cf = chassis.CFrame
	local up = cf.UpVector
	local rayLen = cfg.suspensionLength + cfg.wheelRadius

	-- Input.
	local throttle = cfg.seat.ThrottleFloat
	local steerInput = cfg.seat.SteerFloat
	local handbrake = if cfg.getHandbrake then cfg.getHandbrake() else false

	-- Chassis forward speed (m/s) for taper + steer falloff.
	local vel = chassis.AssemblyLinearVelocity
	local fwdSpeedStuds = vel:Dot(cf.LookVector)
	local speedMps = vel.Magnitude / SPM

	-- Speed-sensitive steering toward the input target.
	local lockRad = math.rad(h.steeringLock)
	local falloff = 1 - math.clamp(speedMps / 40, 0, 1) * STEER_SPEED_FALLOFF
	local steerTarget = -steerInput * lockRad * falloff
	self.steerAngle += math.clamp(steerTarget - self.steerAngle, -STEER_SPEED * dt, STEER_SPEED * dt)

	-- Pass 1: probe every wheel, record compression.
	for _, s in self.states do
		local hpWorld = cf:PointToWorldSpace(s.cfg.hardpoint)
		s.hpWorld = hpWorld
		local res = workspace:Raycast(hpWorld, -up * rayLen, cfg.raycastParams)
		local length: number
		if res then
			local d = (hpWorld - res.Position).Magnitude
			length = math.clamp(d - cfg.wheelRadius, 0, cfg.suspensionLength)
			s.grounded = true
			s.hitPos = res.Position
		else
			length = cfg.suspensionLength
			s.grounded = false
		end
		s.compression = cfg.suspensionLength - length
		s.vel = (s.lastLength - length) / dt -- + = compressing
		s.lastLength = length
		s.length = length
	end

	-- Anti-roll: stiffen the axle against left/right compression difference.
	local antiRoll = h.antiRollForce * self.baseStiffness * ANTIROLL_SCALE
	local function rollMate(s: WheelState): WheelState?
		for _, o in self.states do
			if o ~= s and o.cfg.front == s.cfg.front then
				return o
			end
		end
		return nil
	end

	-- Drive/brake totals (RAGE: fInitialDriveForce/fBrakeForce * weight).
	local weight = self.mass * workspace.Gravity
	local topStuds = h.driveMaxFlatVel * TOPSPEED_MPS * SPM
	local driveForceTotal = h.driveForce * weight * DRIVE_SCALE
	local taperFwd = math.clamp(1 - fwdSpeedStuds / math.max(topStuds, 1), 0, 1)
	local taperRev = math.clamp(1 + fwdSpeedStuds / math.max(topStuds * 0.4, 1), 0, 1)

	-- Pass 2: apply suspension + anti-roll + tyre forces.
	for _, s in self.states do
		if not s.grounded then
			-- Airborne: just let the visual wheel hang at full droop & coast spin.
			self:placeWheel(s, dt, fwdSpeedStuds)
			continue
		end

		-- Suspension spring + damper (+ anti-roll).
		local dampCoeff = (if s.vel > 0 then h.suspensionCompDamp else h.suspensionReboundDamp)
			* DAMP_SCALE
			* s.stiffness
		local springForce = s.stiffness * s.compression + dampCoeff * s.vel
		local mate = rollMate(s)
		if mate then
			springForce += (s.compression - mate.compression) * antiRoll
		end
		springForce = math.max(springForce, 0)
		local load = springForce
		chassis:ApplyImpulseAtPosition(up * springForce * dt, s.hpWorld)

		-- Wheel directions (front wheels steer).
		local steer = if s.cfg.front then self.steerAngle else 0
		local cosS, sinS = math.cos(steer), math.sin(steer)
		local fwd = cf.LookVector * cosS + cf.RightVector * sinS
		local right = cf.RightVector * cosS - cf.LookVector * sinS

		local contactVel = chassis:GetVelocityAtPosition(s.hitPos)
		local vForward = contactVel:Dot(fwd)
		local vLateral = contactVel:Dot(right)

		-- Longitudinal: drive / brake / reverse.
		local long = 0
		if throttle > 0.05 then
			local share = (if s.cfg.front then h.driveBiasFront else (1 - h.driveBiasFront))
				/ math.max(self.driveShareSum, 0.01)
			long += throttle * driveForceTotal * taperFwd * share
		elseif throttle < -0.05 then
			if vForward > 1 then
				-- Foot brake.
				local share = (if s.cfg.front then h.brakeBiasFront else (1 - h.brakeBiasFront))
					/ math.max(self.brakeShareSum, 0.01)
				long -= (-throttle) * h.brakeForce * weight * BRAKE_SCALE * share
			else
				-- Reverse.
				local share = (if s.cfg.front then h.driveBiasFront else (1 - h.driveBiasFront))
					/ math.max(self.driveShareSum, 0.01)
				long -= (-throttle) * driveForceTotal * REVERSE_SCALE * taperRev * share
			end
		end
		-- Handbrake locks the rear: hard brake + reduced lateral grip (drift).
		local latMult = 1
		if handbrake and not s.cfg.front then
			long = -math.sign(vForward) * h.handbrakeForce * weight * HANDBRAKE_SCALE / #self.states
			latMult = 0.35
		end

		-- Lateral grip: cancel sideways slip up to the friction budget.
		local perWheelMass = self.mass / #self.states
		local desiredLat = -vLateral * perWheelMass / dt
		local mu = h.tractionCurveMax * GRIP_SCALE
		local maxF = mu * load
		local longC = math.clamp(long, -maxF, maxF)
		local latBudget = math.sqrt(math.max(maxF * maxF - longC * longC, 0)) * latMult
		local lat = math.clamp(desiredLat, -latBudget, latBudget)

		local impulse = (fwd * longC + right * lat) * dt
		chassis:ApplyImpulseAtPosition(impulse, s.hitPos)

		self:placeWheel(s, dt, vForward)
	end

	-- Aerodynamic drag (keeps top speed sane alongside the drive taper).
	local dragF = -vel * vel.Magnitude * h.dragCoeff * DRAG_SCALE
	chassis:ApplyImpulseAtPosition(dragF * dt, chassis.Position)
end

-- Position + spin the visual wheel via its Weld C0 (chassis-local).
function VehiclePhysics.placeWheel(self: any, s: WheelState, dt: number, surfaceSpeed: number)
	local centerLocal = s.cfg.hardpoint - Vector3.yAxis * s.length
	local steer = if s.cfg.front then self.steerAngle else 0
	s.spin += (surfaceSpeed / math.max(self.config.wheelRadius, 0.01)) * dt
	s.cfg.weld.C0 = CFrame.new(centerLocal)
		* CFrame.Angles(0, steer, 0)
		* CFrame.Angles(s.spin, 0, 0)
end

function VehiclePhysics.destroy(self: any)
	if self.conn then
		self.conn:Disconnect()
		self.conn = nil
	end
end

return VehiclePhysics
