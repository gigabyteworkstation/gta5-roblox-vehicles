--!strict
-- Parses the "GVEH" v2 wire format into geometry/texture/bone records.
-- Keeps the raw buffer and stores per-geometry byte offsets so the loader can
-- read straight into EditableMesh.

local BinaryReader = require(script.Parent.BinaryReader)

local VehicleWire = {}

export type Geometry = {
	shaderIndex: number,
	vertexCount: number,
	indexCount: number,
	hasNormals: boolean,
	hasUVs: boolean,
	skinned: boolean,
	posOffset: number,
	normalOffset: number?,
	uvOffset: number?,
	indexOffset: number,
	skinOffset: number?, -- per vertex: u16 boneIdx[4], u8 weight[4]
}

export type Texture = {
	name: string,
	width: number,
	height: number,
	rgbaOffset: number,
}

export type Bone = {
	name: string,
	parent: number, -- index into bones (1-based here); -1 = root
	cframe: CFrame, -- RAGE-space world rest CFrame (position + rotation)
}

export type Vehicle = {
	buffer: buffer,
	geometries: { Geometry },
	textures: { Texture },
	bones: { Bone },
}

local MAGIC = "GVEH"

function VehicleWire.parse(data: buffer | string): Vehicle
	local buf = if type(data) == "string" then buffer.fromstring(data) else data
	local r = BinaryReader.new(buf)

	local magic = r:fixedString(4)
	assert(magic == MAGIC, `bad magic '{magic}' (expected GVEH)`)
	local _version = r:u8()
	r:skip(3) -- reserved
	local geomCount = r:u32()
	local texCount = r:u32()
	local boneCount = r:u32()

	local geometries: { Geometry } = table.create(geomCount) :: { Geometry }
	for i = 1, geomCount do
		local shaderIndex = r:u16()
		local flags = r:u8()
		r:u8() -- reserved
		local vcount = r:u32()
		local icount = r:u32()
		local hasN = bit32.band(flags, 1) == 1
		local hasUV = bit32.band(flags, 2) == 2
		local skinned = bit32.band(flags, 4) == 4

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
		local skinOffset: number? = nil
		if skinned then
			skinOffset = r:tell()
			r:skip(vcount * 12) -- 4*u16 + 4*u8
		end

		geometries[i] = {
			shaderIndex = shaderIndex,
			vertexCount = vcount,
			indexCount = icount,
			hasNormals = hasN,
			hasUVs = hasUV,
			skinned = skinned,
			posOffset = posOffset,
			normalOffset = normalOffset,
			uvOffset = uvOffset,
			indexOffset = indexOffset,
			skinOffset = skinOffset,
		}
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

	local bones: { Bone } = table.create(boneCount) :: { Bone }
	for i = 1, boneCount do
		local nameLen = r:u16()
		local name = r:fixedString(nameLen)
		local parent = r:i16()
		local px, py, pz = r:f32(), r:f32(), r:f32()
		local qx, qy, qz, qw = r:f32(), r:f32(), r:f32(), r:f32()
		bones[i] = { name = name, parent = parent, cframe = CFrame.new(px, py, pz, qx, qy, qz, qw) }
	end

	return { buffer = buf, geometries = geometries, textures = textures, bones = bones }
end

return VehicleWire
