use crate::fskl::{Fskl, NodeEntry};
use anyhow::{Context, Result, bail, ensure};
use glam::{Mat4, Quat, Vec3};
use std::collections::BTreeMap;

pub struct Joint {
    pub parent: Option<usize>,
    pub child: Option<usize>,
    pub sibling: Option<usize>,
    pub tag: u16,
    pub scale: Vec3,
    pub angles: Vec3,
    pub pos: Vec3,
    pub local: Mat4,
    pub world: Mat4,
}

pub struct Rig {
    pub roots: Vec<usize>,
    pub joints: Vec<Joint>,
    pub by_id: BTreeMap<u16, usize>,
}

// FZ's local matrix applies X, then Y, then Z (row-vector native representation).
pub fn rotation(a: Vec3) -> Quat {
    Quat::from_rotation_z(a.z) * Quat::from_rotation_y(a.y) * Quat::from_rotation_x(a.x)
}

impl Rig {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let s = Fskl::parse(bytes)?;
        ensure!(!s.nodes.is_empty(), "FSKL contains no bones");
        let idx = |i: i32| -> Result<Option<usize>> {
            Ok(if i == -1 {
                None
            } else {
                Some(usize::try_from(i).context("invalid negative FSKL link")?)
            })
        };
        let mut joints = Vec::new();
        let mut by_id = BTreeMap::new();
        for (i, node) in s.nodes.iter().enumerate() {
            let NodeEntry::Bone(b) = node else {
                bail!("unknown FSKL node layout at {i}");
            };
            let id = u16::try_from(b.node_id).context("invalid FZ bone id")?;
            ensure!(
                by_id.insert(id, i).is_none(),
                "duplicate source bone id {id}"
            );
            let scale = Vec3::from_slice(&b.transform.scale);
            let angles = Vec3::from_slice(&b.transform.rotation);
            let pos = Vec3::from_slice(&b.transform.translation);
            ensure!(
                scale.is_finite()
                    && !scale.to_array().contains(&0.0)
                    && angles.is_finite()
                    && pos.is_finite(),
                "non-invertible or non-finite bone {i}"
            );
            let local = Mat4::from_scale_rotation_translation(scale, rotation(angles), pos);
            joints.push(Joint {
                parent: idx(b.parent_index)?,
                child: idx(b.first_child_index)?,
                sibling: idx(b.next_sibling_index)?,
                tag: b.motion_tag as u16,
                scale,
                angles,
                pos,
                local,
                world: Mat4::IDENTITY,
            });
        }
        let roots: Vec<usize> = s.root_indices().iter().map(|&x| x as usize).collect();
        let mut stack: Vec<_> = roots.iter().rev().map(|&i| (i, None)).collect();
        let mut seen = vec![false; joints.len()];
        while let Some((i, parent)) = stack.pop() {
            let b = joints
                .get(i)
                .with_context(|| format!("FSKL link to missing bone {i}"))?;
            ensure!(
                !seen[i] && b.parent == parent,
                "inconsistent skeleton hierarchy at bone {i}"
            );
            seen[i] = true;
            joints[i].world = parent.map_or(Mat4::IDENTITY, |p| joints[p].world) * joints[i].local;
            if let Some(n) = joints[i].sibling {
                stack.push((n, parent));
            }
            if let Some(n) = joints[i].child {
                stack.push((n, Some(i)));
            }
        }
        ensure!(seen.iter().all(|&x| x), "unreachable skeleton nodes");
        Ok(Self {
            roots,
            joints,
            by_id,
        })
    }

    // Native binding follows siblings even when the current tag doesn't match.
    // Only matching nodes consume a track and expose their children.
    pub fn motion_nodes(&self, first: usize, tag: Option<u16>) -> impl Iterator<Item = usize> + '_ {
        let mut pending = vec![first];
        std::iter::from_fn(move || {
            loop {
                let i = pending.pop()?;
                let joint = &self.joints[i];
                pending.extend(joint.sibling);
                if tag.is_none_or(|t| t == joint.tag) {
                    pending.extend(joint.child);
                    return Some(i);
                }
            }
        })
    }

    pub fn bind(&self, count: usize, tag: Option<u16>, root: Option<usize>) -> Result<Vec<usize>> {
        let mut matches = Vec::new();
        let mut sizes = Vec::new();
        for (r, &i) in self.roots.iter().enumerate() {
            if root.is_some_and(|n| n != r) {
                continue;
            }
            let mut first = BTreeMap::new();
            for index in self.motion_nodes(i, None) {
                first.entry(self.joints[index].tag).or_insert(index);
            }
            for (t, i) in first {
                if tag.is_some_and(|n| n != t) {
                    continue;
                }
                let order: Vec<_> = self.motion_nodes(i, Some(t)).collect();
                sizes.push((r, t, order.len()));
                if order.len() == count {
                    matches.push(order)
                }
            }
        }
        if matches.len() != 1 {
            bail!(
                "{count} MOT tracks match {} groups; candidates (root, tag, tracks): {sizes:?}; select the matching root, tag or skeleton",
                matches.len()
            );
        }
        Ok(matches.pop().unwrap())
    }
}
