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
/// embedded in the .yft, shared vehshare.ytd, and — derived from the texture
/// names the shaders need — each name prefix's `<prefix>.ytd` / `+hi` /
/// `vehicles_<prefix>_interior.ytd`. So `sultan_dash_hd` pulls in `sultan.ytd`
/// etc. Names lowercased; earlier sources win.
pub fn texture_map(
    veh: &Archive,
    keys: &GtaKeys,
    name: &str,
    yft_rsc: &Rsc7,
    needed: &[String],
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

    // Texture-name prefixes → their dictionaries (interior, shared model, etc.).
    let mut prefixes: std::collections::HashSet<String> = std::collections::HashSet::new();
    for n in needed {
        if let Some(p) = n.to_lowercase().split('_').next() {
            prefixes.insert(p.to_string());
        }
    }
    for p in prefixes {
        add(decode_ytd(veh, keys, &format!("{p}.ytd")));
        add(decode_ytd(veh, keys, &format!("{p}+hi.ytd")));
        add(decode_ytd(veh, keys, &format!("vehicles_{p}_interior.ytd")));
    }

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

    // Resolve each shader's diffuse + normal textures → a deduped, full-res
    // texture list and a per-shader Material. Off by default (RGBA is large).
    let mut textures: Vec<DecodedTexture> = Vec::new();
    let mut materials: Vec<wire::Material> = Vec::new();
    if include_textures {
        let infos = shaders::shader_infos(&r).unwrap_or_default();
        let needed: Vec<String> = infos
            .iter()
            .flat_map(|i| [i.diffuse.clone(), i.normal.clone()])
            .flatten()
            .collect();
        let map = texture_map(veh, keys, name, &r, &needed);
        let mut by_name: HashMap<String, i16> = HashMap::new();
        {
            let mut resolve = |opt: &Option<String>| -> i16 {
                let Some(tname) = opt else { return -1 };
                let key = tname.to_lowercase();
                let Some(tex) = map.get(&key) else { return -1 };
                if let Some(&i) = by_name.get(&key) {
                    return i;
                }
                let i = textures.len() as i16;
                textures.push(textures::downscale(tex, 8192)); // no-op (full res)
                by_name.insert(key, i);
                i
            };
            for info in &infos {
                let diffuse = resolve(&info.diffuse);
                let normal = resolve(&info.normal);
                let glass = info
                    .diffuse
                    .as_ref()
                    .map(|n| n.to_lowercase().contains("glass"))
                    .unwrap_or(false);
                materials.push(wire::Material { diffuse, normal, glass });
            }
        }
    }

    Ok(wire::serialize(&grouped, &textures, &materials))
}
