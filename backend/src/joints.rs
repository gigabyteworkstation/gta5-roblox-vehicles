//! GTA fragment physics JOINTS — the generic articulation data (how GTA
//! actually rigs doors/hood/boot/etc.). Each joint connects two physics links
//! (children/bones) with a type + frame + angle limits.
//!
//! FragType.PhysLODGroup -> LOD1 -> ArticulatedBodyType -> Joints[].
//! phJointType base: m_JointType@0x15, m_ParentLinkIndex@0x16, m_ChildLinkIndex
//! @0x17, then m_OrientParent / m_OrientChild (Mat34V) + 1Dof angle limits —
//! the 9 Vector4s at 0x20.. (CodeWalker's "Unknown_*").

use crate::rsc7::Rsc7;
use anyhow::Result;

const ROOT: u64 = 0x5000_0000;
const FRAGTYPE_PHYSLODGROUP_PTR: usize = 0xF0;
const LODGROUP_LOD1_PTR: usize = 0x10;
const LOD_ARTICULATED_PTR: usize = 0x20;
const LOD_CHILDREN_PTR: usize = 0xD0;
const LOD_CHILDREN_COUNT: usize = 0x11D; // u8
const CHILD_BONETAG: usize = 0x0A; // u16

const ABT_JOINTS_PTR: usize = 0x78;
const ABT_JOINTS_COUNT: usize = 0x89; // u8

const J_TYPE: usize = 0x15; // u8: 0 = 1Dof (hinge), 1 = 3Dof
const J_PARENT_LINK: usize = 0x16; // u8
const J_CHILD_LINK: usize = 0x17; // u8
const J_VECS: usize = 0x20; // 9 x Vector4

pub struct Joint {
    pub jtype: u8,
    pub parent_link: u8,
    pub child_link: u8,
    pub vecs: [[f32; 4]; 9],
}

/// Bone tag for each physics link (child index → bone tag).
pub fn link_bone_tags(rsc: &Rsc7) -> Vec<u16> {
    let lod1 = rsc
        .u64_at(ROOT, FRAGTYPE_PHYSLODGROUP_PTR)
        .and_then(|g| if g == 0 { Ok(0) } else { rsc.u64_at(g, LODGROUP_LOD1_PTR) })
        .unwrap_or(0);
    if lod1 == 0 {
        return vec![];
    }
    let children_ptr = rsc.u64_at(lod1, LOD_CHILDREN_PTR).unwrap_or(0);
    let count = rsc.u8_at(lod1, LOD_CHILDREN_COUNT).unwrap_or(0) as usize;
    if children_ptr == 0 || count == 0 {
        return vec![];
    }
    let ptrs = rsc.ptr_array(children_ptr, count).unwrap_or_default();
    ptrs.iter()
        .map(|&c| if c == 0 { 0 } else { rsc.u16_at(c, CHILD_BONETAG).unwrap_or(0) })
        .collect()
}

pub fn parse(rsc: &Rsc7) -> Result<Vec<Joint>> {
    let lodgroup = rsc.u64_at(ROOT, FRAGTYPE_PHYSLODGROUP_PTR)?;
    if lodgroup == 0 {
        return Ok(vec![]);
    }
    let lod1 = rsc.u64_at(lodgroup, LODGROUP_LOD1_PTR)?;
    if lod1 == 0 {
        return Ok(vec![]);
    }
    let abt = rsc.u64_at(lod1, LOD_ARTICULATED_PTR)?;
    if abt == 0 {
        return Ok(vec![]);
    }
    let joints_ptr = rsc.u64_at(abt, ABT_JOINTS_PTR)?;
    let count = rsc.u8_at(abt, ABT_JOINTS_COUNT)? as usize;
    if joints_ptr == 0 || count == 0 {
        return Ok(vec![]);
    }
    let ptrs = rsc.ptr_array(joints_ptr, count)?;

    let mut out = Vec::new();
    for &j in &ptrs {
        if j == 0 {
            continue;
        }
        let mut vecs = [[0f32; 4]; 9];
        for k in 0..9 {
            if let Ok(b) = rsc.at(j + (J_VECS + k * 16) as u64, 16) {
                for c in 0..4 {
                    vecs[k][c] = f32::from_le_bytes(b[c * 4..c * 4 + 4].try_into().unwrap());
                }
            }
        }
        out.push(Joint {
            jtype: rsc.u8_at(j, J_TYPE)?,
            parent_link: rsc.u8_at(j, J_PARENT_LINK)?,
            child_link: rsc.u8_at(j, J_CHILD_LINK)?,
            vecs,
        });
    }
    Ok(out)
}
