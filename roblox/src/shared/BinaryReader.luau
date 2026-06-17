--!strict
-- Minimal little-endian cursor over a Roblox buffer, for the GVEH wire format.

local BinaryReader = {}
BinaryReader.__index = BinaryReader

export type BinaryReader = {
	buf: buffer,
	offset: number,
	u8: (self: BinaryReader) -> number,
	u16: (self: BinaryReader) -> number,
	u32: (self: BinaryReader) -> number,
	f32: (self: BinaryReader) -> number,
	fixedString: (self: BinaryReader, n: number) -> string,
	skip: (self: BinaryReader, n: number) -> (),
	tell: (self: BinaryReader) -> number,
}

function BinaryReader.new(buf: buffer): BinaryReader
	return (setmetatable({ buf = buf, offset = 0 }, BinaryReader) :: any) :: BinaryReader
end

function BinaryReader.u8(self: BinaryReader): number
	local v = buffer.readu8(self.buf, self.offset)
	self.offset += 1
	return v
end

function BinaryReader.u16(self: BinaryReader): number
	local v = buffer.readu16(self.buf, self.offset)
	self.offset += 2
	return v
end

function BinaryReader.u32(self: BinaryReader): number
	local v = buffer.readu32(self.buf, self.offset)
	self.offset += 4
	return v
end

function BinaryReader.f32(self: BinaryReader): number
	local v = buffer.readf32(self.buf, self.offset)
	self.offset += 4
	return v
end

function BinaryReader.fixedString(self: BinaryReader, n: number): string
	local s = buffer.readstring(self.buf, self.offset, n)
	self.offset += n
	return s
end

function BinaryReader.skip(self: BinaryReader, n: number)
	self.offset += n
end

function BinaryReader.tell(self: BinaryReader): number
	return self.offset
end

return BinaryReader
