//! Milestone 1: prove RPF layers 1-2 against the real archive.
//! List vehicle assets and extract one vehicle's raw resource bytes.

mod archive;
mod handling;
mod joints;
mod parts;
mod pipeline;
mod rsc7;
mod server;
mod shaders;
mod skeleton;
mod textures;
mod wire;
mod yft;

use anyhow::{Context, Result};
use archive::{default_keys_dir, Archive, GtaKeys, RpfEntryKind};
use rsc7::{version_from_flags, Rsc7};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gtaveh", about = "GTA5 vehicle backend (milestone 1: list/extract)")]
struct Cli {
    /// Directory containing gtav_aes_key.dat / gtav_ng_key.dat / gtav_ng_decrypt_tables.dat
    #[arg(long, global = true)]
    keys: Option<PathBuf>,

    /// Outer archive (default: x64e.rpf in the keys dir)
    #[arg(long, global = true)]
    archive: Option<PathBuf>,

    /// Nested archive to descend into; pass "" to treat --archive as the vehicle archive directly
    #[arg(long, global = true, default_value = "vehicles.rpf")]
    nested: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List files in the (nested) vehicle archive, optionally filtered by substring
    List { filter: Option<String> },

    /// Extract one vehicle's raw resource bytes to ./out
    ReadVehicle {
        /// Vehicle/asset base name, e.g. "zion_hi"
        name: String,
        /// Output directory
        #[arg(long, default_value = "out")]
        out: PathBuf,
    },

    /// Extract one raw file from the archive (e.g. "handling.meta") to ./out
    Extract {
        /// File name inside the archive
        file: String,
        #[arg(long, default_value = "out")]
        out: PathBuf,
    },

    /// Parse the RSC7 paging of a vehicle's .yft and verify the segment math
    Inspect {
        /// Vehicle/asset base name, e.g. "zion_hi"
        name: String,
    },

    /// Decode a vehicle's .yft mesh and write an OBJ for inspection
    Mesh {
        /// Vehicle/asset base name, e.g. "zion_hi"
        name: String,
        #[arg(long, default_value = "out")]
        out: PathBuf,
    },

    /// Decode a .ytd texture dictionary to PNGs
    Textures {
        /// The .ytd filename inside the archive, e.g. "zion+hi.ytd" or "zion.ytd"
        ytd: String,
        #[arg(long, default_value = "out")]
        out: PathBuf,
    },

    /// Build a vehicle's GVEH wire blob to a file (for size/offline checks)
    Build {
        name: String,
        #[arg(long, default_value = "out")]
        out: PathBuf,
    },

    /// Parse and print a vehicle's skeleton (bones) for verification
    Skeleton {
        name: String,
    },

    /// Group a vehicle's mesh into rigid parts by bone (articulation preview)
    Parts {
        name: String,
    },

    /// Print each geometry's shader → diffuse texture name
    Shaders {
        name: String,
    },

    /// Parse and print the fragment physics joints (articulation)
    Joints {
        name: String,
    },

    /// Run the HTTP server for the Roblox client
    Serve {
        /// Listen address (0.0.0.0 lets other LAN machines reach it)
        #[arg(long, default_value = "0.0.0.0:5000")]
        addr: String,
    },
}

fn open_vehicle_archive(cli: &Cli, keys: &GtaKeys) -> Result<Archive> {
    let keys_dir = cli.keys.clone().unwrap_or_else(default_keys_dir);
    let outer_path = cli
        .archive
        .clone()
        .unwrap_or_else(|| keys_dir.join("x64e.rpf"));

    println!("Opening outer archive: {}", outer_path.display());
    let outer = Archive::open(&outer_path, Some(keys))?;
    println!(
        "  encryption={:?}  entries={}  dirs={}",
        outer.encryption, outer.entry_count, outer.dir_count
    );

    if cli.nested.is_empty() {
        return Ok(outer);
    }

    println!("Descending into nested archive: {}", cli.nested);
    let veh = outer.open_nested(&cli.nested, Some(keys))?;
    println!(
        "  '{}' encryption={:?}  entries={}  dirs={}",
        veh.name, veh.encryption, veh.entry_count, veh.dir_count
    );
    Ok(veh)
}

fn main() -> Result<()> {
    // Windows' default 1 MB main-thread stack overflows when GtaKeys builds the
    // ~278 KB NG decrypt table by value. Run everything on a roomy stack.
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(run)?
        .join()
        .expect("worker thread panicked")
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let keys_dir = cli.keys.clone().unwrap_or_else(default_keys_dir);

    println!("Loading keys from: {}", keys_dir.display());
    let keys = GtaKeys::load_from_path(&keys_dir)
        .with_context(|| format!("loading GTA5 keys from {}", keys_dir.display()))?;

    let veh = open_vehicle_archive(&cli, &keys)?;

    match &cli.command {
        Commands::List { filter } => {
            let mut files = veh.list_files();
            files.sort_by(|a, b| a.name.cmp(&b.name));
            let mut shown = 0usize;
            for f in &files {
                if let Some(sub) = filter {
                    if !f.name.to_lowercase().contains(&sub.to_lowercase()) {
                        continue;
                    }
                }
                println!("  {}", f.name);
                shown += 1;
            }
            println!("({shown} files shown of {} total)", files.len());
        }

        Commands::ReadVehicle { name, out } => {
            std::fs::create_dir_all(out)?;

            // Show every asset whose name relates to this vehicle, so we can see
            // the real base/_hi/+hi/+hifr layout from the manifest in practice.
            let base = name.trim_end_matches("_hi");
            println!("\nAssets related to '{base}':");
            for f in veh.list_files() {
                if f.name.to_lowercase().starts_with(&base.to_lowercase()) {
                    println!("  {}", f.name);
                }
            }

            // Extract the requested fragment.
            let yft = format!("{name}.yft");
            extract_one(&veh, &keys, &yft, out)?;

            // Try the matching HD texture dictionaries (best-effort).
            for cand in [format!("{name}+hifr.ytd"), format!("{name}+hi.ytd")] {
                if veh.find_file(&cand).is_some() {
                    extract_one(&veh, &keys, &cand, out)?;
                }
            }
        }

        Commands::Extract { file, out } => {
            std::fs::create_dir_all(out)?;
            extract_one(&veh, &keys, file, out)?;
        }

        Commands::Inspect { name } => {
            let yft = format!("{name}.yft");
            let file = veh
                .find_file(&yft)
                .with_context(|| format!("{yft} not found"))?;
            let rsc = veh.extract(file, Some(&keys))?;

            let r = Rsc7::parse(&rsc)?;
            let version = version_from_flags(r.system_flags, r.graphics_flags);
            let sys_size = rsc7::size_from_flags(r.system_flags);
            let gfx_size = rsc7::size_from_flags(r.graphics_flags);
            println!("\n{yft}: {} bytes (with RSC7 header)", rsc.len());
            println!("  resource version : {version}  (expect 162 for fragType/.yft)");
            println!(
                "  compressed body  : {} bytes  ({:.2}x ratio)",
                r.compressed_len,
                (sys_size + gfx_size) as f64 / r.compressed_len.max(1) as f64
            );
            println!("  system  segment  : {sys_size} bytes  (decoded {})", r.system.len());
            println!("  graphics segment : {gfx_size} bytes  (decoded {})", r.graphics.len());
            if r.system.len() == sys_size && r.graphics.len() == gfx_size {
                println!("  paging check     : OK (segments match flag-decoded sizes)");
            } else {
                println!("  paging check     : segments differ from flag sizes");
            }

            print!("  system[0..64]    :");
            for (i, b) in r.system.iter().take(64).enumerate() {
                if i % 16 == 0 {
                    print!("\n    ");
                }
                print!("{b:02X} ");
            }
            println!();
            // The first 8 bytes of the root object are its vtable pointer (a
            // tagged system pointer). Show how it classifies.
            let vptr = u64::from_le_bytes(r.system[0..8].try_into().unwrap());
            println!("  root vtable ptr  : 0x{vptr:016X} -> {:?}", Rsc7::classify(vptr));
        }

        Commands::Mesh { name, out } => {
            std::fs::create_dir_all(out)?;
            let yft = format!("{name}.yft");
            let file = veh.find_file(&yft).with_context(|| format!("{yft} not found"))?;
            let rsc = veh.extract(file, Some(&keys))?;
            let r = Rsc7::parse(&rsc)?;

            let mesh = yft::decode(&r)?;
            let bones = skeleton::parse(&r)?;
            let bone_name = |idx: u16| -> String {
                bones.get(idx as usize).map(|b| b.name.clone()).unwrap_or_else(|| format!("#{idx}"))
            };
            let (mut tv, mut ti) = (0usize, 0usize);
            println!("\n{yft}: {} geometries, {} bones", mesh.geometries.len(), bones.len());
            for (i, g) in mesh.geometries.iter().enumerate() {
                tv += g.positions.len();
                ti += g.indices.len();
                if i < 12 {
                    // For a skinned geom, show vertex 0's dominant bone (highest weight).
                    let skin = if g.skinned && !g.bone_idx.is_empty() {
                        let (idx0, wt0) = (g.bone_idx[0], g.bone_wt[0]);
                        let best = (0..4).max_by_key(|&k| wt0[k]).unwrap();
                        format!("  skin→{}", bone_name(idx0[best]))
                    } else {
                        String::new()
                    };
                    println!(
                        "  geom {i:>3}: {:>6} verts  {:>6} tris  stride {:>3}  fvf 0x{:08X}  shader {}{}",
                        g.positions.len(),
                        g.indices.len() / 3,
                        g.stride,
                        g.fvf_flags,
                        g.shader_index,
                        skin,
                    );
                }
            }
            println!("  TOTAL: {tv} verts, {} tris", ti / 3);
            if !mesh.children.is_empty() {
                println!("  physics children with geometry (wheels/doors/etc.):");
                for (i, bonetag, ngeo) in &mesh.children {
                    println!("    child {i:>2}: boneTag {bonetag:>5}  {ngeo} geometries");
                }
            }

            let obj = out.join(format!("{name}.obj"));
            yft::write_obj(&mesh, &obj)?;
            println!("  wrote {}", obj.display());
        }

        Commands::Textures { ytd, out } => {
            let file = veh.find_file(ytd).with_context(|| format!("{ytd} not found"))?;
            let rsc = veh.extract(file, Some(&keys))?;
            let r = Rsc7::parse(&rsc)?;
            let texs = textures::decode_dictionary(&r)?;

            let dir = out.join(textures::sanitize(ytd.trim_end_matches(".ytd")));
            std::fs::create_dir_all(&dir)?;
            println!("\n{ytd}: {} textures", texs.len());
            for t in &texs {
                let png = dir.join(format!("{}.png", textures::sanitize(&t.name)));
                textures::write_png(t, &png)?;
                println!("  {:<28} {:>4}x{:<4} {:?}", t.name, t.width, t.height, t.format);
            }
            println!("  wrote {} PNGs to {}", texs.len(), dir.display());
        }

        Commands::Build { name, out } => {
            std::fs::create_dir_all(out)?;
            let bytes = pipeline::build_vehicle(&veh, &keys, name, true)?;
            let dest = out.join(format!("{name}.gveh"));
            std::fs::write(&dest, &bytes)?;
            println!(
                "\nwrote {} ({:.2} MB) -> {}",
                bytes.len(),
                bytes.len() as f64 / 1_048_576.0,
                dest.display()
            );
        }

        Commands::Skeleton { name } => {
            let yft = format!("{name}.yft");
            let file = veh.find_file(&yft).with_context(|| format!("{yft} not found"))?;
            let rsc = veh.extract(file, Some(&keys))?;
            let r = Rsc7::parse(&rsc)?;
            let bones = skeleton::parse(&r)?;
            println!("\n{yft}:");
            skeleton::print_tree(&bones);
        }

        Commands::Parts { name } => {
            let yft = format!("{name}.yft");
            let file = veh.find_file(&yft).with_context(|| format!("{yft} not found"))?;
            let rsc = veh.extract(file, Some(&keys))?;
            let r = Rsc7::parse(&rsc)?;
            let mesh = yft::decode(&r)?;
            let bones = skeleton::parse(&r)?;
            let world = skeleton::world_transforms(&bones);
            let grouped = parts::group(&mesh, &bones, &world);
            println!("\n{yft}: {} parts", grouped.len());
            for p in &grouped {
                let verts: usize = p.geometries.iter().map(|g| g.positions.len()).sum();
                let tris: usize = p.geometries.iter().map(|g| g.indices.len() / 3).sum();
                if let Some(h) = &p.hinge {
                    println!(
                        "  {:<16} {:>6} verts {:>6} tris  HINGE pos=({:.2},{:.2},{:.2}) axis=({:.0},{:.0},{:.0})",
                        p.name, verts, tris, h.pos[0], h.pos[1], h.pos[2], h.axis[0], h.axis[1], h.axis[2]
                    );
                } else {
                    println!("  {:<16} {:>6} verts {:>6} tris  (body)", p.name, verts, tris);
                }
            }
        }

        Commands::Shaders { name } => {
            let yft = format!("{name}.yft");
            let file = veh.find_file(&yft).with_context(|| format!("{yft} not found"))?;
            let rsc = veh.extract(file, Some(&keys))?;
            let r = Rsc7::parse(&rsc)?;
            let mesh = yft::decode(&r)?;
            let infos = shaders::shader_infos(&r)?;
            let needed: Vec<String> = infos
                .iter()
                .flat_map(|i| [i.diffuse.clone(), i.normal.clone()])
                .flatten()
                .collect();
            let tex_map = pipeline::texture_map(&veh, &keys, name, &r, &needed);
            println!("\n{yft}: {} shaders, {} textures available", infos.len(), tex_map.len());

            let mut seen = std::collections::BTreeSet::new();
            for g in &mesh.geometries {
                if let Some(info) = infos.get(g.shader_index as usize) {
                    if let Some(d) = &info.diffuse {
                        seen.insert(d.clone());
                    }
                    if let Some(n) = &info.normal {
                        seen.insert(n.clone());
                    }
                }
            }
            let (mut found, mut missing) = (0, 0);
            for tex in &seen {
                match tex_map.get(&tex.to_lowercase()) {
                    Some(t) => {
                        found += 1;
                        println!("  [ok]   {tex:<32} {}x{} {:?}", t.width, t.height, t.format);
                    }
                    None => {
                        missing += 1;
                        println!("  [MISS] {tex}");
                    }
                }
            }
            println!("\nresolved {found}/{} used textures ({missing} missing)", seen.len());
        }

        Commands::Joints { name } => {
            let yft = format!("{name}.yft");
            let file = veh.find_file(&yft).with_context(|| format!("{yft} not found"))?;
            let rsc = veh.extract(file, Some(&keys))?;
            let r = Rsc7::parse(&rsc)?;
            let js = joints::parse(&r)?;
            let tags = joints::link_bone_tags(&r);
            let bones = skeleton::parse(&r)?;
            let bone_for_tag = |tag: u16| -> String {
                bones.iter().find(|b| b.tag == tag).map(|b| b.name.clone()).unwrap_or_else(|| format!("tag{tag}"))
            };
            let link = |i: u8| -> String {
                tags.get(i as usize).map(|&t| bone_for_tag(t)).unwrap_or_else(|| format!("link{i}"))
            };
            println!("\n{yft}: {} joints", js.len());
            for j in &js {
                let ty = if j.jtype == 0 { "1Dof" } else { "3Dof" };
                println!("  {ty}  {} -> {}", link(j.parent_link), link(j.child_link));
                for (k, v) in j.vecs.iter().enumerate() {
                    let off = 0x20 + k * 16;
                    println!("    [0x{off:02X}] ({:7.3} {:7.3} {:7.3} {:7.3})", v[0], v[1], v[2], v[3]);
                }
            }
        }

        Commands::Serve { addr } => {
            let sock = addr.parse().with_context(|| format!("bad addr {addr}"))?;
            return server::serve(veh, keys, sock);
        }
    }

    Ok(())
}

fn extract_one(veh: &Archive, keys: &GtaKeys, name: &str, out: &std::path::Path) -> Result<()> {
    match veh.find_file(name) {
        None => {
            println!("  [miss] {name} not found");
            Ok(())
        }
        Some(file) => {
            let kind = match veh.entry_kind(file) {
                RpfEntryKind::ResourceFile { .. } => "resource",
                _ => "file",
            };
            let bytes = veh
                .extract(file, Some(keys))
                .with_context(|| format!("extracting {name}"))?;
            let dest = out.join(name.replace('+', "_plus_"));
            std::fs::write(&dest, &bytes)?;
            println!(
                "  [ok]   {name}  ({kind}, {} bytes) -> {}",
                bytes.len(),
                dest.display()
            );
            Ok(())
        }
    }
}
