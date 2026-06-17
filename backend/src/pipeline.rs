//! End-to-end: vehicle name -> wire bytes. Shared by the CLI and the server.

use crate::archive::{Archive, GtaKeys};
use crate::rsc7::Rsc7;
use crate::{textures, wire, yft};
use anyhow::{Context, Result};

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

    Ok(wire::serialize(&mesh, &texs))
}
