--!strict
-- Parses the "GVEH" wire format from the backend into geometry/texture records.
-- Keeps the raw buffer and stores per-geometry byte offsets (no giant
-- intermediate vertex tables) so the loader can read straight into EditableMesh.

local BinaryReader = require(script.Parent.BinaryReader)

local VehicleWire = {}

export type Geometry = {
	shaderIndex: number,
	vertexCount: number,
	indexCount: number,
	hasNormals: boolean,
	hasUVs: boolean,
	posOffset: number,
	normalOffset: number?,
	uvOffset: number?,
	indexOffset: number,
}

export type Texture = {
	name: string,
	width: number,
	height: number,
	rgbaOffset: number,
}

export type Vehicle = {
	buffer: buffer,
	geometries: { Geometry },
	textures: { Texture },
}

local MAGIC = "GVEH"

function VehicleWire.parse(data: string): Vehicle
	local buf = buffer.fromstring(data)
	local r = BinaryReader.new(buf)

	local magic = r:fixedString(4)
	assert(magic == MAGIC, `bad magic '{magic}' (expected GVEH)`)
	local _version = r:u8()
	r:skip(3) -- reserved
	local geomCount = r:u32()
	local texCount = r:u32()

	local geometries: { Geometry } = table.create(geomCount)
	for i = 1, geomCount do
		local shaderIndex = r:u16()
		local flags = r:u8()
		r:u8() -- reserved
		local vcount = r:u32()
		local icount = r:u32()
		local hasN = bit32.band(flags, 1) == 1
		local hasUV = bit32.band(flags, 2) == 2

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

		geometries[i] = {
			shaderIndex = shaderIndex,
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

	local textures: { Texture } = table.create(texCount)
	for i = 1, texCount do
		local nameLen = r:u16()
		local name = r:fixedString(nameLen)
		local width = r:u16()
		local height = r:u16()
		local rgbaOffset = r:tell()
		r:skip(width * height * 4)
		textures[i] = { name = name, width = width, height = height, rgbaOffset = rgbaOffset }
	end

	return { buffer = buf, geometries = geometries, textures = textures }
end

return VehicleWire
