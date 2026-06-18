//! Parse the drawable's ShaderGroup to learn which texture each shader uses.
//!
//! FragType.Drawable -> DrawableBase.ShaderGroupPointer -> ShaderGroup. Each
//! geometry's shader_index selects a ShaderFX; the shader's first texture
//! parameter points to a grcTexture whose name is the diffuse texture.

use crate::rsc7::Rsc7;
use anyhow::Result;

const ROOT: u64 = 0x5000_0000;
const FRAGTYPE_DRAWABLE_PTR: usize = 0x30;
const DB_SHADERGROUP_PTR: usize = 0x10;

const SG_SHADERS_PTR: usize = 0x10;
const SG_SHADERS_COUNT: usize = 0x18; // u16
const SG_TXD_PTR: usize = 0x08; // embedded TextureDictionary

const SHADER_PARAMS_PTR: usize = 0x00;
const SHADER_PARAM_COUNT: usize = 0x10; // u8

const PARAM_SIZE: u64 = 16;
const PARAM_DATATYPE: usize = 0x00; // u8: 0 = texture
const PARAM_DATAPTR: usize = 0x08; // u64

const TEX_NAME_PTR: usize = 0x28; // grcTexture name pointer

fn shader_group(rsc: &Rsc7) -> Result<u64> {
    let drawable = rsc.u64_at(ROOT, FRAGTYPE_DRAWABLE_PTR)?;
    Ok(rsc.u64_at(drawable, DB_SHADERGROUP_PTR)?)
}

/// Pointer to the drawable's embedded TextureDictionary (0 if none).
pub fn embedded_txd_ptr(rsc: &Rsc7) -> u64 {
    shader_group(rsc)
        .and_then(|sg| if sg == 0 { Ok(0) } else { rsc.u64_at(sg, SG_TXD_PTR) })
        .unwrap_or(0)
}

/// Diffuse texture name per shader index (None if the shader has no texture
/// parameter, e.g. runtime-coloured paint).
pub fn diffuse_names(rsc: &Rsc7) -> Result<Vec<Option<String>>> {
    let sg = shader_group(rsc)?;
    if sg == 0 {
        return Ok(vec![]);
    }
    let shaders_ptr = rsc.u64_at(sg, SG_SHADERS_PTR)?;
    let count = rsc.u16_at(sg, SG_SHADERS_COUNT)? as usize;
    let shader_ptrs = rsc.ptr_array(shaders_ptr, count)?;

    let mut out = Vec::with_capacity(count);
    for &sh in &shader_ptrs {
        if sh == 0 {
            out.push(None);
            continue;
        }
        let params_ptr = rsc.u64_at(sh, SHADER_PARAMS_PTR)?;
        let pcount = rsc.u8_at(sh, SHADER_PARAM_COUNT)? as usize;

        let mut name = None;
        for i in 0..pcount {
            let p = params_ptr + i as u64 * PARAM_SIZE;
            if rsc.u8_at(p, PARAM_DATATYPE).unwrap_or(1) != 0 {
                continue; // not a texture
            }
            let tex = rsc.u64_at(p, PARAM_DATAPTR).unwrap_or(0);
            if tex == 0 {
                continue;
            }
            let nptr = rsc.u64_at(tex, TEX_NAME_PTR).unwrap_or(0);
            let n = rsc.str_at(nptr);
            if !n.is_empty() {
                name = Some(n);
                break; // first texture param = diffuse
            }
        }
        out.push(name);
    }
    Ok(out)
}
