//! Compact binary wire format ("GVEH") sent to the Roblox client.
//!
//! All little-endian. Geometry attributes are de-interleaved (plain arrays) so
//! the LuaU side can feed them straight into EditableMesh without re-parsing a
//! packed vertex stride.
//!
//! Header:
//!   u32  magic 'GVEH'
//!   u8   version (=1)
//!   u8   reserved
//!   u16  reserved
//!   u32  geometryCount
//!   u32  textureCount
//! Geometry[]:
//!   u16  shaderIndex
//!   u8   attrFlags (bit0 normals, bit1 uvs)
//!   u8   reserved
//!   u32  vertexCount   (always < 65536 per geometry)
//!   u32  indexCount
//!   f32  positions[vertexCount*3]
//!   f32  normals[vertexCount*3]      (if bit0)
//!   f32  uvs[vertexCount*2]          (if bit1)
//!   u16  indices[indexCount]
//! Texture[]:
//!   u16  nameLen, name bytes (utf8)
//!   u16  width, u16 height
//!   u8   rgba[width*height*4]

use crate::textures::DecodedTexture;
use crate::yft::Mesh;

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

pub fn serialize(mesh: &Mesh, texs: &[DecodedTexture]) -> Vec<u8> {
    let mut w = W { b: Vec::with_capacity(4 << 20) };
    w.u32(MAGIC);
    w.u8(1);
    w.u8(0);
    w.u16(0);
    w.u32(mesh.geometries.len() as u32);
    w.u32(texs.len() as u32);

    for g in &mesh.geometries {
        let has_n = !g.normals.is_empty();
        let has_uv = !g.uvs.is_empty();
        let flags = (has_n as u8) | ((has_uv as u8) << 1);

        w.u16(g.shader_index);
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
        // Indices fit in u16: each geometry has < 65536 vertices.
        for &i in &g.indices {
            w.u16(i as u16);
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
