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

// Shader parameter name hashes (the param's MEANING, not its position).
const DIFFUSE_SAMPLER: u32 = 4059966321;
const DIFFUSE_TEX_SAMPLER: u32 = 3004704155;
const BUMP_SAMPLER: u32 = 1186448975;

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

#[derive(Default, Clone)]
pub struct ShaderInfo {
    pub diffuse: Option<String>, // 1st texture param
    pub normal: Option<String>,  // 2nd texture param (bump/normal)
}

/// Texture names per shader index. Vehicle shaders list textures in a stable
/// order: diffuse, then bump/normal, then spec/etc.
pub fn shader_infos(rsc: &Rsc7) -> Result<Vec<ShaderInfo>> {
    let sg = shader_group(rsc)?;
    if sg == 0 {
        return Ok(vec![]);
    }
    let shaders_ptr = rsc.u64_at(sg, SG_SHADERS_PTR)?;
    let count = rsc.u16_at(sg, SG_SHADERS_COUNT)? as usize;
    let shader_ptrs = rsc.ptr_array(shaders_ptr, count)?;

    let mut out = Vec::with_capacity(count);
    for &sh in &shader_ptrs {
        let mut info = ShaderInfo::default();
        if sh != 0 {
            let params_ptr = rsc.u64_at(sh, SHADER_PARAMS_PTR).unwrap_or(0);
            let pcount = rsc.u8_at(sh, SHADER_PARAM_COUNT).unwrap_or(0) as usize;
            if params_ptr != 0 && pcount > 0 {
                // Read params; vector params contribute 16*DataType bytes of inline
                // data after the param array, then the name hashes follow.
                let mut datatypes = Vec::with_capacity(pcount);
                let mut dataptrs = Vec::with_capacity(pcount);
                let mut vec_size = 0u64;
                for i in 0..pcount {
                    let p = params_ptr + i as u64 * PARAM_SIZE;
                    let dt = rsc.u8_at(p, PARAM_DATATYPE).unwrap_or(0);
                    let dptr = rsc.u64_at(p, PARAM_DATAPTR).unwrap_or(0);
                    if dt != 0 {
                        vec_size += 16 * dt as u64;
                    }
                    datatypes.push(dt);
                    dataptrs.push(dptr);
                }
                let hash_base = params_ptr + pcount as u64 * PARAM_SIZE + vec_size;
                for i in 0..pcount {
                    if datatypes[i] != 0 || dataptrs[i] == 0 {
                        continue; // only texture params
                    }
                    let hash = rsc.u32_at(hash_base, i * 4).unwrap_or(0);
                    let name = rsc.str_at(rsc.u64_at(dataptrs[i], TEX_NAME_PTR).unwrap_or(0));
                    if name.is_empty() {
                        continue;
                    }
                    if hash == DIFFUSE_SAMPLER || hash == DIFFUSE_TEX_SAMPLER {
                        info.diffuse = Some(name);
                    } else if hash == BUMP_SAMPLER {
                        info.normal = Some(name);
                    }
                }
            }
        }
        out.push(info);
    }
    Ok(out)
}
