//! Compact binary wire format ("GVEH") sent to the Roblox client.
//!
//! All little-endian. Geometry attributes are de-interleaved (plain arrays) so
//! the LuaU side can feed them straight into EditableMesh without re-parsing a
//! packed vertex stride.
//!
//! Parts-based format (v3). The vehicle is a list of rigid parts: a "body" plus
//! articulated parts (doors/hood/boot) each carrying a hinge. All little-endian;
//! geometry attributes de-interleaved. RAGE space — client applies the axis swap.
//!
//! Header:
//!   u32  magic 'GVEH'
//!   u8   version (=3)
//!   u8   reserved
//!   u16  reserved
//!   u32  partCount
//!   u32  textureCount
//! Part[]:
//!   u16  nameLen, name bytes (utf8)
//!   u8   articulated (0/1)
//!   u8   reserved
//!   if articulated:
//!     f32 hingePos[3], f32 hingeAxis[3], f32 minAngle, f32 maxAngle  (RAGE space, radians)
//!   u32  geometryCount
//!   Geometry[]:
//!     u16  shaderIndex
//!     i16  textureIndex (index into Texture[], -1 = none)
//!     u8   attrFlags (bit0 normals, bit1 uvs)
//!     u8   reserved
//!     u32  vertexCount   (< 65536)
//!     u32  indexCount
//!     f32  positions[vertexCount*3]
//!     f32  normals[vertexCount*3]   (if bit0)
//!     f32  uvs[vertexCount*2]       (if bit1)
//!     u16  indices[indexCount]
//! Texture[]:
//!   u16  nameLen, name bytes (utf8)
//!   u16  width, u16 height
//!   u8   rgba[width*height*4]

use crate::parts::Part;
use crate::textures::DecodedTexture;
use crate::yft::Geometry;

const MAGIC: u32 = u32::from_le_bytes(*b"GVEH");

struct W {
    b: Vec<u8>,
}
impl W {
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn raw(&mut self, s: &[u8]) {
        self.b.extend_from_slice(s);
    }
}

fn write_geometry(w: &mut W, g: &Geometry, tex_index: i16) {
    let has_n = !g.normals.is_empty();
    let has_uv = !g.uvs.is_empty();
    let flags = (has_n as u8) | ((has_uv as u8) << 1);

    w.u16(g.shader_index);
    w.u16(tex_index as u16); // -1 = no texture
    w.u8(flags);
    w.u8(0);
    w.u32(g.positions.len() as u32);
    w.u32(g.indices.len() as u32);

    for p in &g.positions {
        w.f32(p[0]);
        w.f32(p[1]);
        w.f32(p[2]);
    }
    if has_n {
        for n in &g.normals {
            w.f32(n[0]);
            w.f32(n[1]);
            w.f32(n[2]);
        }
    }
    if has_uv {
        for uv in &g.uvs {
            w.f32(uv[0]);
            w.f32(uv[1]);
        }
    }
    for &i in &g.indices {
        w.u16(i as u16);
    }
}

/// `shader_tex_index[shader_index]` = index into `texs` for that shader's
/// diffuse texture, or -1 for none.
pub fn serialize(parts: &[Part], texs: &[DecodedTexture], shader_tex_index: &[i16]) -> Vec<u8> {
    let mut w = W { b: Vec::with_capacity(4 << 20) };
    w.u32(MAGIC);
    w.u8(4);
    w.u8(0);
    w.u16(0);
    w.u32(parts.len() as u32);
    w.u32(texs.len() as u32);

    let tex_for = |g: &Geometry| -> i16 {
        shader_tex_index.get(g.shader_index as usize).copied().unwrap_or(-1)
    };

    for p in parts {
        let name = p.name.as_bytes();
        w.u16(name.len() as u16);
        w.raw(name);
        w.u8(p.articulated as u8);
        w.u8(0);
        if let Some(h) = &p.hinge {
            w.f32(h.pos[0]);
            w.f32(h.pos[1]);
            w.f32(h.pos[2]);
            w.f32(h.axis[0]);
            w.f32(h.axis[1]);
            w.f32(h.axis[2]);
            w.f32(h.min_angle);
            w.f32(h.max_angle);
        }
        w.u32(p.geometries.len() as u32);
        for g in &p.geometries {
            write_geometry(&mut w, g, tex_for(g));
        }
    }

    for t in texs {
        let name = t.name.as_bytes();
        w.u16(name.len() as u16);
        w.raw(name);
        w.u16(t.width as u16);
        w.u16(t.height as u16);
        w.raw(&t.rgba);
    }

    w.b
}
