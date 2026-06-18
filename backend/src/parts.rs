//! Group decoded geometry into rigid PARTS by dominant bone — mirroring how
//! GTA5 groups vehicle meshes (by skinning weight). Each triangle is assigned
//! to the bone it's most weighted to; triangles on an *articulated* bone (door,
//! hood, boot) split off into their own part with a hinge, everything else
//! merges into the "body".

use crate::skeleton::Bone;
use crate::yft::{Geometry, Mesh};

pub struct Hinge {
    pub pos: [f32; 3],  // RAGE space (client applies the axis swap)
    pub axis: [f32; 3], // RAGE space
    pub min_angle: f32, // radians
    pub max_angle: f32,
}

pub struct Part {
    pub name: String, // "body" or the bone name
    pub articulated: bool,
    pub hinge: Option<Hinge>,
    pub geometries: Vec<Geometry>,
}

const PI: f32 = std::f32::consts::PI;

/// Which bones articulate, and their GTA articulation (Automobile.cpp): the
/// bone-LOCAL hinge axis, and the signed open angle (radians). Driver-side
/// doors open negative, passenger-side positive (mirrored). The world hinge
/// axis is this local axis rotated by the bone's world orientation.
fn articulated_spec(name: &str) -> Option<([f32; 3], f32)> {
    // AXIS_X = (1,0,0), AXIS_Z = (0,0,1) in the bone's local frame.
    if name.starts_with("door_dside") {
        Some(([0.0, 0.0, 1.0], -0.4 * PI))
    } else if name.starts_with("door_pside") {
        Some(([0.0, 0.0, 1.0], 0.4 * PI))
    } else if name == "bonnet" || name == "hood" {
        Some(([1.0, 0.0, 0.0], 0.3 * PI))
    } else if name == "boot" || name == "boot_2" {
        Some(([1.0, 0.0, 0.0], -0.3 * PI))
    } else {
        None
    }
}

fn is_articulated(name: &str) -> bool {
    articulated_spec(name).is_some()
}

/// Dominant bone (skeleton index) for a triangle: the bone with the greatest
/// summed weight across its three vertices. Returns None for unskinned geoms.
fn triangle_bone(g: &Geometry, a: usize, b: usize, c: usize) -> Option<u16> {
    if !g.skinned || g.bone_idx.is_empty() {
        return None;
    }
    let mut acc: std::collections::HashMap<u16, u32> = std::collections::HashMap::new();
    for &v in &[a, b, c] {
        let idx = g.bone_idx[v];
        let wt = g.bone_wt[v];
        for k in 0..4 {
            if wt[k] > 0 {
                *acc.entry(idx[k]).or_insert(0) += wt[k] as u32;
            }
        }
    }
    acc.into_iter().max_by_key(|&(_, w)| w).map(|(b, _)| b)
}

/// Re-emit a subset of a geometry's triangles as a standalone Geometry,
/// remapping the referenced vertices/attributes to a compact 0..n range.
fn extract(src: &Geometry, tris: &[(usize, usize, usize)]) -> Geometry {
    let mut remap: std::collections::HashMap<usize, u32> = std::collections::HashMap::new();
    let mut g = Geometry {
        shader_index: src.shader_index,
        stride: src.stride,
        fvf_flags: src.fvf_flags,
        fvf_types: src.fvf_types,
        skinned: src.skinned,
        ..Default::default()
    };
    let mut map = |old: usize, g: &mut Geometry| -> u32 {
        if let Some(&n) = remap.get(&old) {
            return n;
        }
        let n = g.positions.len() as u32;
        g.positions.push(src.positions[old]);
        if !src.normals.is_empty() {
            g.normals.push(src.normals[old]);
        }
        if !src.uvs.is_empty() {
            g.uvs.push(src.uvs[old]);
        }
        if src.skinned {
            g.bone_idx.push(src.bone_idx[old]);
            g.bone_wt.push(src.bone_wt[old]);
        }
        remap.insert(old, n);
        n
    };
    for &(a, b, c) in tris {
        let na = map(a, &mut g);
        let nb = map(b, &mut g);
        let nc = map(c, &mut g);
        g.indices.push(na);
        g.indices.push(nb);
        g.indices.push(nc);
    }
    g
}

pub fn group(mesh: &Mesh, bones: &[Bone], world: &[([f32; 3], [f32; 4])]) -> Vec<Part> {
    use std::collections::HashMap;
    let bone_name = |idx: u16| bones.get(idx as usize).map(|b| b.name.as_str()).unwrap_or("");

    // Collect triangles per target part name.
    let mut body: Vec<Geometry> = Vec::new();
    // part name -> (hinge axis, list of (geom, tris))
    let mut artic: HashMap<String, Vec<(usize, Vec<(usize, usize, usize)>)>> = HashMap::new();

    for (gi, g) in mesh.geometries.iter().enumerate() {
        // Bucket this geometry's triangles by destination part.
        let mut body_tris: Vec<(usize, usize, usize)> = Vec::new();
        let mut part_tris: HashMap<String, Vec<(usize, usize, usize)>> = HashMap::new();

        let idx = &g.indices;
        for t in (0..idx.len().saturating_sub(2)).step_by(3) {
            let (a, b, c) = (idx[t] as usize, idx[t + 1] as usize, idx[t + 2] as usize);
            let dest = triangle_bone(g, a, b, c)
                .map(bone_name)
                .filter(|n| is_articulated(n))
                .map(|n| n.to_string());
            match dest {
                Some(name) => part_tris.entry(name).or_default().push((a, b, c)),
                None => body_tris.push((a, b, c)),
            }
        }

        if !body_tris.is_empty() {
            body.push(extract(g, &body_tris));
        }
        for (name, tris) in part_tris {
            artic.entry(name).or_default().push((gi, tris));
        }
    }

    let mut parts = vec![Part {
        name: "body".to_string(),
        articulated: false,
        hinge: None,
        geometries: body,
    }];

    for (name, entries) in artic {
        let (local_axis, open) = articulated_spec(&name).unwrap_or(([0.0, 0.0, 1.0], 0.4 * PI));
        // Hinge position + world axis come from the bone's world rest transform:
        // the local axis rotated into world space (so doors swing along the
        // raked pillar, etc.).
        let (pos, axis) = bones
            .iter()
            .position(|b| b.name == name)
            .and_then(|i| world.get(i))
            .map(|(p, q)| (*p, crate::skeleton::quat_rotate(*q, local_axis)))
            .unwrap_or(([0.0, 0.0, 0.0], local_axis));
        let geometries = entries
            .into_iter()
            .map(|(gi, tris)| extract(&mesh.geometries[gi], &tris))
            .collect();
        parts.push(Part {
            name,
            articulated: true,
            hinge: Some(Hinge {
                pos,
                axis,
                min_angle: open.min(0.0),
                max_angle: open.max(0.0),
            }),
            geometries,
        });
    }

    parts
}
