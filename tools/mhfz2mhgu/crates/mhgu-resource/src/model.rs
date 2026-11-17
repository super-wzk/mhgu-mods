//! Switch rModel (MOD 0xD4), skinned 0x14D40022 vertex layout.
use crate::binary::Fields;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, Vec3};
use half::f16;
use std::collections::BTreeMap;

pub struct Bone {
    pub parent: Option<u8>,
    pub local: Mat4,
    pub world: Mat4,
}
#[derive(Clone, Copy)]
struct Bounds {
    lo: Vec3,
    hi: Vec3,
}
impl Default for Bounds {
    fn default() -> Self {
        Self {
            lo: Vec3::splat(f32::INFINITY),
            hi: Vec3::splat(f32::NEG_INFINITY),
        }
    }
}
impl Bounds {
    fn add(&mut self, p: Vec3) {
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }
    fn center(self) -> Vec3 {
        (self.lo + self.hi) * 0.5
    }
    fn half(self) -> Vec3 {
        (self.hi - self.lo) * 0.5
    }
    fn sphere(self) -> [f32; 4] {
        self.center().extend(self.half().length()).to_array()
    }
    fn write(&self, out: &mut Vec<u8>) {
        out.floats(self.lo.extend(0.).to_array());
        out.floats(self.hi.extend(0.).to_array());
    }
}

/// Model-space vertex. Bone indices address the supplied skeleton.
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
    pub tangent: Vec3,
    pub bitangent: Vec3,
    pub bones: [u8; 4],
    pub weights: [f32; 4],
}
/// Draw group with local triangle-strip indices per material; 0xffff restarts a strip.
pub struct MeshGroup {
    pub vertices: Vec<Vertex>,
    pub strips: BTreeMap<u32, Vec<u16>>,
}

fn row(v: &Vertex, b: Bounds, side: f32, out: &mut Vec<u8>) {
    for f in ((v.position - b.lo) / side).to_array() {
        out.u16((f.clamp(0., 1.) * 32767.).round() as u16);
    }
    let [w0, w1, w2, _] = v.weights;
    let wh = [f16::from_f32(w1), f16::from_f32(w2)];
    // The last weight is implicit; keep rounding from making it negative.
    out.u16(
        ((w0 * 32767.).round() as u16)
            .min(((1. - wh[0].to_f32() - wh[1].to_f32()) * 32767.).floor() as u16),
    );
    let mut t = (v.tangent - v.normal * v.normal.dot(v.tangent)).normalize_or_zero();
    if t == Vec3::ZERO {
        t = if v.normal == Vec3::ZERO {
            Vec3::X
        } else {
            v.normal.any_orthonormal_vector()
        };
    }
    let sign = if v.normal.cross(t).dot(v.bitangent) < 0. {
        -1.
    } else {
        1.
    };
    for f in v
        .normal
        .extend(1.)
        .to_array()
        .into_iter()
        .chain(t.extend(sign).to_array())
    {
        out.push((f.clamp(-1., 1.) * 127.).round() as i8 as u8);
    }
    out.extend(v.bones);
    for h in [f16::from_f32(v.uv.x), f16::from_f32(v.uv.y), wh[0], wh[1]] {
        out.u16(h.to_bits());
    }
}

fn envelope(out: &mut Vec<u8>, bone: u8, b: Bounds) {
    out.u32(bone.into());
    out.extend([0; 12]);
    out.floats(b.sphere());
    b.write(out);
    out.floats(Mat4::from_translation(b.center()).to_cols_array());
    out.floats(b.half().extend(0.).to_array());
}

/// Encode draw groups, bind poses and referenced material names as a single MOD.
pub fn encode(
    joints: &[Bone],
    objects: &[MeshGroup],
    material_names: &[String],
) -> Result<Vec<u8>> {
    let logical: Vec<_> = (0..joints.len())
        .map(u8::try_from)
        .collect::<Result<_, _>>()?;
    encode_with_ids(joints, objects, material_names, &logical)
}

pub fn encode_with_ids(
    joints: &[Bone],
    objects: &[MeshGroup],
    material_names: &[String],
    logical: &[u8],
) -> Result<Vec<u8>> {
    ensure!(joints.len() == logical.len(), "missing logical joint IDs");
    let mut remap = [255u8; 256];
    for (i, &id) in logical.iter().enumerate() {
        ensure!(
            id != 255 && remap[usize::from(id)] == 255,
            "duplicate or invalid logical joint"
        );
        remap[usize::from(id)] = i.try_into()?;
    }
    ensure!(
        !joints.is_empty() && joints.len() <= 255,
        "MOD needs 1..=255 bone records"
    );
    let materials = material_names.len();
    ensure!(
        materials <= 4096,
        "too many materials for a 12-bit MOD index"
    );
    ensure!(
        !objects.is_empty() && objects.len() <= 4096,
        "expected 1..=4096 MOD draw groups"
    );
    let mut bounds = Bounds::default();
    let mut groups = Vec::with_capacity(32 * objects.len());
    for (group, o) in objects.iter().enumerate() {
        ensure!(
            !o.vertices.is_empty() && o.vertices.len() <= u16::MAX as usize,
            "vertex count does not fit a MOD draw"
        );
        let mut b = Bounds::default();
        for v in &o.vertices {
            b.add(v.position);
        }
        bounds.add(b.lo);
        bounds.add(b.hi);
        groups.u32(group as u32);
        groups.extend([0; 12]);
        groups.floats(b.sphere());
    }
    let side = (bounds.hi - bounds.lo).max_element();
    let side = if side == 0. { 1. } else { side };
    let dequant =
        Mat4::from_scale_rotation_translation(Vec3::splat(side), glam::Quat::IDENTITY, bounds.lo);
    let mut vb = vec![];
    let mut ib = vec![];
    let mut meshes = vec![];
    let mut envs = vec![];
    let mut furthest = vec![0f32; joints.len()];
    for (group, o) in objects.iter().enumerate() {
        let stream = u32::try_from(vb.len())?;
        for v in &o.vertices {
            row(v, bounds, side, &mut vb);
        }
        for (&mat, indices) in &o.strips {
            ensure!(
                (mat as usize) < materials,
                "MOD material {mat} not in material table"
            );
            let start = u32::try_from(ib.len() / 2)?;
            let mut used = BTreeMap::<u8, Bounds>::new();
            let mut nweights = 0;
            let mut min = u16::MAX;
            let mut max = 0;
            for &i in indices {
                ib.u16(i);
                if i == u16::MAX {
                    continue;
                }
                min = min.min(i);
                max = max.max(i);
                let v = o
                    .vertices
                    .get(i as usize)
                    .context("strip index outside MOD vertices")?;
                let active = v.weights.iter().filter(|&&w| w > 0.).count();
                nweights = nweights.max(active);
                for (&b, &w) in v.bones.iter().zip(&v.weights) {
                    if w <= 0. {
                        continue;
                    }
                    ensure!(
                        (b as usize) < joints.len(),
                        "vertex refers to missing MOD bone {b}"
                    );
                    used.entry(b).or_default().add(v.position);
                    let i = b as usize;
                    furthest[i] =
                        furthest[i].max(v.position.distance(joints[i].world.w_axis.truncate()));
                }
            }
            ib.u16(u16::MAX);
            let mesh_id = u16::try_from(meshes.len() / 48 + 1)?;
            meshes.u16(0xefff);
            meshes.u16(o.vertices.len() as u16);
            for x in [
                0xff00_0000 | (mat << 12) | group as u32,
                1 | ((nweights.max(3) as u32) << 3) | (28 << 16) | (4 << 24),
                0,
                stream,
                0x14d40022,
                start,
                u32::try_from(indices.len())?,
                0,
            ] {
                meshes.u32(x);
            }
            meshes.extend([0, u8::try_from(used.len())?]);
            meshes.u16(mesh_id);
            meshes.u16(min);
            meshes.u16(max);
            meshes.u32(0);
            for (bone, b) in used {
                envelope(&mut envs, bone, b);
            }
        }
    }
    let mut bones = vec![];
    for (i, b) in joints.iter().enumerate() {
        bones.extend([logical[i], b.parent.unwrap_or(255), i as u8, 0]);
        let p = b.local.w_axis.truncate();
        bones.floats([furthest[i], p.length(), p.x, p.y, p.z]);
    }
    for b in joints {
        bones.floats(b.local.to_cols_array());
    }
    for b in joints {
        bones.floats((b.world.inverse() * dequant).to_cols_array());
    }
    bones.extend(remap);
    let mut names = vec![];
    for s in material_names {
        let mut name = [0; 128];
        ensure!(
            s.len() < 128 && !s.as_bytes().contains(&0),
            "material name cannot fit the 128-byte MOD field"
        );
        name[..s.len()].copy_from_slice(s.as_bytes());
        names.extend(name);
    }
    let mut out = b"MOD\0".to_vec();
    out.u16(0xd4);
    out.u16(joints.len() as u16);
    out.u16(u16::try_from(meshes.len() / 48)?);
    out.u16(materials as u16);
    for n in [vb.len() / 28, ib.len() / 2, 0, vb.len(), 0, objects.len()] {
        out.u32(u32::try_from(n)?);
    }
    let mut pos = 0x84usize;
    for size in [
        bones.len(),
        groups.len(),
        names.len(),
        meshes.len() + envs.len(),
        vb.len(),
        ib.len(),
        0,
    ] {
        out.u32(u32::try_from(pos)?);
        pos += size;
    }
    out.floats(bounds.sphere());
    bounds.write(&mut out);
    for v in [1000, 3000, 0x80000001, 0, u32::try_from(envs.len() / 144)?] {
        out.u32(v);
    }
    for block in [bones, groups, names, meshes, envs, vb, ib] {
        out.extend(block);
    }
    Ok(out)
}

/// Native unskinned MOD for a dropped monster part (float XYZ, packed normal,
/// colour and two half UV pairs, layout 5E7F2030).
pub fn static_prop(o: &MeshGroup, names: &[String]) -> Result<Vec<u8>> {
    let mut b = Bounds::default();
    let mut vb = Vec::new();
    for v in &o.vertices {
        b.add(v.position);
        vb.floats(v.position.to_array());
        for x in v.normal.extend(0.).to_array() {
            vb.push((x.clamp(-1., 1.) * 127.).round() as i8 as u8);
        }
        vb.extend([255; 4]);
        for x in [v.uv.x, v.uv.y, v.uv.x, v.uv.y] {
            vb.u16(f16::from_f32(x).to_bits());
        }
    }
    let mut groups = vec![0; 16];
    groups.floats(b.sphere());
    let mut materials = vec![];
    for name in names {
        let mut row = [0; 128];
        row[..name.len()].copy_from_slice(name.as_bytes());
        materials.extend(row);
    }
    let mut meshes = vec![];
    let mut ib = vec![];
    let mut env = vec![];
    for (&material, strip) in &o.strips {
        let mut mb = Bounds::default();
        let verts = strip.iter().copied().filter(|&i| i != u16::MAX);
        for i in verts.clone() {
            mb.add(o.vertices[usize::from(i)].position);
        }
        meshes.u16(0xefff);
        meshes.u16(o.vertices.len().try_into()?);
        for n in [
            0xff00_0000 | material << 12,
            0x441c0001,
            0,
            0,
            0x5e7f2030,
            (ib.len() / 2) as u32,
            strip.len() as u32,
            0,
        ] {
            meshes.u32(n);
        }
        meshes.extend([0, 1]);
        meshes.u16((env.len() / 144 + 1) as u16);
        meshes.u16(verts.clone().min().context("empty static draw")?);
        meshes.u16(verts.max().unwrap());
        meshes.u32(0);
        for &i in strip {
            ib.u16(i);
        }
        ib.u16(u16::MAX);
        envelope(&mut env, 255, mb);
    }
    let mut out = b"MOD\0".to_vec();
    out.u16(0xd4);
    out.u16(0);
    out.u16(o.strips.len().try_into()?);
    out.u16(names.len().try_into()?);
    for v in [o.vertices.len(), ib.len() / 2, 0, vb.len(), 0, 1] {
        out.u32(v.try_into()?);
    }
    out.u32(0);
    let mut pos = 0x84;
    for size in [
        groups.len(),
        materials.len(),
        meshes.len() + env.len(),
        vb.len(),
        ib.len(),
        0,
    ] {
        out.u32(pos as u32);
        pos += size;
    }
    out.floats(b.sphere());
    b.write(&mut out);
    for v in [1000, 3000, 0x80000001, 0, o.strips.len() as u32] {
        out.u32(v);
    }
    for data in [groups, materials, meshes, env, vb, ib] {
        out.extend(data);
    }
    Ok(out)
}
