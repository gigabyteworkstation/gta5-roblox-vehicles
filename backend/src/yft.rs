//! .yft (fragType) → drawable → geometry decoder.
//!
//! Layouts taken from CodeWalker (gen8 / GTA5 PC), cross-checked against the
//! RAGE source. Walks: FragType -> FragDrawable(DrawableBase) -> High LOD models
//! -> geometries -> {VertexBuffer, IndexBuffer, VertexDeclaration}, decoding the
//! packed FVF into plain position/normal/uv/colour arrays.

use crate::rsc7::Rsc7;
use anyhow::{bail, Context, Result};

// ---- struct field offsets (gen8) ------------------------------------------
const FRAGTYPE_DRAWABLE_PTR: usize = 0x30;
const FRAGTYPE_PHYSLODGROUP_PTR: usize = 0xF0;

// FragPhysicsLODGroup
const LODGROUP_LOD1_PTR: usize = 0x10;
// FragPhysicsLOD
const LOD_CHILDREN_PTR: usize = 0xD0;
const LOD_TRANSFORMS_PTR: usize = 0x100;
const LOD_CHILDREN_COUNT: usize = 0x11D; // u8
// FragPhysTypeChild
const CHILD_BONETAG: usize = 0x0A; // u16
const CHILD_DRAWABLE1_PTR: usize = 0xA0;
// FragPhysTransforms: 16 floats (4x4) per child, inline after a 0x20 header
const TRANSFORMS_MATRICES: usize = 0x20;

const DB_MODELS_HIGH_PTR: usize = 0x50;

// ResourcePointerListHeader { Pointer u64 @0x00, Count u16 @0x08, Capacity u16 @0x0A }
const PLH_POINTER: usize = 0x00;
const PLH_CAPACITY: usize = 0x0A;

const MODEL_GEOMETRIES_PTR: usize = 0x08;
const MODEL_GEOM_COUNT: usize = 0x10;
const MODEL_SHADERMAP_PTR: usize = 0x20;

const GEO_VB_PTR: usize = 0x18;
const GEO_IB_PTR: usize = 0x38;
const GEO_INDICES_COUNT: usize = 0x58;
const GEO_VERTS_COUNT: usize = 0x60;
const GEO_BONEIDS_PTR: usize = 0x68;
const GEO_STRIDE: usize = 0x70;
const GEO_BONEIDS_COUNT: usize = 0x72; // u16

const VB_STRIDE: usize = 0x08;
const VB_DATA1_PTR: usize = 0x10;
const VB_VERTEX_COUNT: usize = 0x18;
const VB_INFO_PTR: usize = 0x30;

const DECL_FLAGS: usize = 0x00;
const DECL_TYPES: usize = 0x08;

const IB_INDICES_COUNT: usize = 0x08;
const IB_INDICES_PTR: usize = 0x10;

// ---- decoded output --------------------------------------------------------
#[derive(Default)]
pub struct Geometry {
    pub shader_index: u16,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[u8; 4]>,
    pub indices: Vec<u32>,
    pub stride: u16,
    pub fvf_flags: u32,
    pub fvf_types: u64,
    /// which part this came from: "body", or a child like "child3"
    pub part: String,
    /// per-vertex skinning (present when the FVF has BlendWeights+BlendIndices).
    /// bone_idx are SKELETON bone indices (after the geometry's BoneIds remap).
    pub skinned: bool,
    pub bone_idx: Vec<[u16; 4]>,
    pub bone_wt: Vec<[u8; 4]>,
}

pub struct Mesh {
    pub geometries: Vec<Geometry>,
    /// (child index, bone tag, geometry count) for each physics child decoded
    pub children: Vec<(usize, u16, usize)>,
}

/// A 4x4 row-major matrix (SharpDX layout): translation in floats 12,13,14.
type Mat4 = [f32; 16];

fn read_matrix(rsc: &Rsc7, transforms_ptr: u64, i: usize) -> Result<Mat4> {
    let off = TRANSFORMS_MATRICES + i * 64;
    let b = rsc.at(transforms_ptr + off as u64, 64)?;
    let mut m = [0f32; 16];
    for (j, c) in b.chunks_exact(4).enumerate() {
        m[j] = f32::from_le_bytes(c.try_into().unwrap());
    }
    Ok(m)
}

fn xform_point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    [
        p[0] * m[0] + p[1] * m[4] + p[2] * m[8] + m[12],
        p[0] * m[1] + p[1] * m[5] + p[2] * m[9] + m[13],
        p[0] * m[2] + p[1] * m[6] + p[2] * m[10] + m[14],
    ]
}

fn xform_dir(m: &Mat4, n: [f32; 3]) -> [f32; 3] {
    [
        n[0] * m[0] + n[1] * m[4] + n[2] * m[8],
        n[0] * m[1] + n[1] * m[5] + n[2] * m[9],
        n[0] * m[2] + n[1] * m[6] + n[2] * m[10],
    ]
}

/// gen8 vertex component sizes by 4-bit type code.
fn component_size(code: u8) -> usize {
    match code {
        0 => 0,        // Nothing
        1 => 4,        // Half2
        2 => 4,        // Float
        3 => 8,        // Half4
        4 => 0,        // FloatUnk
        5 => 8,        // Float2
        6 => 12,       // Float3
        7 => 16,       // Float4
        8 => 4,        // UByte4
        9 => 4,        // Colour
        10 => 4,       // RGBA8SNorm
        _ => 4,        // Unk1..5 (assume 4)
    }
}

fn half_to_f32(h: u16) -> f32 {
    let sign = (h >> 15) & 1;
    let exp = (h >> 10) & 0x1F;
    let mant = h & 0x3FF;
    let val = if exp == 0 {
        // subnormal
        (mant as f32) * 2f32.powi(-24)
    } else if exp == 0x1F {
        if mant == 0 { f32::INFINITY } else { f32::NAN }
    } else {
        (1.0 + (mant as f32) / 1024.0) * 2f32.powi(exp as i32 - 15)
    };
    if sign == 1 { -val } else { val }
}

/// Decode the byte offset within a vertex for each present channel (bit index).
fn channel_offsets(flags: u32, types: u64) -> [Option<usize>; 16] {
    let mut offs = [None; 16];
    let mut cursor = 0usize;
    for k in 0..16 {
        if (flags >> k) & 1 == 1 {
            offs[k] = Some(cursor);
            let code = ((types >> (k * 4)) & 0xF) as u8;
            cursor += component_size(code);
        }
    }
    offs
}

fn comp_code(types: u64, k: usize) -> u8 {
    ((types >> (k * 4)) & 0xF) as u8
}

pub fn decode(rsc: &Rsc7) -> Result<Mesh> {
    // Root FragType lives at the start of the system segment (system pointer 0x50000000).
    const ROOT: u64 = 0x5000_0000;

    let mut geometries = Vec::new();
    let mut children = Vec::new();

    // 1) Common drawable = the welded body (no wheels).
    let drawable = rsc
        .u64_at(ROOT, FRAGTYPE_DRAWABLE_PTR)
        .context("reading FragType.DrawablePointer")?;
    if drawable != 0 {
        decode_drawable(rsc, drawable, None, "body", &mut geometries)?;
    }

    // 2) Physics children = wheels, doors, bumpers... each with its own drawable
    //    and a transform placing it in vehicle space.
    let lodgroup = rsc.u64_at(ROOT, FRAGTYPE_PHYSLODGROUP_PTR).unwrap_or(0);
    if lodgroup != 0 {
        let lod1 = rsc.u64_at(lodgroup, LODGROUP_LOD1_PTR).unwrap_or(0);
        if lod1 != 0 {
            let children_ptr = rsc.u64_at(lod1, LOD_CHILDREN_PTR).unwrap_or(0);
            let children_count = rsc.u8_at(lod1, LOD_CHILDREN_COUNT).unwrap_or(0) as usize;
            let transforms_ptr = rsc.u64_at(lod1, LOD_TRANSFORMS_PTR).unwrap_or(0);

            if children_ptr != 0 && children_count > 0 {
                let child_ptrs = rsc.ptr_array(children_ptr, children_count)?;
                for (i, &child) in child_ptrs.iter().enumerate() {
                    if child == 0 {
                        continue;
                    }
                    let d1 = rsc.u64_at(child, CHILD_DRAWABLE1_PTR).unwrap_or(0);
                    if d1 == 0 {
                        continue;
                    }
                    let bone_tag = rsc.u16_at(child, CHILD_BONETAG).unwrap_or(0);
                    let mat = if transforms_ptr != 0 {
                        read_matrix(rsc, transforms_ptr, i).ok()
                    } else {
                        None
                    };
                    let before = geometries.len();
                    decode_drawable(rsc, d1, mat.as_ref(), &format!("child{i}"), &mut geometries)?;
                    let added = geometries.len() - before;
                    if added > 0 {
                        children.push((i, bone_tag, added));
                    }
                }
            }
        }
    }

    if geometries.is_empty() {
        bail!("no geometries decoded");
    }
    Ok(Mesh { geometries, children })
}

/// Decode all High-LOD geometries of a DrawableBase, optionally transformed.
fn decode_drawable(
    rsc: &Rsc7,
    drawable: u64,
    transform: Option<&Mat4>,
    part: &str,
    out: &mut Vec<Geometry>,
) -> Result<()> {
    let high_ptr = rsc.u64_at(drawable, DB_MODELS_HIGH_PTR).unwrap_or(0);
    if high_ptr == 0 {
        return Ok(()); // some children carry no visual high-LOD models
    }
    let models_arr = rsc.u64_at(high_ptr, PLH_POINTER)?;
    let models_cap = rsc.u16_at(high_ptr, PLH_CAPACITY)? as usize;
    let model_ptrs = rsc.ptr_array(models_arr, models_cap)?;

    for &model in &model_ptrs {
        if model == 0 {
            continue;
        }
        let geo_arr = rsc.u64_at(model, MODEL_GEOMETRIES_PTR)?;
        let geo_count = rsc.u16_at(model, MODEL_GEOM_COUNT)? as usize;
        let shadermap_ptr = rsc.u64_at(model, MODEL_SHADERMAP_PTR)?;
        let geo_ptrs = rsc.ptr_array(geo_arr, geo_count)?;

        for (gi, &geo) in geo_ptrs.iter().enumerate() {
            if geo == 0 {
                continue;
            }
            let shader_index = if shadermap_ptr != 0 {
                rsc.u16_at(shadermap_ptr, gi * 2).unwrap_or(0)
            } else {
                0
            };
            let mut g = decode_geometry(rsc, geo, shader_index)?;
            g.part = part.to_string();
            if let Some(m) = transform {
                for p in &mut g.positions {
                    *p = xform_point(m, *p);
                }
                for n in &mut g.normals {
                    *n = xform_dir(m, *n);
                }
            }
            out.push(g);
        }
    }
    Ok(())
}

fn decode_geometry(rsc: &Rsc7, geo: u64, shader_index: u16) -> Result<Geometry> {
    let vb = rsc.u64_at(geo, GEO_VB_PTR)?;
    let ib = rsc.u64_at(geo, GEO_IB_PTR)?;
    let stride = rsc.u16_at(geo, GEO_STRIDE)?;
    let _verts_count_geo = rsc.u16_at(geo, GEO_VERTS_COUNT)?;
    let _indices_count_geo = rsc.u32_at(geo, GEO_INDICES_COUNT)?;

    if vb == 0 || ib == 0 {
        bail!("geometry missing vertex/index buffer");
    }

    // Vertex buffer
    let vb_stride = rsc.u16_at(vb, VB_STRIDE)?;
    let vertex_count = rsc.u32_at(vb, VB_VERTEX_COUNT)? as usize;
    let data_ptr = rsc.u64_at(vb, VB_DATA1_PTR)?;
    let info_ptr = rsc.u64_at(vb, VB_INFO_PTR)?;

    let stride = if vb_stride != 0 { vb_stride } else { stride } as usize;
    if stride == 0 {
        bail!("zero vertex stride");
    }

    // Vertex declaration (FVF)
    let fvf_flags = rsc.u32_at(info_ptr, DECL_FLAGS)?;
    let fvf_types = rsc.u64_at(info_ptr, DECL_TYPES)?;
    let offs = channel_offsets(fvf_flags, fvf_types);

    // BoneIds palette: per-vertex BlendIndices index into this list of skeleton
    // bone indices. If absent, BlendIndices are direct skeleton bone indices.
    let boneids_ptr = rsc.u64_at(geo, GEO_BONEIDS_PTR)?;
    let boneids_count = rsc.u16_at(geo, GEO_BONEIDS_COUNT)? as usize;
    let palette: Vec<u16> = if boneids_ptr != 0 && boneids_count > 0 {
        rsc.at(boneids_ptr, boneids_count * 2)
            .map(|b| b.chunks_exact(2).map(|c| u16::from_le_bytes(c.try_into().unwrap())).collect())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    // Skinned when both BlendWeights (ch1) and BlendIndices (ch2) are present.
    let skinned = offs[1].is_some() && offs[2].is_some();

    // Bounds check the whole vertex buffer up front.
    let vbytes = rsc
        .at(data_ptr, vertex_count * stride)
        .context("vertex data out of bounds")?;

    let mut g = Geometry {
        shader_index,
        stride: stride as u16,
        fvf_flags,
        fvf_types,
        skinned,
        ..Default::default()
    };

    let read_f32 = |b: &[u8], o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let read_half = |b: &[u8], o: usize| half_to_f32(u16::from_le_bytes(b[o..o + 2].try_into().unwrap()));

    for i in 0..vertex_count {
        let base = i * stride;
        let v = &vbytes[base..base + stride];

        // Position (channel 0) — required, virtually always Float3.
        if let Some(o) = offs[0] {
            g.positions.push([read_f32(v, o), read_f32(v, o + 4), read_f32(v, o + 8)]);
        }
        // Normal (channel 3)
        if let Some(o) = offs[3] {
            let code = comp_code(fvf_types, 3);
            if code == 6 {
                g.normals.push([read_f32(v, o), read_f32(v, o + 4), read_f32(v, o + 8)]);
            }
        }
        // Colour0 (channel 4) — Colour (4xu8)
        if let Some(o) = offs[4] {
            g.colors.push([v[o], v[o + 1], v[o + 2], v[o + 3]]);
        }
        // TexCoord0 (channel 6) — Half2 (common) or Float2
        if let Some(o) = offs[6] {
            let code = comp_code(fvf_types, 6);
            let uv = match code {
                1 => [read_half(v, o), read_half(v, o + 2)], // Half2
                5 => [read_f32(v, o), read_f32(v, o + 4)],   // Float2
                _ => [0.0, 0.0],
            };
            g.uvs.push(uv);
        }
        // Skinning: BlendWeights (ch1, UByte4) + BlendIndices (ch2, UByte4).
        if skinned {
            let wo = offs[1].unwrap();
            let io = offs[2].unwrap();
            let mut idx = [0u16; 4];
            for k in 0..4 {
                let bi = v[io + k] as usize;
                idx[k] = if palette.is_empty() {
                    bi as u16
                } else {
                    *palette.get(bi).unwrap_or(&0)
                };
            }
            g.bone_idx.push(idx);
            g.bone_wt.push([v[wo], v[wo + 1], v[wo + 2], v[wo + 3]]);
        }
    }

    // Index buffer — u16 indices.
    let idx_count = rsc.u32_at(ib, IB_INDICES_COUNT)? as usize;
    let idx_ptr = rsc.u64_at(ib, IB_INDICES_PTR)?;
    let ibytes = rsc.at(idx_ptr, idx_count * 2).context("index data out of bounds")?;
    g.indices = ibytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes(c.try_into().unwrap()) as u32)
        .collect();

    Ok(g)
}

/// Write all geometries as a single OBJ (positions, uvs, faces) for eyeballing.
pub fn write_obj(mesh: &Mesh, path: &std::path::Path) -> Result<()> {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "# decoded from GTA5 .yft");
    let mut vbase = 1usize; // OBJ is 1-indexed
    for (gi, g) in mesh.geometries.iter().enumerate() {
        let _ = writeln!(s, "o {}_geom{gi}_shader{}", g.part, g.shader_index);
        for p in &g.positions {
            let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
        }
        for uv in &g.uvs {
            // OBJ V is flipped relative to typical engine UVs.
            let _ = writeln!(s, "vt {} {}", uv[0], 1.0 - uv[1]);
        }
        let has_uv = !g.uvs.is_empty();
        for tri in g.indices.chunks_exact(3) {
            let (a, b, c) = (vbase + tri[0] as usize, vbase + tri[1] as usize, vbase + tri[2] as usize);
            if has_uv {
                let _ = writeln!(s, "f {a}/{a} {b}/{b} {c}/{c}");
            } else {
                let _ = writeln!(s, "f {a} {b} {c}");
            }
        }
        vbase += g.positions.len();
    }
    std::fs::write(path, s)?;
    Ok(())
}
