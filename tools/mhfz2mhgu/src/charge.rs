//! Ordinary Espinas single-action experiment on GU Rathian's existing 1:0 handler.
use crate::{Monster, model, motion, package, read};
use anyhow::{Context, Result, ensure};
use glam::{Quat, Vec4};
use mhf_resource::{
    monster::{BodyVolume, MotionBinding, Tables},
    motion::MotionArchive,
    pac,
    rig::{Rig, rotation},
};
use mhgu_resource::{
    arc::{self, Asset},
    bdd, emc, focus, graft,
    lmt::{Track, Usage, Writer},
};
use std::{collections::BTreeMap, path::Path};

const E: &str = "em001_00";
const MODEL: &str = r"enemy\em001\em001_00\mod\em001_00";
const MOT: &str = r"enemy\em001\em001_00\mot\em001_00_0";
const HDE: u32 = 0x70bb64ba;
const HTS: u32 = 0x5653c1b0;
const DTT: u32 = 0x48e8ac29;
const BDD: u32 = 0x1ac7b52d;

fn u32_at(b: &[u8], p: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(b.get(p..p + 4).context("truncated table")?.try_into()?) as usize)
}
fn put(b: &mut [u8], p: usize, v: usize) {
    b[p..p + 4].copy_from_slice(&(v as u32).to_le_bytes());
}

fn selector(b: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        b.starts_with(b"EMC\0") && u32_at(b, 8)? == 21,
        "not a native GU command table"
    );
    let mut s = emc::Scripts::new();
    for g in 0..21 {
        let count = u32_at(b, 16 + g * 16)?;
        let table = u32_at(b, 20 + g * 16)?;
        for i in 0..count {
            let len = u32_at(b, table + i * 16)?;
            let pos = u32_at(b, table + i * 16 + 4)?;
            let data = if g == 0 {
                vec![0, 1, 0, emc::END]
            } else {
                b.get(pos..pos + len)
                    .context("invalid EMC script")?
                    .to_vec()
            };
            s.insert((g as u16, i.try_into()?), data);
        }
    }
    emc::encode(&s)
}

fn bake_phases(
    a: &MotionArchive<'_>,
    rig: &Rig,
    ids: &[u8],
    phases: &[MotionBinding],
) -> Result<(u32, Vec<Track>, Vec<u32>)> {
    let mut result = BTreeMap::<(u8, u8), Track>::new();
    let mut travel = Vec4::ZERO;
    let mut duration = 0;
    let mut boundaries = vec![0];
    for phase in phases {
        let (base, slot) = (phase.clip / 100, phase.clip % 100);
        let mut parts = BTreeMap::new();
        let mut frames = None;
        for group in [base, base + 2, base + 4] {
            let Some(m) = a.motion(group, slot)? else {
                ensure!(group != base, "missing Espinas base motion {group}:{slot}");
                continue;
            };
            let (f, tr) = motion::bake(&m, rig, None, None)?;
            ensure!(
                frames.replace(f).is_none_or(|n| n == f),
                "tagged poses have different durations"
            );
            for t in tr {
                if t.bone != 255 {
                    ensure!(
                        parts.insert((t.bone, t.usage as u8), t.samples).is_none(),
                        "duplicate bound track"
                    );
                }
            }
        }
        let f = frames.unwrap();
        duration += f;
        boundaries.push(duration);
        for (i, b) in rig.joints.iter().enumerate() {
            for (usage, rest) in [
                (Usage::Translation, b.pos.extend(1.)),
                (
                    Usage::Rotation,
                    Vec4::from_array(rotation(b.angles).to_array()),
                ),
                (Usage::Scale, b.scale.extend(1.)),
            ] {
                let mut samples = parts
                    .remove(&(i as u8, usage as u8))
                    .unwrap_or_else(|| vec![rest]);
                if samples.len() == 1 {
                    samples.resize(f as usize, samples[0]);
                }
                if i == 1 && usage == Usage::Translation {
                    let origin = samples[0];
                    let root = result.entry((255, 4)).or_insert_with(|| Track {
                        bone: 255,
                        usage: Usage::RootTranslation,
                        samples: vec![],
                    });
                    for p in &mut samples {
                        root.samples
                            .push(travel + Vec4::new(p.x - origin.x, 0., p.z - origin.z, 1.));
                        p.x = b.pos.x;
                        p.z = b.pos.z;
                    }
                    travel = *root.samples.last().unwrap();
                    travel.w = 0.;
                }
                let tr = result
                    .entry((ids[i], usage as u8))
                    .or_insert_with(|| Track {
                        bone: ids[i],
                        usage,
                        samples: vec![],
                    });
                if usage == Usage::Rotation
                    && tr.samples.last().is_some_and(|p| p.dot(samples[0]) < 0.)
                {
                    for p in &mut samples {
                        *p = -*p;
                    }
                }
                if let Some(&last) = tr.samples.last() {
                    for (step, p) in samples
                        .iter_mut()
                        .take(phase.blend_steps.saturating_sub(1))
                        .enumerate()
                    {
                        let weight = (step + 1) as f32 / phase.blend_steps as f32;
                        *p = if usage == Usage::Rotation {
                            let q = Quat::from_array(last.to_array())
                                .slerp(Quat::from_array(p.to_array()), weight);
                            Vec4::from_array(q.to_array())
                        } else {
                            last.lerp(*p, weight)
                        };
                    }
                }
                tr.samples.extend(samples);
            }
        }
    }
    let mut tr: Vec<_> = result.into_values().collect();
    tr.sort_by_key(|t| (t.bone != 255, t.bone, t.usage as u8));
    for t in &mut tr {
        if t.samples.iter().all(|p| *p == t.samples[0]) {
            t.samples.truncate(1);
        }
    }
    Ok((duration, tr, boundaries))
}

// The default calm requests run a full-body idle on pose group 0 and replace
// only one tagged pose. Completion follows that pose, not the idle clip (81CC6D7C).
fn layered(
    a: &MotionArchive<'_>,
    rig: &Rig,
    ids: &[u8],
    base: &MotionBinding,
    overlay: usize,
    blend: usize,
) -> Result<(u32, Vec<Track>, Vec<u32>)> {
    let (base_len, mut tracks, _) = bake_phases(a, rig, ids, std::slice::from_ref(base))?;
    let source = a
        .motion_by_native_id(overlay)?
        .context("missing tagged pose")?;
    let (len, pose) = motion::bake(&source, rig, None, None)?;
    let joints = rig.bind(source.tracks.len(), None, None)?;
    for track in &mut tracks {
        let sub = joints.iter().copied().find(|&i| ids[i] == track.bone);
        if let Some(i) = sub {
            let joint = &rig.joints[i];
            let other = pose
                .iter()
                .find(|t| t.bone == i as u8 && t.usage == track.usage);
            let rest = match track.usage {
                Usage::Rotation => Vec4::from_array(rotation(joint.angles).to_array()),
                Usage::Translation => joint.pos.extend(1.),
                Usage::Scale => joint.scale.extend(1.),
                Usage::RootTranslation => unreachable!(),
            };
            let start = track.samples[0];
            track.samples.clear();
            for t in 0..len as usize {
                let dst = other.map_or(rest, |p| {
                    p.samples[if p.samples.len() == 1 { 0 } else { t }]
                });
                // 81695BFA halves the explicit blend; 817315F8 uses 1/(n+1).
                let w = ((t + 1) as f32 / (1 + blend / 2) as f32).min(1.);
                let v = if w == 1. {
                    dst
                } else if track.usage == Usage::Rotation {
                    Vec4::from_array(
                        Quat::from_array(start.to_array())
                            .slerp(Quat::from_array(dst.to_array()), w)
                            .to_array(),
                    )
                } else {
                    start.lerp(dst, w)
                };
                track.samples.push(v);
            }
        } else if track.samples.len() > 1 {
            let base = std::mem::take(&mut track.samples);
            track.samples = (0..len as usize)
                .map(|t| base[t % ((base_len - 1).max(1) as usize)])
                .collect();
        }
        if track.samples.iter().all(|v| *v == track.samples[0]) {
            track.samples.truncate(1);
        }
    }
    Ok((len, tracks, vec![0, len]))
}

/// Source markers from the ordinary (non-HC) 107 and 118 branches of 81CD8FFE.
/// The single-cycle test joins the phases at frame 75; events remain relative to
/// the phase that creates them. FZ cancels a kind-1 effect when its motion changes.
pub struct Action {
    pub motions: &'static [u8],
    // (phase, source motion-frame marker, kind-1 effect ID).
    events: &'static [(usize, u16, u16)],
}
pub const CHARGE: Action = Action {
    motions: &[107, 118],
    events: &[
        (0, 26, 103),
        (0, 26, 74),
        (1, 2, 82),
        (1, 16, 101),
        (1, 20, 78),
    ],
};

/// 81CCE8E2 dispatch and 81CD8FFE frame events for ordinary species 80.
/// Each of these single-clip handlers returns to think at motion end.
pub fn replay(sequence: &[(u8, u8)]) -> Result<Action> {
    Ok(match sequence {
        [(3, 2)] => CHARGE,
        [(3, 8)] => anyhow::bail!(
            "FZ (3,8) also spawns effect 54 at frame 70: fixed world position, radius 400 (joint 127); a bone-only replay would omit it"
        ),
        [(3, 9)] => Action {
            motions: &[104],
            events: &[(0, 10, 69)],
        },
        [(3, 12)] => Action {
            motions: &[106],
            events: &[(0, 87, 71), (0, 116, 71)],
        },
        [(3, 15)] => Action {
            motions: &[117],
            events: &[
                (0, 36, 81),
                (0, 66, 80),
                (0, 70, 79),
                (0, 78, 83),
                (0, 92, 81),
            ],
        },
        _ => anyhow::bail!(
            "no complete motion and melee binding for the selected FZ request sequence {sequence:?}"
        ),
    })
}

fn melee(
    tables: &Tables<'_>,
    ids: &[u8],
    hz: &[u8],
    atk: &[u8],
    events: &[(u16, u16, u16)],
) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        hz.starts_with(b"HTS\0") && atk.starts_with(b"HDE\0") && u32_at(atk, 8)? == 64,
        "expected em001_00 attack data"
    );
    let n = u32_at(hz, 8)?;
    let mut groups = Vec::new();
    for i in 0..n {
        let len = u32_at(hz, 16 + i * 8)?;
        let off = u32_at(hz, 20 + i * 8)?;
        groups.push(
            hz.get(off..off + 48 * len)
                .context("short native HTS group")?
                .to_vec(),
        );
    }
    let empty = u8::try_from(
        groups
            .iter()
            .position(Vec::is_empty)
            .context("missing empty native hit group")?,
    )?;
    let mut dst = atk.to_vec();
    let attack_count = 63 + events.len();
    dst.resize(16 + 32 * attack_count, 0);
    let mut cut_groups = Vec::new();
    let mut mapped_groups = BTreeMap::new();
    for i in [8, 50] {
        dst[16 + 32 * i + 25] = empty;
    }
    for (index, &(effect, frame, end)) in events.iter().enumerate() {
        let slot = if index == 0 { 19 } else { 63 + index };
        let a = tables.melee(effect)?;
        let r = a.record;
        let shape_group = |severed: bool| -> Result<Vec<u8>> {
            let mut bytes = vec![];
            for v in &a.volumes {
                if severed && v.disabled_by & 1 != 0 {
                    continue;
                }
                ensure!(
                    v.disabled_by <= 1 && v.flags == 0,
                    "unmapped melee contact flags"
                );
                for h in [
                    if v.joint == 127 {
                        255
                    } else {
                        u16::from(ids[usize::from(v.joint)])
                    },
                    0,
                    v.shape,
                    0,
                ] {
                    bytes.extend(h.to_le_bytes());
                }
                for f in [
                    0., v.radius, v.start[0], v.start[1], v.start[2], 0., v.end[0], v.end[1],
                    v.end[2], 0.,
                ] {
                    bytes.extend(f.to_le_bytes());
                }
            }
            Ok(bytes)
        };
        // FZ reuses one geometry group from multiple effects and motions.
        // GU stores group IDs in one byte; only distinct geometries need slots.
        let (group, cut) = *match mapped_groups.entry(a.group) {
            std::collections::btree_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::btree_map::Entry::Vacant(v) => {
                let group = u8::try_from(groups.len())?;
                groups.push(shape_group(false)?);
                let cut = if a.volumes.iter().any(|v| v.disabled_by != 0) {
                    let cut = u8::try_from(groups.len())?;
                    groups.push(shape_group(true)?);
                    cut
                } else {
                    group
                };
                v.insert((group, cut))
            }
        };
        cut_groups.push(cut);
        let source_status = u32::from_le_bytes(r[20..24].try_into()?);
        ensure!(
            matches!(source_status, 0 | 2),
            "unmapped FZ melee status {source_status:#x}"
        );
        // Rathian physical/poison contact policy; transplant the FZ timing, base
        // power, reaction, guard strength and poison amount, not unknown bitfields.
        let native = if source_status == 2 { 7 } else { 1 };
        let row = &mut dst[16 + 32 * slot..48 + 32 * slot];
        row.copy_from_slice(&atk[16 + 32 * native..48 + 32 * native]);
        let delay = u16::from_le_bytes(r[0..2].try_into()?);
        let duration = u16::from_le_bytes(r[2..4].try_into()?);
        let start = frame + delay;
        row[0..2].copy_from_slice(&start.to_le_bytes());
        row[2..4].copy_from_slice(&duration.min(end - start).to_le_bytes());
        row[4..8].copy_from_slice(&r[4..8]);
        row[11] = r[14];
        row[16..20].copy_from_slice(&source_status.to_le_bytes());
        row[25] = group;
        row[29] = u8::try_from(u32::from_le_bytes(r[24..28].try_into()?))?;
    }
    // Native HDE loads counted 32-byte rows. These last rows are never armed.
    // They let the companion plugin recover the tail variant for every contact,
    // including the primary slot, without hardcoding the move or the donor's group IDs.
    let cut_start = dst.len();
    dst.extend(&cut_groups);
    dst.resize(cut_start + 32 * events.len().div_ceil(32), 0);
    let footer = dst.len();
    dst.resize(footer + 32, 0);
    dst[footer..footer + 4].copy_from_slice(b"ESP1");
    put(&mut dst, footer + 4, events.len());
    let total = (dst.len() - 16) / 32;
    put(&mut dst, 8, total);
    let mut h = hz[..16].to_vec();
    put(&mut h, 8, groups.len());
    h.resize(16 + 8 * groups.len(), 0);
    for (i, g) in groups.into_iter().enumerate() {
        let off = h.len();
        put(&mut h, 16 + i * 8, g.len() / 48);
        put(&mut h, 20 + i * 8, off);
        h.extend(g);
    }
    Ok((h, dst))
}

fn body(donor: &[u8], source: Vec<BodyVolume>, ids: &[u8]) -> Result<Vec<u8>> {
    let mut gu = bdd::Body::read(donor)?;
    let native = std::mem::take(&mut gu.volumes);
    for fz in source {
        // These three actor-root utility spheres aren't ordinary damage volumes. Keep
        // Rathian's native actor-level volume below, not three extra hittable body parts.
        if fz.joint == 1 && matches!(fz.flags, 5 | 0x300 | 0x1800) {
            continue;
        }
        // Keep GU contact flags from the same anatomical part, preferring the same
        // hitzone as well. FZ packs its contact flags differently from GU.
        let policy = native
            .iter()
            .filter(|v| v.part == fz.part && v.bone != 255)
            .min_by_key(|v| (v.hitzone != fz.hitzone, v.disabled_by != 0))
            .with_context(|| format!("no Rathian body part {}", fz.part))?;
        let bone = *ids
            .get(usize::from(fz.joint))
            .context("FZ body joint outside Espinas skeleton")?;
        gu.volumes.push(bdd::Volume {
            bone: u16::from(bone),
            other_bone: u16::MAX,
            shape: fz.shape,
            hitzone: fz.hitzone,
            part: fz.part,
            flags: policy.flags,
            disabled_by: fz.disabled_by,
            extra: policy.extra,
            radius: fz.radius,
            start: fz.start,
            end: fz.end,
        });
    }
    gu.volumes
        .extend(native.into_iter().filter(|v| v.bone == 255));
    gu.encode()
}

pub fn build(
    input: &Path,
    donor_path: &Path,
    template: &Path,
    vita_elf: &Path,
    action: &Action,
) -> Result<Vec<u8>> {
    build_mode(input, donor_path, template, vita_elf, Some(action))
}

pub(crate) fn hunt(
    input: &Path,
    donor_path: &Path,
    template: &Path,
    vita_elf: &Path,
) -> Result<Vec<u8>> {
    build_mode(input, donor_path, template, vita_elf, None)
}

fn build_mode(
    input: &Path,
    donor_path: &Path,
    template: &Path,
    vita_elf: &Path,
    action: Option<&Action>,
) -> Result<Vec<u8>> {
    let monster = Monster {
        pac: input.into(),
        model_member: 0,
    };
    let (source, pair, rig) = monster.open()?;
    ensure!(
        rig.joints.len() == 48 && rig.roots == [0, 45],
        "expected standard em080 skeleton"
    );
    let elf = read(vita_elf)?;
    let tables = Tables::espinas(&elf)?;
    let visual = package::build(&monster, template, E, 1, 2, 0)?;
    let mut replacements = BTreeMap::new();
    for e in arc::entries(&visual)? {
        if e.kind != arc::MOTION {
            replacements.insert((e.name.to_string(), e.kind), e.unpack()?);
        }
    }
    let raw = read(donor_path)?;
    let native = arc::entries(&raw)?;
    let get = |name: &str, kind: u32| -> Result<Vec<u8>> {
        native
            .iter()
            .find(|e| e.kind == kind && e.name == name)
            .with_context(|| format!("missing donor resource {name}"))?
            .unpack()
    };
    let key = (MODEL.to_string(), arc::MODEL);
    let (model, ids) = if action.is_none() {
        let body = model::hunt_body(&pac::member(&pair, 0)?, &rig)?;
        let (mut b, ids) = graft::graft(&get(MODEL, arc::MODEL)?, &body, &[])?;
        let mesh = u32_at(&b, 48)?;
        let groups = u32_at(&b, 40)?;
        // GU Rathian: 101 = intact tip, 8 = cut surface. Mesh group ids are
        // names, not indices in the directory (the donor also uses 100+).
        put(&mut b, groups + 32, 101);
        put(&mut b, groups + 64, 8);
        for i in 0..usize::from(u16::from_le_bytes(b[8..10].try_into()?)) {
            let q = mesh + 48 * i + 4;
            let row = u32_at(&b, q)?;
            let mat = (row >> 12) & 4095;
            if mat == 11 {
                b[q - 4..q - 2].fill(0);
                let flags = u32_at(&b, q + 4)?;
                put(&mut b, q + 4, flags & !1);
            }
            let group = match mat {
                6 => 8,
                10 | 12 => 101,
                _ => 0,
            };
            put(&mut b, q, (row & !4095) | group);
        }
        (b, ids)
    } else {
        graft::graft(&get(MODEL, arc::MODEL)?, &replacements[&key], &[1])?
    };
    replacements.insert(key, model);
    let mot = pac::member(&source, 2)?;
    let a = MotionArchive::from_directory(&mot)?;
    let hn = r"enemy\hit_data\em001_00_hitsize";
    let an = r"enemy\hit_data\em001_00_attackdata";
    if let Some(action) = action {
        let mut writer = Writer::from_existing(get(MOT, arc::MOTION)?)?;
        let (frames, tracks, _) = bake_phases(&a, &rig, &ids, &[tables.motion(1)?])?;
        writer.clip_with_loop(1, frames, &tracks, Some(0))?;
        let phases = action
            .motions
            .iter()
            .map(|&id| tables.motion(id))
            .collect::<Result<Vec<_>>>()?;
        let (frames, tracks, boundaries) = bake_phases(&a, &rig, &ids, &phases)?;
        let mut events = vec![];
        for &(phase, marker, effect) in action.events {
            let base = u16::try_from(boundaries[phase])?;
            let end = u16::try_from(boundaries[phase + 1])?;
            ensure!(base + marker < end, "event outside source motion");
            events.push((effect, base + marker, end));
        }
        writer.clip(4, frames, &tracks)?;
        replacements.insert((MOT.into(), arc::MOTION), writer.finish());
        let (hits, attacks) = melee(&tables, &ids, &get(hn, HTS)?, &get(an, HDE)?, &events)?;
        replacements.insert((hn.into(), HTS), hits);
        replacements.insert((an.into(), HDE), attacks);
    } else {
        let tail = format!("{MODEL}_tail");
        replacements.insert(
            (tail.clone(), arc::MODEL),
            model::detached_tail(&pac::member(&pair, 0)?, &rig)?,
        );
        replacements.insert(
            (tail, arc::MATERIAL),
            replacements[&(MODEL.to_string(), arc::MATERIAL)].clone(),
        );
        let combat = mhf_resource::combat::Combat::espinas(&elf)?;
        let bank_name = MOT.strip_suffix("_0").unwrap().to_owned() + "_1";
        let mut bank = Writer::from_existing(get(&bank_name, arc::MOTION)?)?;
        let mut available = bank
            .empty_slots()
            .filter(|&i| i != 0)
            .collect::<Vec<_>>()
            .into_iter();
        let mut entries = Vec::new();
        let mut all_events = Vec::new();
        let mut runs = Vec::new();
        let mut bind = |g: u8, id: u8, action: Action, looped: bool| -> Result<()> {
            let slot = available.next().context("not enough empty LMT slots")?;
            let phases = action
                .motions
                .iter()
                .map(|&m| tables.motion(m))
                .collect::<Result<Vec<_>>>()?;
            let overlay = match (g, id) {
                (0, 41) => Some(527),
                (0, 42) => Some(326),
                (0, 5) => Some(224),
                (0, 7) => Some(226),
                (0, 8) => Some(427),
                (0, 11) => Some(228),
                (0, 13) => Some(220),
                _ => None,
            };
            let (frames, tracks, bounds) = match overlay {
                Some(clip) => layered(
                    &a,
                    &rig,
                    &ids,
                    &phases[0],
                    clip,
                    if matches!(id, 41 | 42) { 8 } else { 10 },
                )?,
                None => bake_phases(&a, &rig, &ids, &phases)?,
            };
            let mut loop_frame = looped.then_some(0);
            if matches!((g, id), (3, 1 | 2 | 18 | 19) | (1, 0) | (251, 10)) {
                // 81007FFE reads the compiled MOT loop flag and frame. Both
                // run handlers use source 107; its repeat starts at 24, not 0.
                let m = a
                    .motion_by_native_id(phases[0].clip)?
                    .context("missing charge motion")?;
                ensure!(m.metadata_present == 1, "charge has no authored loop");
                let f = f32::from_bits(m.metadata);
                ensure!(
                    f.fract() == 0. && (0.0..frames as f32).contains(&f),
                    "invalid charge loop"
                );
                loop_frame = Some(f as u32);
                let root = tracks
                    .iter()
                    .find(|t| t.usage == Usage::RootTranslation)
                    .context("missing charge travel")?;
                ensure!(
                    root.samples.len() == frames as usize,
                    "invalid charge root curve"
                );
                runs.push((
                    entries.len(),
                    root.samples.iter().map(|s| s.z).collect::<Vec<_>>(),
                    f as u16,
                ));
            }
            bank.clip_with_loop(slot, frames, &tracks, loop_frame)?;
            let mut row = [0u8; 32];
            row[0] = g;
            row[1] = id;
            row[2..4].copy_from_slice(&(256 + u16::try_from(slot)?).to_le_bytes());
            row[4..6].copy_from_slice(&u16::try_from(all_events.len())?.to_le_bytes());
            row[6] = action.events.len().try_into()?;
            row[7] = phases[0].blend_steps.saturating_sub(1).try_into()?;
            row[16..20].copy_from_slice(&frames.to_le_bytes());
            if g == 250 {
                let t = tables.charge_turn(id)?;
                for (i, x) in t.iter().enumerate() {
                    row[20 + 2 * i..22 + 2 * i].copy_from_slice(&x.to_le_bytes());
                }
            }
            for &(phase, marker, effect) in action.events {
                if effect == 54 {
                    let attack = tables.melee(effect)?;
                    let [sphere] = attack.volumes.as_slice() else {
                        anyhow::bail!("expected one stationary sphere")
                    };
                    ensure!(
                        sphere.joint == 127 && sphere.shape == 0 && sphere.start == [0.; 3],
                        "unmapped positioned effect"
                    );
                    row[24..28].copy_from_slice(&sphere.radius.to_le_bytes());
                    row[28] = u8::try_from(
                        all_events.len() - usize::from(u16::from_le_bytes(row[4..6].try_into()?))
                            + 1,
                    )?;
                }
                let frame = u16::try_from(bounds[phase])? + marker;
                let end = u16::try_from(bounds[phase + 1])?;
                ensure!(
                    frame < end,
                    "FZ request {g}:{id}, marker {marker} (effect {effect}) outside motion {:?}, bounds {bounds:?}",
                    action.motions
                );
                all_events.push((effect, frame, end));
            }
            entries.push(row);
            Ok(())
        };
        for m in [39, 34, 37, 62] {
            let motions: &'static [u8] = match m {
                39 => &[39],
                34 => &[34],
                37 => &[37],
                _ => &[62],
            };
            bind(
                255,
                m,
                Action {
                    motions,
                    events: &[],
                },
                m == 34,
            )?;
        }
        for id in [41, 42] {
            bind(
                0,
                id,
                Action {
                    motions: &[34],
                    events: &[],
                },
                false,
            )?;
        }
        for id in [5, 7, 8, 11, 13] {
            bind(
                0,
                id,
                Action {
                    motions: &[1],
                    events: &[],
                },
                false,
            )?;
        }
        // 0:9 chooses 80 for an odd actor RNG counter, 81 for even.
        bind(
            0,
            9,
            Action {
                motions: &[81],
                events: &[],
            },
            false,
        )?;
        bind(
            254,
            9,
            Action {
                motions: &[80],
                events: &[],
            },
            false,
        )?;
        bind(
            3,
            11,
            Action {
                motions: &[105],
                events: &[
                    (0, 38, 98),
                    (0, 40, 99),
                    (0, 46, 70),
                    (0, 46, 97),
                    (0, 54, 100),
                ],
            },
            false,
        )?;
        bind(
            3,
            16,
            Action {
                motions: &[36],
                events: &[(0, 46, 41), (0, 50, 1)],
            },
            false,
        )?;
        bind(
            3,
            0,
            Action {
                motions: &[111],
                events: &[
                    (0, 2, 66),
                    (0, 20, 67),
                    (0, 100, 66),
                    (0, 120, 68),
                    (0, 180, 68),
                ],
            },
            false,
        )?;
        // 81CD0B88: after 125 ends, the ordinary handler plays the whole
        // 111 recovery, which has its own contacts. Do not collapse it to idle.
        bind(
            3,
            4,
            Action {
                motions: &[125, 111],
                events: &[
                    (0, 66, 88),
                    (0, 66, 91),
                    (0, 122, 89),
                    (0, 122, 90),
                    (0, 122, 91),
                    (0, 200, 102),
                    (0, 210, 91),
                    (1, 2, 66),
                    (1, 20, 67),
                    (1, 100, 66),
                    (1, 120, 68),
                    (1, 180, 68),
                ],
            },
            false,
        )?;
        // 81CD3DF2 / table 826B92D4: the short correction, left and right
        // are different native clips, each with its own frame-event branch.
        bind(
            1,
            1,
            Action {
                motions: &[136],
                events: &[],
            },
            false,
        )?;
        bind(
            253,
            5,
            Action {
                motions: &[5],
                events: &[(0, 2, 58), (0, 2, 52), (0, 32, 59)],
            },
            false,
        )?;
        bind(
            253,
            6,
            Action {
                motions: &[6],
                events: &[(0, 2, 61), (0, 2, 52), (0, 32, 60)],
            },
            false,
        )?;
        // 1:11 selects a zone node and dispatches the bound turn and walk; it has no motion of its own.
        bind(
            1,
            0,
            Action {
                motions: &[3],
                events: &[(0, 2, 55), (0, 2, 51), (0, 52, 56), (0, 52, 51)],
            },
            true,
        )?;
        bind(
            251,
            10,
            Action {
                motions: &[10],
                events: &[(0, 2, 26), (0, 42, 25), (0, 74, 26)],
            },
            true,
        )?;
        bind(
            252,
            1,
            Action {
                motions: &[1],
                events: &[],
            },
            false,
        )?;
        for (id, motion) in [(13, &[43][..]), (14, &[42][..])] {
            bind(
                3,
                id,
                Action {
                    motions: motion,
                    events: &[(0, 30, 104)],
                },
                false,
            )?;
        }
        for (id, motion, e) in [
            (119, &[119][..], &[(0, 2, 85), (0, 2, 87)][..]),
            (120, &[120], &[(0, 2, 85), (0, 2, 87)]),
            (122, &[122], &[(0, 2, 85), (0, 2, 87)]),
            (123, &[123], &[(0, 2, 84), (0, 2, 86)]),
        ] {
            bind(
                250,
                id,
                Action {
                    motions: motion,
                    events: e,
                },
                false,
            )?;
        }
        for id in [1, 2, 18, 19] {
            bind(
                3,
                id,
                Action {
                    motions: &[107],
                    events: &[(0, 26, 103), (0, 26, 74)],
                },
                true,
            )?;
        }
        bind(
            252,
            108,
            Action {
                motions: &[108],
                events: &[(0, 14, 75)],
            },
            false,
        )?;
        bind(
            252,
            118,
            Action {
                motions: &[118],
                events: &[(0, 2, 82), (0, 16, 101), (0, 20, 78)],
            },
            false,
        )?;
        bind(
            3,
            8,
            Action {
                motions: &[103],
                events: &[(0, 2, 53), (0, 70, 54)],
            },
            false,
        )?;
        bind(
            3,
            10,
            Action {
                motions: &[103, 104],
                events: &[(0, 2, 53), (0, 70, 54), (1, 10, 69)],
            },
            false,
        )?;
        bind(
            3,
            5,
            Action {
                motions: &[47],
                events: &[],
            },
            false,
        )?;
        for id in [6, 7] {
            bind(
                3,
                id,
                Action {
                    motions: &[114],
                    events: &[],
                },
                false,
            )?;
        }
        bind(
            252,
            115,
            Action {
                motions: &[115],
                events: &[],
            },
            false,
        )?;
        bind(
            252,
            116,
            Action {
                motions: &[116],
                events: &[],
            },
            false,
        )?;
        bind(
            3,
            17,
            Action {
                motions: &[52],
                events: &[],
            },
            false,
        )?;
        bind(
            252,
            57,
            Action {
                motions: &[57],
                events: &[(0, 2, 37), (0, 28, 29), (0, 36, 40)],
            },
            false,
        )?;
        bind(
            2,
            1,
            Action {
                motions: &[15],
                events: &[],
            },
            true,
        )?;
        bind(
            252,
            11,
            Action {
                motions: &[11],
                events: &[],
            },
            false,
        )?;
        bind(
            252,
            19,
            Action {
                motions: &[19],
                events: &[(0, 2, 13)],
            },
            false,
        )?;
        for id in [9, 12, 15] {
            bind(3, id, replay(&[(3, id)])?, false)?;
        }
        // GU 9B2BC -> CF43D8 -> A134C: grounded death 11:0 ends on
        // bank 3 slot 17. Keep its native completion/quest command.
        let reaction_bank = MOT.strip_suffix("_0").unwrap().to_owned() + "_3";
        let mut native_reactions = Writer::from_existing(get(&reaction_bank, arc::MOTION)?)?;
        for (slot, source) in [(17, 68), (10, 77), (11, 78), (12, 82)] {
            let (frames, tracks, _) = bake_phases(&a, &rig, &ids, &[tables.motion(source)?])?;
            let lp = if slot == 10 {
                let m = a
                    .motion_by_native_id(tables.motion(source)?.clip)?
                    .context("missing airborne fall")?;
                (m.metadata_present == 1)
                    .then(|| f32::from_bits(m.metadata) as u32)
                    .filter(|&x| x < frames)
            } else {
                None
            };
            native_reactions.clip_with_loop(slot, frames, &tracks, lp)?;
        }
        replacements.insert((reaction_bank, arc::MOTION), native_reactions.finish());
        // 4:0,1,2,3; 4:5/16 read the two triples at 826B9734.
        let reaction_bank = MOT.strip_suffix("_0").unwrap().to_owned() + "_4";
        let mut rx = Writer::from_existing(get(&reaction_bank, arc::MOTION)?)?;
        let slots = rx.empty_slots().filter(|&s| s != 0).collect::<Vec<_>>();
        for (slot, source) in slots
            .into_iter()
            .zip([60u8, 66, 63, 64, 69, 70, 71, 74, 75, 72])
        {
            let binding = tables.motion(source)?;
            let (frames, tracks, _) = bake_phases(&a, &rig, &ids, &[binding])?;
            let binding = tables.motion(source)?;
            let m = a
                .motion_by_native_id(binding.clip)?
                .context("missing flinch")?;
            let lp = if matches!(source, 70 | 75) && m.metadata_present == 1 {
                Some(f32::from_bits(m.metadata) as u32)
            } else {
                None
            };
            rx.clip_with_loop(slot, frames, &tracks, lp)?;
            let mut row = [0; 32];
            row[0] = 249;
            row[1] = source;
            row[2..4].copy_from_slice(&(1024 + slot as u16).to_le_bytes());
            row[7] = binding.blend_steps.saturating_sub(1) as u8;
            row[16..20].copy_from_slice(&frames.to_le_bytes());
            entries.push(row);
        }
        replacements.insert((reaction_bank, arc::MOTION), rx.finish());
        let (hits, mut hde) = melee(&tables, &ids, &get(hn, HTS)?, &get(an, HDE)?, &all_events)?;
        // Replace the replay footer; everything before it is still native HDE rows.
        hde.truncate(hde.len() - 32);
        for (index, samples, repeat) in runs {
            put(&mut entries[index], 8, (hde.len() - 16) / 32);
            entries[index][12..14].copy_from_slice(&u16::try_from(samples.len())?.to_le_bytes());
            entries[index][14..16].copy_from_slice(&repeat.to_le_bytes());
            for z in samples {
                hde.extend(z.to_le_bytes());
            }
            hde.resize(16 + (hde.len() - 16).next_multiple_of(32), 0);
        }
        let (fire, speed, radius, offset) = tables.fireball()?;
        for entry in &mut entries {
            let (g, id) = (entry[0], entry[1]);
            let flight = match (g, id) {
                (3, 17) => Some(14),
                (252, 116) => Some(17),
                _ => None,
            };
            if let Some(kind) = flight {
                let curves = tables.flight_curves(kind)?;
                for (i, c) in curves.into_iter().enumerate() {
                    put(entry, 8 + 4 * i, (hde.len() - 16) / 32);
                    hde.extend((c.len() as u32).to_le_bytes());
                    for (frame, val) in c {
                        hde.extend(frame.to_le_bytes());
                        hde.extend(val.to_le_bytes());
                    }
                    hde.resize(16 + (hde.len() - 16).next_multiple_of(32), 0);
                }
            }
        }
        let sh = r"shell\em\em001_00_shell00\em001_00_00";
        let n = format!("{sh}_hitdata");
        let k = 0x4eb68cee;
        let mut shell = get(&n, k)?;
        ensure!(
            shell.starts_with(b"HDS\0") && shell.len() == 128,
            "unexpected Rathian shell HDS"
        );
        // The first HDS row is the moving fireball. Leave the explosion a
        // separate resource, so the native impact can register it independently.
        shell[16..24].copy_from_slice(&fire[..8]);
        shell[27] = fire[14];
        shell[32..36].copy_from_slice(&fire[20..24]);
        shell[45] = u8::try_from(u32::from_le_bytes(fire[24..28].try_into()?))?;
        replacements.insert((n, k), shell);
        let n = format!("{sh}_hitsize");
        let mut shapes = get(&n, HTS)?;
        let pos = u32_at(&shapes, 20)?;
        shapes[pos + 12..pos + 16].copy_from_slice(&radius.to_le_bytes());
        replacements.insert((n, HTS), shapes);
        let directory = (hde.len() - 16) / 32;
        for entry in &entries {
            hde.extend(entry);
        }
        // Ordinary low/high rank profile, 8189604A and 816A09AC. GU has
        // no matching Frontier event/HC rank; do not load those variant rows.
        let emd_path = input
            .parent()
            .and_then(Path::parent)
            .context("expected dat/emmodel PAC")?
            .join("mhfemd.bin");
        let emd = pac::decode(read(&emd_path)?)?;
        let species = u32_at(&emd, 12)? + 192 * 80;
        let zones = u32_at(&emd, species + 12)?;
        let anger = u32_at(&emd, species + 72)?;
        let physiology = (hde.len() - 16) / 32;
        hde.extend(
            emd.get(anger..anger + 60)
                .context("missing FZ anger profile")?,
        );
        for z in emd
            .get(zones..zones + 140)
            .context("missing FZ body hitzones")?
            .as_chunks::<10>()
            .0
        {
            // FZ 816A09AC: physical columns begin at +1; GU BAB60 at +0.
            // GU's additional mount accumulation is not an FZ hitzone.
            hde.extend(&z[1..10]);
            hde.push(0);
        }
        hde.resize(16 + (hde.len() - 16).next_multiple_of(32), 0);
        let params = (hde.len() - 16) / 32;
        let mut header = [0; 32];
        for (i, d) in combat.distances.iter().enumerate() {
            header[4 * i..4 * i + 4].copy_from_slice(&d.to_le_bytes());
        }
        header[16..20].copy_from_slice(&speed.to_le_bytes());
        header[20..24].copy_from_slice(&offset[1].to_le_bytes());
        header[24..28].copy_from_slice(&offset[2].to_le_bytes());
        header[28] = ids[34];
        header[30..32].copy_from_slice(&u16::try_from(physiology)?.to_le_bytes());
        ensure!(offset[0] == 0., "unexpected lateral projectile origin");
        hde.extend(header);
        let mut routes: Vec<[u8; espinas_behavior::ROUTE_WIDTH]> = vec![];
        let mut missing = std::collections::BTreeSet::new();
        for rage in [false, true] {
            for air in [false, true] {
                for lane in 0..5 {
                    for turned in [false, true] {
                        for random in 0..32 {
                            let d = if lane == 0 {
                                0.
                            } else {
                                combat.distances[lane - 1].next_up()
                            };
                            let seq = combat.requests(&mhf_resource::combat::Target {
                                enraged: rage,
                                airborne: air,
                                distance: Some(d),
                                angle: if turned { 32768 } else { 0 },
                                random,
                            })?;
                            let mut row = [0; espinas_behavior::ROUTE_WIDTH];
                            ensure!(
                                2 * seq.len() < row.len(),
                                "source sequence does not fit decision row"
                            );
                            row[0] = seq.len().try_into()?;
                            for (i, (g, id)) in seq.into_iter().enumerate() {
                                row[1 + 2 * i] = g;
                                row[2 + 2 * i] = id;
                                let bound = |g, id| entries.iter().any(|e| e[0] == g && e[1] == id);
                                if !(bound(g, id)
                                    || ((g, id) == (1, 11) && bound(1, 0) && bound(1, 1)))
                                {
                                    missing.insert((g, id));
                                }
                            }
                            let idx = routes.iter().position(|r| r == &row).unwrap_or_else(|| {
                                routes.push(row);
                                routes.len() - 1
                            });
                            hde.push(idx.try_into()?);
                        }
                    }
                }
            }
        }
        ensure!(
            missing.is_empty(),
            "full hunt unavailable: missing FZ handlers {missing:?}; no release ARC written"
        );
        let route_row = (hde.len() - 16) / 32;
        for r in &routes {
            hde.extend(r);
        }
        let padding = (32 - (hde.len() - 16) % 32) % 32;
        hde.resize(hde.len() + padding, 0);
        let footer = hde.len();
        hde.resize(footer + 32, 0);
        hde[footer..footer + 4].copy_from_slice(b"ESH5");
        for (i, value) in [
            all_events.len(),
            63 + all_events.len(),
            directory,
            entries.len(),
            params,
            route_row,
            routes.len(),
        ]
        .into_iter()
        .enumerate()
        {
            put(&mut hde, footer + 4 + 4 * i, value);
        }
        let total = (hde.len() - 16) / 32;
        put(&mut hde, 8, total);
        replacements.insert((an.into(), HDE), hde);
        replacements.insert((hn.into(), HTS), hits);
        replacements.insert((bank_name, arc::MOTION), bank.finish());
        let mut base = Writer::from_existing(get(MOT, arc::MOTION)?)?;
        // The donor can request its neutral pose before the first AI callback.
        let (frames, tracks, _) = bake_phases(&a, &rig, &ids, &[tables.motion(34)?])?;
        base.clip_with_loop(1, frames, &tracks, Some(0))?;
        replacements.insert((MOT.into(), arc::MOTION), base.finish());
    }
    let bn = r"enemy\body_data\em001_00_bodydata";
    replacements.insert(
        (bn.into(), BDD),
        body(&get(bn, BDD)?, tables.body()?, &ids)?,
    );
    let dn = r"enemy\dt_tune\em001_00_dttune";
    replacements.insert(
        (dn.into(), DTT),
        focus::rathian(get(dn, DTT)?, ids[3], ids[30])?,
    );
    let cam = r"enemy\cam_body_data\em001_00_cambodydata";
    // Same surface the camera can actually see: torso, neck, skull, legs and tail,
    // excluding wing membranes and the special actor-level damage helper.
    let mut camera = bdd::Body::read(&get(cam, BDD)?)?;
    camera.volumes = bdd::Body::read(&replacements[&(bn.into(), BDD)])?
        .volumes
        .into_iter()
        .filter(|v| v.bone != 255 && !matches!(v.part, 1 | 2))
        .map(|v| bdd::Volume {
            flags: 0,
            extra: 0,
            hitzone: 0,
            part: 0,
            ..v
        })
        .collect();
    replacements.insert((cam.into(), BDD), camera.encode()?);
    let cn = r"enemy\cmd_tbl\em001_00_cmdtbl";
    replacements.insert(
        (cn.into(), arc::COMMAND),
        selector(&get(cn, arc::COMMAND)?)?,
    );
    let mut assets = vec![];
    for e in native {
        let key = (e.name.to_string(), e.kind);
        let data = match replacements.remove(&key) {
            Some(b) => b,
            None => e.unpack()?,
        };
        assets.push(Asset {
            name: key.0,
            kind: key.1,
            data,
        });
    }
    // Native monster archives place textures before the material that requests them.
    let mut ordered: Vec<_> = replacements
        .into_iter()
        .map(|((name, kind), data)| Asset { name, kind, data })
        .collect();
    ordered.append(&mut assets);
    arc::encode(&ordered)
}

/// Standalone, independently addressable PAC clips for the same grafted skeleton
/// as the trial. Do not alias Frontier request IDs to MHGU action numbers.
pub fn motion_bank(pac_path: &Path, donor_path: &Path) -> Result<Vec<u8>> {
    let monster = Monster {
        pac: pac_path.into(),
        model_member: 0,
    };
    let (src, pair, rig) = monster.open()?;
    ensure!(
        rig.roots == [0, 45] && rig.joints.len() == 48,
        "expected ordinary Espinas skeleton"
    );
    let donor = read(donor_path)?;
    let native = arc::entries(&donor)?
        .into_iter()
        .find(|e| e.kind == arc::MODEL && e.name == MODEL)
        .context("missing Rathian donor MOD")?
        .unpack()?;
    let (_, ids) = graft::graft(
        &native,
        &model::convert(&pac::member(&pair, 0)?, &rig)?,
        &[1],
    )?;
    let motions = pac::member(&src, 2)?;
    let archive = MotionArchive::from_directory(&motions)?;
    let mut writer = Writer::new(200)?;
    let mut count = 0;
    let body_tracks = rig.motion_nodes(rig.roots[0], Some(0)).count();
    let mut other = vec![];
    for clip in 0..200 {
        let Some(base) = archive.motion(clip / 100, clip % 100)? else {
            continue;
        };
        if base.tracks.len() != body_tracks {
            other.push(clip);
            continue;
        }
        let phase = MotionBinding {
            clip,
            blend_steps: 0,
        };
        let (frames, tracks, _) = bake_phases(&archive, &rig, &ids, &[phase])
            .with_context(|| format!("Espinas PAC clip {clip}"))?;
        writer.clip(clip, frames, &tracks)?;
        count += 1;
    }
    println!(
        "{count} full-body clips on the em001_00 graft; slots with a different base track layout: {other:?}"
    );
    Ok(writer.finish())
}
