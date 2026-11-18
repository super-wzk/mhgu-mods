use anyhow::{Context, Result, bail, ensure};
use glam::{Mat4, Quat, Vec2, Vec3};
use mhf_resource::{
    fmod::{Component, FaceGroup, Fmod, Object, TriangleStrip},
    rig::Rig,
};
use mhgu_resource::{
    lmt::{Track, Usage},
    model::{self, Bone, MeshGroup, Vertex},
};
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
    encode_body(source, rig, None)
}

// Keep skin/animation joints in a disjoint logical range. GU's native
// fly-wyvern callbacks address numbered attachments (wings, head, feet, tail).
// These sockets follow the actual FZ joints, but legacy requests on a socket
// cannot transform the skinned joint. Vertex indices stay in original PAC order.
pub const HUNT_JOINTS: [u8; 48] = {
    let mut ids = [0; 48];
    let mut i = 0;
    while i < 48 {
        ids[i] = 148 + i as u8;
        i += 1;
    }
    ids
};
const WYVERN_SOCKETS: &[(u8, usize)] = &[
    (0, 0),
    (1, 2),
    (2, 3),
    (100, 29),
    (101, 29),
    (102, 30),
    (103, 31),
    (3, 34),
    (4, 35),
    (5, 36),
    (6, 4),
    (7, 5),
    (8, 6),
    (202, 7),
    (9, 7),
    (60, 8),
    (61, 9),
    (10, 10),
    (11, 11),
    (12, 12),
    (201, 13),
    (13, 13),
    (70, 14),
    (71, 15),
    (200, 3),
    (15, 17),
    (16, 18),
    (17, 19),
    (18, 20),
    (80, 22),
    (81, 21),
    (19, 23),
    (20, 24),
    (21, 25),
    (22, 26),
    (90, 28),
    (91, 27),
    (140, 40),
    (141, 41),
    (142, 42),
    (143, 42),
    (144, 43),
    (145, 44),
];

pub fn hunt_body(source: &[u8], rig: &Rig, spawn: &[Track]) -> Result<Vec<u8>> {
    encode_body(source, rig, Some(spawn))
}

/// Weighted points on the two feet in the original em080 skin. Keep the
/// inverse bind per influence: a toe can also be weighted to the ankle.
pub struct Soles(Vec<Vec<(usize, f32, Vec3)>>);

impl Soles {
    pub fn espinas(source: &[u8], rig: &Rig) -> Result<Self> {
        let model = Fmod::parse(source)?;
        let mut points = Vec::new();
        for object in model.objects() {
            let body = extract(object, rig)?;
            points.extend(
                body.vertices
                    .iter()
                    .filter(|v| matches!(v.bones[0], 20..=22 | 26..=28))
                    .map(|v| {
                        v.bones
                            .iter()
                            .zip(v.weights)
                            .filter(|&(_, w)| w > 0.)
                            .map(|(&joint, w)| {
                                let i = usize::from(joint);
                                (
                                    i,
                                    w,
                                    rig.joints[i].world.inverse().transform_point3(v.position),
                                )
                            })
                            .collect::<Vec<_>>()
                    }),
            );
        }
        ensure!(!points.is_empty(), "Espinas foot skin missing");
        Ok(Self(points))
    }

    /// GU has no Frontier foot solver. Apply only the measured penetration to
    /// the visible skeleton; the actor's position and horizontal travel stay intact.
    pub fn ground_sleep(
        &self,
        rig: &Rig,
        ids: &[u8],
        frames: u32,
        tracks: &mut [Track],
    ) -> Result<()> {
        let mut indices = Vec::with_capacity(ids.len());
        for &id in ids {
            let index = |usage| {
                tracks
                    .iter()
                    .position(|t| t.bone == id && t.usage == usage)
                    .context("incomplete sleep pose")
            };
            indices.push([
                index(Usage::Scale)?,
                index(Usage::Rotation)?,
                index(Usage::Translation)?,
            ]);
        }
        let root_y = indices[rig.roots[0]][2];
        let mut corrected = Vec::with_capacity(frames as usize);
        let mut world = vec![Mat4::IDENTITY; ids.len()];
        for f in 0..frames as usize {
            let sample = |i: usize| {
                let values = &tracks[i].samples;
                values[if values.len() == 1 { 0 } else { f }]
            };
            for (i, &[s, r, p]) in indices.iter().enumerate() {
                let local = Mat4::from_scale_rotation_translation(
                    sample(s).truncate(),
                    Quat::from_array(sample(r).to_array()),
                    sample(p).truncate(),
                );
                world[i] = rig.joints[i]
                    .parent
                    .map_or(local, |parent| world[parent] * local);
            }
            let floor = self
                .0
                .iter()
                .map(|v| {
                    v.iter()
                        .map(|&(j, w, p)| world[j].transform_point3(p).y * w)
                        .sum::<f32>()
                })
                .fold(0., f32::min);
            let mut root = sample(root_y);
            root.y -= floor;
            corrected.push(root);
        }
        if corrected.iter().all(|&p| p == corrected[0]) {
            corrected.truncate(1);
        }
        tracks[root_y].samples = corrected;
        Ok(())
    }
}

fn encode_body(source: &[u8], rig: &Rig, spawn: Option<&[Track]>) -> Result<Vec<u8>> {
    let f = Fmod::parse(source)?;
    let names = f
        .materials()
        .enumerate()
        .map(|(i, _)| material_name(i))
        .collect::<Vec<_>>();
    let mut objects = f
        .objects()
        .enumerate()
        .map(|(i, o)| extract(o, rig).with_context(|| format!("FMOD object {i}")))
        .collect::<Result<Vec<_>>>()?;
    if spawn.is_some() {
        // 43/44 and 46/47 have identical local orientation and child length.
        // Rebind the prop's two weighted joints to the animated body tip.
        let tail = objects.get_mut(1).context("missing tail object")?;
        let from = rig.joints[46].world;
        let to = rig.joints[43].world;
        let transform = to * from.inverse();
        for v in &mut tail.vertices {
            v.position = transform.transform_point3(v.position);
            v.normal = transform.transform_vector3(v.normal).normalize_or_zero();
            v.tangent = transform.transform_vector3(v.tangent);
            v.bitangent = transform.transform_vector3(v.bitangent);
            for joint in &mut v.bones {
                *joint = match *joint {
                    46 => 43,
                    47 => 44,
                    _ => anyhow::bail!("unexpected detached tail skin"),
                };
            }
        }
    }
    let mut bones = rig
        .joints
        .iter()
        .enumerate()
        .map(|(i, b)| {
            // GU can draw the base skeleton before an LMT is active. Keep the
            // original inverse bind for skinning, but initialize the local pose
            // to frame zero of the same FZ clip that the action will start.
            let local = if let Some(tracks) = spawn {
                let sample = |usage| {
                    tracks
                        .iter()
                        .find(|t| t.bone == i as u8 && t.usage == usage)
                        .map(|t| t.samples[0])
                        .context("incomplete Espinas spawn pose")
                };
                Mat4::from_scale_rotation_translation(
                    sample(Usage::Scale)?.truncate(),
                    Quat::from_array(sample(Usage::Rotation)?.to_array()),
                    sample(Usage::Translation)?.truncate(),
                )
            } else {
                b.local
            };
            Ok(Bone {
                parent: b.parent.map(u8::try_from).transpose()?,
                local,
                world: b.world,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if spawn.is_some() {
        ensure!(
            bones.len() == HUNT_JOINTS.len(),
            "unexpected Espinas skeleton"
        );
        let mut logical = HUNT_JOINTS.to_vec();
        for &(id, parent) in WYVERN_SOCKETS {
            bones.push(Bone {
                parent: Some(parent as u8),
                local: Mat4::IDENTITY,
                world: rig.joints[parent].world,
            });
            logical.push(id);
        }
        model::encode_with_ids(&bones, &objects, &names, &logical)
    } else {
        model::encode(&bones, &objects, &names)
    }
}

/// Detached em080 tail: the PAC authors object 1 in its own root coordinate
/// system. GU instantiates this resource as an unskinned object after a cut.
pub fn detached_tail(source: &[u8], rig: &Rig) -> Result<Vec<u8>> {
    let f = Fmod::parse(source)?;
    let tail = extract(f.objects().nth(1).context("missing detached tail")?, rig)?;
    ensure!(
        tail.vertices.iter().all(|v| v
            .bones
            .iter()
            .zip(v.weights)
            .all(|(&b, w)| w == 0. || matches!(b, 45..=47))),
        "unexpected detached tail bind"
    );
    let names = (0..f.materials().count())
        .map(material_name)
        .collect::<Vec<_>>();
    model::static_prop(&tail, &names)
}
