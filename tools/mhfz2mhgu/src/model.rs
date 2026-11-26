use anyhow::{Context, Result, bail, ensure};
use glam::{Vec2, Vec3};
use mhf_resource::{
    fmod::{Component, FaceGroup, Fmod, Object, TriangleStrip},
    rig::Rig,
};
use mhgu_resource::model::{self, Bone, MeshGroup, Vertex};
use std::collections::BTreeMap;

fn extract(o: &Object<'_>, rig: &Rig) -> Result<MeshGroup> {
    extract_mesh(o, Some(rig))
}

pub(super) fn static_mesh(o: &Object<'_>) -> Result<MeshGroup> {
    extract_mesh(o, None)
}

fn extract_mesh(o: &Object<'_>, rig: Option<&Rig>) -> Result<MeshGroup> {
    o.validate_geometry()?;
    let positions = &o.positions().context("missing FMOD positions")?.values;
    let normals = &o.normals().context("missing FMOD normals")?.values;
    let uvs = &o.uvs().context("missing FMOD UVs")?.values;
    ensure!(
        !positions.is_empty() && positions.len() <= u16::MAX as usize,
        "MOD cannot represent {} vertices in one draw",
        positions.len()
    );
    let mut palette = None;
    let mut mats = None;
    let mut map = None;
    for c in &o.components {
        match c {
            Component::BoneMap(b) => palette = Some(&b.values),
            Component::MaterialList(b) => mats = Some(&b.values),
            Component::MaterialMap(b) => map = Some(&b.values),
            _ => {}
        }
    }
    let joint = |local: u32| -> Result<u8> {
        let Some(rig) = rig else { return Ok(0) };
        let id = match palette {
            Some(table) => *table
                .get(local as usize)
                .with_context(|| format!("missing palette entry {local}"))?,
            None => local,
        };
        Ok(u8::try_from(
            *rig.by_id
                .get(&u16::try_from(id)?)
                .with_context(|| format!("unknown FSKL bone ID {id}"))?,
        )?)
    };
    let mut vertices = Vec::with_capacity(positions.len());
    let skin = rig.and_then(|_| o.weights());
    for (i, &p) in positions.iter().enumerate() {
        let mut influences = BTreeMap::<u8, f32>::new();
        if let Some(w) = skin {
            for w in &w.vertices[i].influences {
                ensure!(
                    w.weight.is_finite() && w.weight >= 0.,
                    "invalid weight on vertex {i}"
                );
                if w.weight > 0. {
                    *influences.entry(joint(w.bone_index)?).or_default() += w.weight;
                }
            }
        }
        if influences.is_empty() {
            ensure!(
                palette.is_some_and(|table| table.len() == 1)
                    || rig.is_none_or(|r| r.joints.len() == 1),
                "vertex {i} has no binding in a multi-bone mesh"
            );
            influences.insert(joint(0)?, 1.);
        }
        ensure!(
            influences.len() <= 4,
            "vertex {i}: {} active bones exceed the 4-weight MOD layout",
            influences.len()
        );
        let mut sorted: Vec<_> = influences.into_iter().collect();
        sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
        let sum: f32 = sorted.iter().map(|x| x.1).sum();
        let mut bones = [sorted[0].0; 4];
        let mut weights = [0.; 4];
        for (j, (b, w)) in sorted.into_iter().enumerate() {
            bones[j] = b;
            weights[j] = w / sum;
        }
        let p = Vec3::from_array(p);
        let n = Vec3::from_array(normals[i]).normalize_or_zero();
        let uv = Vec2::new(uvs[i][0], 1. - uvs[i][1]);
        ensure!(
            p.is_finite() && n.is_finite() && uv.is_finite(),
            "non-finite vertex {i}"
        );
        vertices.push(Vertex {
            position: p,
            normal: n,
            uv,
            tangent: Vec3::ZERO,
            bitangent: Vec3::ZERO,
            bones,
            weights,
        });
    }
    let faces = o.faces().context("missing FMOD faces")?;
    let mut strips: BTreeMap<u32, Vec<u16>> = BTreeMap::new();
    let mut ordinal = 0;
    for g in &faces.groups {
        let FaceGroup::Strips(g) = g else {
            bail!("unsupported FMOD face group");
        };
        for strip in &g.strips {
            let slot = match map {
                Some(t) => *t.get(ordinal).context("missing strip material")?,
                None => 0,
            };
            let mat = match mats {
                Some(t) => *t
                    .get(slot as usize)
                    .context("invalid object material index")?,
                None => slot,
            };
            ordinal += 1;
            if strip.indices.len() < 3 {
                continue;
            }
            let indices = strips.entry(mat).or_default();
            append_strip(indices, strip, &mut vertices);
        }
    }
    ensure!(!strips.is_empty(), "no drawable triangle strips");
    Ok(MeshGroup { vertices, strips })
}

fn append_strip(indices: &mut Vec<u16>, strip: &TriangleStrip, vertices: &mut [Vertex]) {
    if !indices.is_empty() {
        indices.push(u16::MAX);
    }
    if strip.reversed() {
        indices.push(strip.indices[0] as u16);
    }
    indices.extend(strip.indices.iter().map(|&i| i as u16));
    for a in strip.indices.windows(3) {
        let [a, b, c] = [a[0] as usize, a[1] as usize, a[2] as usize];
        let p0 = vertices[b].position - vertices[a].position;
        let p1 = vertices[c].position - vertices[a].position;
        let uv0 = vertices[b].uv - vertices[a].uv;
        let uv1 = vertices[c].uv - vertices[a].uv;
        let det = uv0.perp_dot(uv1);
        if det != 0. {
            let t = (p0 * uv1.y - p1 * uv0.y) / det;
            let bt = (p1 * uv0.x - p0 * uv1.x) / det;
            for v in [a, b, c] {
                vertices[v].tangent += t;
                vertices[v].bitangent += bt;
            }
        }
    }
}

pub fn material_name(i: usize) -> String {
    format!("fz_mat_{i:03}")
}

pub fn convert(source: &[u8], rig: &Rig) -> Result<Vec<u8>> {
    let f = Fmod::parse(source)?;
    let names = f
        .materials()
        .enumerate()
        .map(|(i, _)| material_name(i))
        .collect::<Vec<_>>();
    let objects = f
        .objects()
        .enumerate()
        .map(|(i, o)| extract(o, rig).with_context(|| format!("FMOD object {i}")))
        .collect::<Result<Vec<_>>>()?;
    let bones = rig
        .joints
        .iter()
        .map(|bone| {
            Ok(Bone {
                parent: bone.parent.map(u8::try_from).transpose()?,
                local: bone.local,
                world: bone.world,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    model::encode(&bones, &objects, &names)
}
