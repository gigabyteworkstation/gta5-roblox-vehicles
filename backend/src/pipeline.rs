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

    // Textures: base dict + HD dict (named off the base vehicle, not the _hi frag).
    // Off by default — they're large (uncompressed RGBA) and not bound to
    // geometries yet (needs shader→texture mapping).
    let mut texs = Vec::new();
    if include_textures {
        let base = name.trim_end_matches("_hi");
        for tn in [format!("{base}.ytd"), format!("{base}+hi.ytd")] {
            if let Some(tf) = veh.find_file(&tn) {
                if let Ok(tb) = veh.extract(tf, Some(keys)) {
                    if let Ok(tr) = Rsc7::parse(&tb) {
                        if let Ok(mut t) = textures::decode_dictionary(&tr) {
                            texs.append(&mut t);
                        }
                    }
                }
            }
        }
    }

    Ok(wire::serialize(&grouped, &texs))
}
