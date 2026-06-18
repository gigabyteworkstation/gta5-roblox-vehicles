--!strict
-- Parses the "GVEH" v3 (parts) wire format. The vehicle is a list of rigid
-- parts: a "body" plus articulated parts (doors/hood/boot) each with a hinge.

local BinaryReader = require(script.Parent.BinaryReader)

local VehicleWire = {}

export type Geometry = {
	shaderIndex: number,
	diffuseIndex: number, -- index into Vehicle.textures, -1 = none
	normalMapIndex: number, -- -1 = none
	glass: boolean,
	vertexCount: number,
	indexCount: number,
	hasNormals: boolean,
	hasUVs: boolean,
	posOffset: number,
	normalOffset: number?,
	uvOffset: number?,
	indexOffset: number,
}

export type Hinge = {
	pos: Vector3, -- RAGE space
	axis: Vector3, -- RAGE space
	minAngle: number, -- radians
	maxAngle: number,
}

export type Part = {
	name: string,
	parent: string, -- parent part name (GTA bone hierarchy); "" = top
	articulated: boolean,
	hinge: Hinge?,
	geometries: { Geometry },
}

export type Texture = { name: string, width: number, height: number, rgbaOffset: number }

export type Wheel = { pos: Vector3, mirror: boolean } -- pos in RAGE space

-- GTA handling.meta CHandlingData (the inputs to RAGE's wheel/suspension model).
export type Handling = {
	mass: number,
	dragCoeff: number,
	comOffset: Vector3,
	inertiaMult: Vector3,
	driveBiasFront: number,
	driveGears: number,
	driveForce: number,
	driveMaxFlatVel: number,
	brakeForce: number,
	brakeBiasFront: number,
	handbrakeForce: number,
	steeringLock: number, -- degrees
	tractionCurveMax: number,
	tractionCurveMin: number,
	tractionCurveLateral: number,
	tractionBiasFront: number,
	lowSpeedTractionLoss: number,
	suspensionForce: number,
	suspensionCompDamp: number,
	suspensionReboundDamp: number,
	suspensionUpperLimit: number, -- metres
	suspensionLowerLimit: number,
	suspensionRaise: number,
	suspensionBiasFront: number,
	antiRollForce: number,
	antiRollBiasFront: number,
	seatOffset: Vector3,
}

export type Vehicle = {
	buffer: buffer,
	parts: { Part },
	textures: { Texture },
	wheels: { Wheel },
	wheelRadius: number, -- metres (RAGE)
	bodyMin: Vector3, -- RAGE-space body AABB
	bodyMax: Vector3,
	handling: Handling?,
}

local MAGIC = "GVEH"

local function readGeometry(r: BinaryReader.BinaryReader): Geometry
	local shaderIndex = r:u16()
	local diffuseIndex = r:i16()
	local normalMapIndex = r:i16()
	local flags = r:u8()
	r:u8()
	local vcount = r:u32()
	local icount = r:u32()
	local hasN = bit32.band(flags, 1) == 1
	local hasUV = bit32.band(flags, 2) == 2
	local glass = bit32.band(flags, 4) == 4

	local posOffset = r:tell()
	r:skip(vcount * 12)
	local normalOffset: number? = nil
	if hasN then
		normalOffset = r:tell()
		r:skip(vcount * 12)
	end
	local uvOffset: number? = nil
	if hasUV then
		uvOffset = r:tell()
		r:skip(vcount * 8)
	end
	local indexOffset = r:tell()
	r:skip(icount * 2)

	return {
		shaderIndex = shaderIndex,
		diffuseIndex = diffuseIndex,
		normalMapIndex = normalMapIndex,
		glass = glass,
		vertexCount = vcount,
		indexCount = icount,
		hasNormals = hasN,
		hasUVs = hasUV,
		posOffset = posOffset,
		normalOffset = normalOffset,
		uvOffset = uvOffset,
		indexOffset = indexOffset,
	}
end

function VehicleWire.parse(data: buffer | string): Vehicle
	local buf = if type(data) == "string" then buffer.fromstring(data) else data
	local r = BinaryReader.new(buf)

	assert(r:fixedString(4) == MAGIC, "bad magic (expected GVEH)")
	local _version = r:u8()
	local hasHandling = r:u8() == 1
	r:skip(2)
	local partCount = r:u32()
	local texCount = r:u32()
	local wheelCount = r:u32()

	-- Physical header: wheel radius + body AABB (RAGE space), then handling.
	local wheelRadius = r:f32()
	local bodyMin = Vector3.new(r:f32(), r:f32(), r:f32())
	local bodyMax = Vector3.new(r:f32(), r:f32(), r:f32())
	local handling: Handling? = nil
	if hasHandling then
		handling = {
			mass = r:f32(),
			dragCoeff = r:f32(),
			comOffset = Vector3.new(r:f32(), r:f32(), r:f32()),
			inertiaMult = Vector3.new(r:f32(), r:f32(), r:f32()),
			driveBiasFront = r:f32(),
			driveGears = r:f32(),
			driveForce = r:f32(),
			driveMaxFlatVel = r:f32(),
			brakeForce = r:f32(),
			brakeBiasFront = r:f32(),
			handbrakeForce = r:f32(),
			steeringLock = r:f32(),
			tractionCurveMax = r:f32(),
			tractionCurveMin = r:f32(),
			tractionCurveLateral = r:f32(),
			tractionBiasFront = r:f32(),
			lowSpeedTractionLoss = r:f32(),
			suspensionForce = r:f32(),
			suspensionCompDamp = r:f32(),
			suspensionReboundDamp = r:f32(),
			suspensionUpperLimit = r:f32(),
			suspensionLowerLimit = r:f32(),
			suspensionRaise = r:f32(),
			suspensionBiasFront = r:f32(),
			antiRollForce = r:f32(),
			antiRollBiasFront = r:f32(),
			seatOffset = Vector3.new(r:f32(), r:f32(), r:f32()),
		}
	end

	local parts: { Part } = table.create(partCount) :: { Part }
	for i = 1, partCount do
		local nameLen = r:u16()
		local name = r:fixedString(nameLen)
		local parentLen = r:u16()
		local parent = r:fixedString(parentLen)
		local articulated = r:u8() == 1
		r:u8()

		local hinge: Hinge? = nil
		if articulated then
			local px, py, pz = r:f32(), r:f32(), r:f32()
			local ax, ay, az = r:f32(), r:f32(), r:f32()
			local minA = r:f32()
			local maxA = r:f32()
			hinge = {
				pos = Vector3.new(px, py, pz),
				axis = Vector3.new(ax, ay, az),
				minAngle = minA,
				maxAngle = maxA,
			}
		end

		local geomCount = r:u32()
		local geometries: { Geometry } = table.create(geomCount) :: { Geometry }
		for j = 1, geomCount do
			geometries[j] = readGeometry(r)
		end

		parts[i] = { name = name, parent = parent, articulated = articulated, hinge = hinge, geometries = geometries }
	end

	local textures: { Texture } = table.create(texCount) :: { Texture }
	for i = 1, texCount do
		local nameLen = r:u16()
		local name = r:fixedString(nameLen)
		local width = r:u16()
		local height = r:u16()
		local rgbaOffset = r:tell()
		r:skip(width * height * 4)
		textures[i] = { name = name, width = width, height = height, rgbaOffset = rgbaOffset }
	end

	local wheels: { Wheel } = table.create(wheelCount) :: { Wheel }
	for i = 1, wheelCount do
		local x, y, z = r:f32(), r:f32(), r:f32()
		local mirror = r:u8() == 1
		r:skip(3)
		wheels[i] = { pos = Vector3.new(x, y, z), mirror = mirror }
	end

	return {
		buffer = buf,
		parts = parts,
		textures = textures,
		wheels = wheels,
		wheelRadius = wheelRadius,
		bodyMin = bodyMin,
		bodyMax = bodyMax,
		handling = handling,
	}
end

return VehicleWire
