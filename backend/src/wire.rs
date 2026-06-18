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
//!     i16  diffuseIndex (index into Texture[], -1 = none)
//!     i16  normalIndex  (-1 = none)
//!     u8   attrFlags (bit0 normals, bit1 uvs, bit2 glass/transparent)
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

use crate::handling::Handling;
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

/// Per-shader material: texture indices + a glass/transparent flag.
#[derive(Clone, Copy)]
pub struct Material {
    pub diffuse: i16,
    pub normal: i16,
    pub glass: bool,
}

impl Material {
    pub const NONE: Material = Material { diffuse: -1, normal: -1, glass: false };
}

fn write_geometry(w: &mut W, g: &Geometry, mat: Material) {
    let has_n = !g.normals.is_empty();
    let has_uv = !g.uvs.is_empty();
    let flags = (has_n as u8) | ((has_uv as u8) << 1) | ((mat.glass as u8) << 2);

    w.u16(g.shader_index);
    w.u16(mat.diffuse as u16); // -1 = none
    w.u16(mat.normal as u16); // -1 = none
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

/// Physical metadata derived from the .yft + handling.meta for the client's
/// raycast-suspension vehicle model.
pub struct Physics<'a> {
    pub wheel_radius: f32,    // metres
    pub body_min: [f32; 3],   // RAGE-space AABB of the body geometry
    pub body_max: [f32; 3],
    pub handling: Option<&'a Handling>,
}

fn write_handling(w: &mut W, h: &Handling) {
    let f = |w: &mut W, v: f32| w.f32(v);
    f(w, h.mass);
    f(w, h.drag_coeff);
    f(w, h.com_offset[0]);
    f(w, h.com_offset[1]);
    f(w, h.com_offset[2]);
    f(w, h.inertia_mult[0]);
    f(w, h.inertia_mult[1]);
    f(w, h.inertia_mult[2]);
    f(w, h.drive_bias_front);
    f(w, h.drive_gears);
    f(w, h.drive_force);
    f(w, h.drive_max_flat_vel);
    f(w, h.brake_force);
    f(w, h.brake_bias_front);
    f(w, h.handbrake_force);
    f(w, h.steering_lock);
    f(w, h.traction_curve_max);
    f(w, h.traction_curve_min);
    f(w, h.traction_curve_lateral);
    f(w, h.traction_bias_front);
    f(w, h.low_speed_traction_loss);
    f(w, h.suspension_force);
    f(w, h.suspension_comp_damp);
    f(w, h.suspension_rebound_damp);
    f(w, h.suspension_upper_limit);
    f(w, h.suspension_lower_limit);
    f(w, h.suspension_raise);
    f(w, h.suspension_bias_front);
    f(w, h.anti_roll_force);
    f(w, h.anti_roll_bias_front);
    f(w, h.seat_offset[0]);
    f(w, h.seat_offset[1]);
    f(w, h.seat_offset[2]);
}

/// `materials[shader_index]` = the material for that shader. `wheels` are the
/// (RAGE-space position, mirror) of each wheel bone to instance the "wheel" part.
pub fn serialize(
    parts: &[Part],
    texs: &[DecodedTexture],
    materials: &[Material],
    wheels: &[([f32; 3], bool)],
    phys: &Physics,
) -> Vec<u8> {
    let mut w = W { b: Vec::with_capacity(4 << 20) };
    w.u32(MAGIC);
    w.u8(8);
    w.u8(phys.handling.is_some() as u8);
    w.u16(0);
    w.u32(parts.len() as u32);
    w.u32(texs.len() as u32);
    w.u32(wheels.len() as u32);

    // Physical header: wheel radius + body AABB (RAGE space), then handling.
    w.f32(phys.wheel_radius);
    for v in phys.body_min {
        w.f32(v);
    }
    for v in phys.body_max {
        w.f32(v);
    }
    if let Some(h) = phys.handling {
        write_handling(&mut w, h);
    }

    let mat_for = |g: &Geometry| -> Material {
        materials.get(g.shader_index as usize).copied().unwrap_or(Material::NONE)
    };

    for p in parts {
        let name = p.name.as_bytes();
        w.u16(name.len() as u16);
        w.raw(name);
        let parent = p.parent.as_bytes();
        w.u16(parent.len() as u16);
        w.raw(parent);
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
            write_geometry(&mut w, g, mat_for(g));
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

    for (pos, mirror) in wheels {
        w.f32(pos[0]);
        w.f32(pos[1]);
        w.f32(pos[2]);
        w.u8(*mirror as u8);
        w.u8(0);
        w.u16(0);
    }

    w.b
}
