//! End-to-end: vehicle name -> wire bytes. Shared by the CLI and the server.

use crate::archive::{Archive, GtaKeys};
use crate::rsc7::Rsc7;
use crate::textures::DecodedTexture;
use crate::{parts, shaders, skeleton, textures, wire, yft};
use anyhow::{Context, Result};
use std::collections::HashMap;

fn decode_ytd(veh: &Archive, keys: &GtaKeys, name: &str) -> Vec<DecodedTexture> {
    veh.find_file(name)
        .and_then(|f| veh.extract(f, Some(keys)).ok())
        .and_then(|b| Rsc7::parse(&b).ok())
        .and_then(|r| textures::decode_dictionary(&r).ok())
        .unwrap_or_default()
}

/// Build a name → texture map from every source a vehicle draws from: the txd
/// embedded in the .yft, the vehicle's own .ytd (+HD), and shared vehshare.ytd.
/// Names are lowercased; earlier sources win (embedded/vehicle over shared).
pub fn texture_map(
    veh: &Archive,
    keys: &GtaKeys,
    name: &str,
    yft_rsc: &Rsc7,
) -> HashMap<String, DecodedTexture> {
    let mut map = HashMap::new();
    let mut add = |texs: Vec<DecodedTexture>| {
        for t in texs {
            map.entry(t.name.to_lowercase()).or_insert(t);
        }
    };

    let embedded = shaders::embedded_txd_ptr(yft_rsc);
    if let Ok(texs) = textures::decode_dictionary_at(yft_rsc, embedded) {
        add(texs);
    }

    let base = name.trim_end_matches("_hi");
    add(decode_ytd(veh, keys, &format!("{base}.ytd")));
    add(decode_ytd(veh, keys, &format!("{base}+hi.ytd")));
    add(decode_ytd(veh, keys, "vehshare.ytd"));
    add(decode_ytd(veh, keys, "vehshare_worn.ytd"));

    map
}

pub fn build_vehicle(
    veh: &Archive,
    keys: &GtaKeys,
    name: &str,
    include_textures: bool,
) -> Result<Vec<u8>> {
    // Mesh from the .yft fragment.
    let yft_name = format!("{name}.yft");
    let file = veh
        .find_file(&yft_name)
        .with_context(|| format!("{yft_name} not found"))?;
    let rsc = veh.extract(file, Some(keys))?;
    let r = Rsc7::parse(&rsc)?;
    let mesh = yft::decode(&r)?;
    let bones = skeleton::parse(&r)?;
    let world = skeleton::world_transforms(&bones);
    let grouped = parts::group(&mesh, &bones, &world);

    // Resolve each shader's diffuse texture → a deduped, downscaled texture list,
    // and a per-shader index into it. Off by default (RGBA is large).
    let mut textures: Vec<DecodedTexture> = Vec::new();
    let mut shader_tex_index: Vec<i16> = Vec::new();
    if include_textures {
        let names = shaders::diffuse_names(&r).unwrap_or_default();
        let map = texture_map(veh, keys, name, &r);
        let mut by_name: HashMap<String, i16> = HashMap::new();
        shader_tex_index = vec![-1i16; names.len()];
        for (si, n) in names.iter().enumerate() {
            let Some(tname) = n else { continue };
            let key = tname.to_lowercase();
            let Some(tex) = map.get(&key) else { continue };
            let idx = match by_name.get(&key) {
                Some(&i) => i,
                None => {
                    let i = textures.len() as i16;
                    textures.push(textures::downscale(tex, 128));
                    by_name.insert(key, i);
                    i
                }
            };
            shader_tex_index[si] = idx;
        }
    }

    Ok(wire::serialize(&grouped, &textures, &shader_tex_index))
}
