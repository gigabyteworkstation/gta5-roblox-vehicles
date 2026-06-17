//! RSC7 paged-resource reader.
//!
//! `rpf_archive::extract` hands us a reconstructed RSC7 file:
//!   [16-byte header: magic, version, system_flags, graphics_flags]
//!   [deflated body = system segment bytes ‖ graphics segment bytes]
//!
//! RAGE resources are a snapshot of two virtual memory segments. Pointers stored
//! in the data are 32-bit tagged addresses (CodeWalker model):
//!   top nibble 0x5 -> system segment,  0x6 -> graphics segment
//!   low 28 bits    -> byte offset within that segment
//! On PC (64-bit) pointers occupy 8 bytes but only the low 32 carry meaning.

use anyhow::{bail, Context, Result};
use flate2::read::DeflateDecoder;
use std::io::Read;

pub const RSC7_MAGIC: u32 = 0x37435352; // 'RSC7'

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    System,
    Graphics,
}

pub struct Rsc7 {
    pub version: u32,
    pub system_flags: u32,
    pub graphics_flags: u32,
    /// compressed body length (as returned by extract, minus the 16-byte header)
    pub compressed_len: usize,
    pub system: Vec<u8>,
    pub graphics: Vec<u8>,
}

/// Decode a segment's byte size from its flags word. Ported verbatim from
/// rpf_archive::resource_size_from_flags (kept local so we don't depend on it
/// being exported).
pub fn size_from_flags(flags: u32) -> usize {
    let s0 = ((flags >> 27) & 0x1) << 0;
    let s1 = ((flags >> 26) & 0x1) << 1;
    let s2 = ((flags >> 25) & 0x1) << 2;
    let s3 = ((flags >> 24) & 0x1) << 3;
    let s4 = ((flags >> 17) & 0x7F) << 4;
    let s5 = ((flags >> 11) & 0x3F) << 5;
    let s6 = ((flags >> 7) & 0xF) << 6;
    let s7 = ((flags >> 5) & 0x3) << 7;
    let s8 = ((flags >> 4) & 0x1) << 8;
    let ss = (flags & 0xF) as usize;
    let base = 0x200usize << ss;
    base * (s0 + s1 + s2 + s3 + s4 + s5 + s6 + s7 + s8) as usize
}

pub fn version_from_flags(sys_flags: u32, gfx_flags: u32) -> u32 {
    let sv = (sys_flags >> 28) & 0xF;
    let gv = (gfx_flags >> 28) & 0xF;
    (sv << 4) | gv
}

impl Rsc7 {
    pub fn parse(rsc: &[u8]) -> Result<Rsc7> {
        if rsc.len() < 16 {
            bail!("RSC7 too small ({} bytes)", rsc.len());
        }
        let magic = u32::from_le_bytes(rsc[0..4].try_into().unwrap());
        if magic != RSC7_MAGIC {
            bail!("not an RSC7 resource (magic = 0x{magic:08X})");
        }
        let version = u32::from_le_bytes(rsc[4..8].try_into().unwrap());
        let system_flags = u32::from_le_bytes(rsc[8..12].try_into().unwrap());
        let graphics_flags = u32::from_le_bytes(rsc[12..16].try_into().unwrap());

        let sys_size = size_from_flags(system_flags);
        let gfx_size = size_from_flags(graphics_flags);
        let body = &rsc[16..];

        // The body is raw deflate (extract() leaves resources compressed).
        let mut data = Vec::with_capacity(sys_size + gfx_size);
        DeflateDecoder::new(body)
            .read_to_end(&mut data)
            .context("inflating RSC7 body")?;

        if data.len() < sys_size {
            bail!(
                "decompressed {} bytes < system segment {}",
                data.len(),
                sys_size
            );
        }

        let system = data[..sys_size].to_vec();
        let graphics = if data.len() >= sys_size + gfx_size {
            data[sys_size..sys_size + gfx_size].to_vec()
        } else {
            data[sys_size..].to_vec()
        };

        Ok(Rsc7 {
            version,
            system_flags,
            graphics_flags,
            compressed_len: body.len(),
            system,
            graphics,
        })
    }

    fn seg(&self, s: Segment) -> &[u8] {
        match s {
            Segment::System => &self.system,
            Segment::Graphics => &self.graphics,
        }
    }

    /// Classify a tagged pointer. Returns None for null (0).
    pub fn classify(ptr: u64) -> Option<(Segment, usize)> {
        if ptr == 0 {
            return None;
        }
        let p = ptr as u32;
        let seg = (p >> 28) & 0xF;
        let off = (p & 0x0FFF_FFFF) as usize;
        match seg {
            5 => Some((Segment::System, off)),
            6 => Some((Segment::Graphics, off)),
            _ => None, // not a segment-tagged pointer
        }
    }

    /// Borrow a slice of `len` bytes at a tagged pointer.
    pub fn at(&self, ptr: u64, len: usize) -> Result<&[u8]> {
        let (s, off) = Self::classify(ptr).context("null/invalid pointer")?;
        let seg = self.seg(s);
        seg.get(off..off + len)
            .with_context(|| format!("pointer 0x{ptr:08X} (off {off}, len {len}) out of {s:?} segment ({})", seg.len()))
    }

    // Typed reads at (tagged pointer + field offset). The +off arithmetic is safe
    // because the offset lives in the low 28 bits and field offsets are tiny.
    pub fn u16_at(&self, ptr: u64, off: usize) -> Result<u16> {
        let b = self.at(ptr + off as u64, 2)?;
        Ok(u16::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn u32_at(&self, ptr: u64, off: usize) -> Result<u32> {
        let b = self.at(ptr + off as u64, 4)?;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn u64_at(&self, ptr: u64, off: usize) -> Result<u64> {
        let b = self.at(ptr + off as u64, 8)?;
        Ok(u64::from_le_bytes(b.try_into().unwrap()))
    }
    pub fn u8_at(&self, ptr: u64, off: usize) -> Result<u8> {
        Ok(self.at(ptr + off as u64, 1)?[0])
    }

    /// Read a null-terminated ASCII string at a tagged pointer.
    pub fn str_at(&self, ptr: u64) -> String {
        let (s, off) = match Self::classify(ptr) {
            Some(v) => v,
            None => return String::new(),
        };
        let seg = self.seg(s);
        let mut end = off;
        while end < seg.len() && seg[end] != 0 {
            end += 1;
        }
        String::from_utf8_lossy(&seg[off..end]).into_owned()
    }

    /// Read `count` consecutive u64 pointers starting at a tagged pointer.
    pub fn ptr_array(&self, ptr: u64, count: usize) -> Result<Vec<u64>> {
        let bytes = self.at(ptr, count * 8)?;
        Ok(bytes
            .chunks_exact(8)
            .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
            .collect())
    }
}
