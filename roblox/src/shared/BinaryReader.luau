--!strict
-- Minimal little-endian cursor over a Roblox buffer, for the GVEH wire format.

local BinaryReader = {}
BinaryReader.__index = BinaryReader

export type BinaryReader = typeof(setmetatable(
	{} :: { buf: buffer, offset: number },
	BinaryReader
))

function BinaryReader.new(buf: buffer): BinaryReader
	return setmetatable({ buf = buf, offset = 0 }, BinaryReader)
end

function BinaryReader:u8(): number
	local v = buffer.readu8(self.buf, self.offset)
	self.offset += 1
	return v
end

function BinaryReader:u16(): number
	local v = buffer.readu16(self.buf, self.offset)
	self.offset += 2
	return v
end

function BinaryReader:u32(): number
	local v = buffer.readu32(self.buf, self.offset)
	self.offset += 4
	return v
end

function BinaryReader:f32(): number
	local v = buffer.readf32(self.buf, self.offset)
	self.offset += 4
	return v
end

function BinaryReader:fixedString(n: number): string
	local s = buffer.readstring(self.buf, self.offset, n)
	self.offset += n
	return s
end

function BinaryReader:skip(n: number)
	self.offset += n
end

function BinaryReader:tell(): number
	return self.offset
end

return BinaryReader
