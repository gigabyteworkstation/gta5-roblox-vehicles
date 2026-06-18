//! GTA5 skeleton (crSkeletonData) parsing — bones for articulation/skinning.
//!
//! FragType.Drawable -> DrawableBase.SkeletonPointer -> Skeleton -> Bones[].
//! Each Bone carries a LOCAL transform (relative to parent), a parent index, a
//! stable Tag (the bone id GTA references, e.g. for wheel_lf), and a name.

use crate::rsc7::Rsc7;
use anyhow::Result;

const ROOT: u64 = 0x5000_0000;
const FRAGTYPE_DRAWABLE_PTR: usize = 0x30;
const DB_SKELETON_PTR: usize = 0x18;
const SKEL_BONES_PTR: usize = 0x20;
const SKEL_BONES_COUNT: usize = 0x5E; // u16

const BONE_SIZE: u64 = 0x50; // 80 bytes
const O_ROTATION: usize = 0x00; // vec4 (x,y,z,w)
const O_TRANSLATION: usize = 0x10; // vec3
const O_SCALE: usize = 0x20; // vec3
const O_PARENT: usize = 0x32; // i16
const O_NAME_PTR: usize = 0x38; // u64
const O_INDEX: usize = 0x42; // i16
const O_TAG: usize = 0x44; // u16

#[derive(Clone)]
pub struct Bone {
    pub index: i16,
    pub name: String,
    pub tag: u16,
    pub parent: i16,
    pub translation: [f32; 3],
    pub rotation: [f32; 4], // quaternion x,y,z,w (local)
    pub scale: [f32; 3],
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn i16_at(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

pub fn parse(rsc: &Rsc7) -> Result<Vec<Bone>> {
    let drawable = rsc.u64_at(ROOT, FRAGTYPE_DRAWABLE_PTR)?;
    if drawable == 0 {
        return Ok(vec![]);
    }
    let skel = rsc.u64_at(drawable, DB_SKELETON_PTR)?;
    if skel == 0 {
        return Ok(vec![]); // unskinned drawable
    }
    let bones_ptr = rsc.u64_at(skel, SKEL_BONES_PTR)?;
    let count = rsc.u16_at(skel, SKEL_BONES_COUNT)? as usize;
    if bones_ptr == 0 || count == 0 {
        return Ok(vec![]);
    }

    let mut bones = Vec::with_capacity(count);
    for i in 0..count {
        let b = rsc.at(bones_ptr + i as u64 * BONE_SIZE, BONE_SIZE as usize)?;
        let name_ptr = u64::from_le_bytes(b[O_NAME_PTR..O_NAME_PTR + 8].try_into().unwrap());
        bones.push(Bone {
            index: i16_at(b, O_INDEX),
            name: rsc.str_at(name_ptr),
            tag: u16::from_le_bytes(b[O_TAG..O_TAG + 2].try_into().unwrap()),
            parent: i16_at(b, O_PARENT),
            translation: [f32_at(b, O_TRANSLATION), f32_at(b, O_TRANSLATION + 4), f32_at(b, O_TRANSLATION + 8)],
            rotation: [f32_at(b, O_ROTATION), f32_at(b, O_ROTATION + 4), f32_at(b, O_ROTATION + 8), f32_at(b, O_ROTATION + 12)],
            scale: [f32_at(b, O_SCALE), f32_at(b, O_SCALE + 4), f32_at(b, O_SCALE + 8)],
        });
    }
    Ok(bones)
}

/// World-space rest position of each bone, by accumulating local translations up
/// the parent chain. Ignores rotation — fine for vehicle bones, which are
/// axis-aligned at rest, and good enough for a first skinned pass (rest pose is
/// undistorted regardless; rotation only affects deformation quality).
pub fn world_positions(bones: &[Bone]) -> Vec<[f32; 3]> {
    let mut world = vec![[0f32; 3]; bones.len()];
    for i in 0..bones.len() {
        let mut acc = bones[i].translation;
        let mut p = bones[i].parent;
        let mut guard = 0;
        while p >= 0 && (p as usize) < bones.len() {
            let pb = &bones[p as usize];
            acc[0] += pb.translation[0];
            acc[1] += pb.translation[1];
            acc[2] += pb.translation[2];
            p = pb.parent;
            guard += 1;
            if guard > 256 {
                break;
            }
        }
        world[i] = acc;
    }
    world
}

/// Depth of a bone in the hierarchy (for pretty printing). Parent index is into
/// the bones array; -1 = root.
fn depth(bones: &[Bone], mut i: i16) -> usize {
    let mut d = 0;
    while let Some(b) = bones.get(i as usize) {
        if b.parent < 0 {
            break;
        }
        i = b.parent;
        d += 1;
        if d > 256 {
            break;
        }
    }
    d
}

pub fn print_tree(bones: &[Bone]) {
    println!("{} bones:", bones.len());
    for (i, b) in bones.iter().enumerate() {
        let indent = "  ".repeat(depth(bones, i as i16) + 1);
        println!(
            "{indent}[{i:>3}] {:<22} tag={:<6} parent={:<3} t=({:.2},{:.2},{:.2})",
            b.name, b.tag, b.parent, b.translation[0], b.translation[1], b.translation[2]
        );
    }
}
