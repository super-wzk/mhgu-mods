//! Convert FZ body, melee and fireball contacts to GU collision resources.
use super::{put, u32_at};
use anyhow::{Context, Result, ensure};
use mhf_resource::monster::{BodyVolume, Fireball, Tables};
use mhgu_resource::{
    bdd,
    hts::{Shape, ShapeGroups},
};
use std::collections::BTreeMap;

// GU reserves the first 64 melee slots (19 is the primary). Private hunts
// start them disabled; only the generated FZ events below receive geometry.
pub(super) fn hunt_melee(
    tables: &Tables<'_>,
    ids: &[u8],
    events: &[(u16, u16, u16)],
) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut hde = vec![0; 16 + 32 * 64];
    hde[..8].copy_from_slice(b"HDE\0\x21\x01\x16\x20");
    // GU physical and poison contact responses; per-event amounts come from FZ.
    for (slot, severity, response) in [(1, 70, 2), (7, 50, 5)] {
        let row = &mut hde[16 + 32 * slot..48 + 32 * slot];
        row[12] = 1;
        row[24] = severity;
        row[26] = 2;
        row[28] = 6;
        row[31] = response;
    }
    encode_melee(tables, ids, events, ShapeGroups::new(vec![vec![]]), hde)
}

pub(super) fn replay_melee(
    tables: &Tables<'_>,
    ids: &[u8],
    shapes: &[u8],
    attacks: Vec<u8>,
    events: &[(u16, u16, u16)],
) -> Result<(Vec<u8>, Vec<u8>)> {
    ensure!(
        attacks.starts_with(b"HDE\0") && u32_at(&attacks, 8)? == 64,
        "expected em001_00 attack data"
    );
    let (shapes, mut attacks) =
        encode_melee(tables, ids, events, ShapeGroups::read(shapes)?, attacks)?;
    let footer = attacks.len();
    attacks.resize(footer + 32, 0);
    attacks[footer..footer + 4].copy_from_slice(b"ESP1");
    put(&mut attacks, footer + 4, events.len());
    let count = (attacks.len() - 16) / 32;
    put(&mut attacks, 8, count);
    Ok((shapes, attacks))
}

fn encode_melee(
    tables: &Tables<'_>,
    ids: &[u8],
    events: &[(u16, u16, u16)],
    mut shapes: ShapeGroups,
    mut dst: Vec<u8>,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let groups = &mut shapes.groups;
    let empty = u8::try_from(
        groups
            .iter()
            .position(Vec::is_empty)
            .context("missing empty native hit group")?,
    )?;
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
        // FZ reuses one geometry group from multiple effects and motions.
        // GU stores group IDs in one byte; only distinct geometries need slots.
        let (group, cut) = *match mapped_groups.entry(a.group) {
            std::collections::btree_map::Entry::Occupied(o) => o.into_mut(),
            std::collections::btree_map::Entry::Vacant(v) => {
                let group = u8::try_from(groups.len())?;
                groups.push(melee_shapes(&a.volumes, ids, false)?);
                let cut = if a.volumes.iter().any(|v| v.disabled_by != 0) {
                    let cut = u8::try_from(groups.len())?;
                    groups.push(melee_shapes(&a.volumes, ids, true)?);
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
        // Slots 1 and 7 supply the GU physical and poison policy respectively.
        let native = if source_status == 2 { 7 } else { 1 };
        let at = 16 + 32 * slot;
        dst.copy_within(16 + 32 * native..48 + 32 * native, at);
        let row = &mut dst[at..at + 32];
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
    Ok((shapes.encode()?, dst))
}

fn melee_shapes(volumes: &[BodyVolume], ids: &[u8], severed: bool) -> Result<Vec<Shape>> {
    let mut shapes = vec![];
    for v in volumes {
        if severed && v.disabled_by & 1 != 0 {
            continue;
        }
        ensure!(
            v.disabled_by <= 1 && v.flags == 0,
            "unmapped melee contact flags"
        );
        let bone = if v.joint == 127 {
            255
        } else {
            u16::from(ids[usize::from(v.joint)])
        };
        let mut shape = [0; 48];
        shape[..2].copy_from_slice(&bone.to_le_bytes());
        shape[4..6].copy_from_slice(&v.shape.to_le_bytes());
        shape[12..16].copy_from_slice(&v.radius.to_le_bytes());
        for (at, point) in [(16, v.start), (32, v.end)] {
            for (component, value) in shape[at..at + 12]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(point)
            {
                *component = value.to_le_bytes();
            }
        }
        shapes.push(shape);
    }
    Ok(shapes)
}

pub(super) fn replay_body(donor: &[u8], source: Vec<BodyVolume>, ids: &[u8]) -> Result<bdd::Body> {
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
        gu.volumes
            .push(volume(fz, u16::from(bone), policy.flags, policy.extra));
    }
    gu.volumes
        .extend(native.into_iter().filter(|v| v.bone == 255));
    Ok(gu)
}

pub(super) fn hunt_body(source: Vec<BodyVolume>, ids: &[u8]) -> Result<bdd::Body> {
    let mut volumes = Vec::new();
    for fz in source {
        let utility = fz.joint == 1;
        // FZ has three root utility spheres. GU wants one actor contact.
        if utility && fz.flags != 5 {
            continue;
        }
        let (bone, flags) = if utility {
            (255, 0x12)
        } else {
            (
                u16::from(
                    *ids.get(usize::from(fz.joint))
                        .context("missing body bone")?,
                ),
                if fz.part == 6 { 0 } else { 0x200 },
            )
        };
        volumes.push(volume(fz, bone, flags, 0));
    }
    Ok(bdd::Body::new(volumes))
}

fn volume(fz: BodyVolume, bone: u16, flags: u16, extra: u16) -> bdd::Volume {
    bdd::Volume {
        bone,
        other_bone: u16::MAX,
        shape: fz.shape,
        hitzone: fz.hitzone,
        part: fz.part,
        flags,
        disabled_by: fz.disabled_by,
        extra,
        radius: fz.radius,
        start: fz.start,
        end: fz.end,
    }
}

pub(super) fn camera_body(body: bdd::Body) -> bdd::Body {
    // Only surfaces visible to the camera, excluding wing membranes and the actor helper.
    bdd::Body::new(
        body.volumes
            .into_iter()
            .filter(|v| v.bone != 255 && !matches!(v.part, 1 | 2))
            .map(|v| bdd::Volume {
                flags: 0,
                extra: 0,
                hitzone: 0,
                part: 0,
                ..v
            })
            .collect(),
    )
}

pub(super) fn fireball(fireball: &Fireball<'_>) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut shell = vec![0; 16 + 2 * 56];
    shell[..8].copy_from_slice(b"HDS\0\x09\x02\x16\x20");
    put(&mut shell, 8, 2);
    let mut groups = Vec::with_capacity(2);
    for (i, contact) in [&fireball.flight, &fireball.impact].into_iter().enumerate() {
        let fz = contact.record;
        let row = &mut shell[16 + 56 * i..16 + 56 * (i + 1)];
        row[..8].copy_from_slice(&fz[..8]); // delay, duration, power, reaction
        row[11] = fz[14];
        row[12] = fz[15];
        row[16..20].copy_from_slice(&fz[20..24]); // combined ailment mask
        row[25] = i as u8; // HTS group
        row[26] = 2;
        row[28] = 6;
        row[29] = u8::try_from(u32::from_le_bytes(fz[24..28].try_into()?))?;
        // GU directed shell: contact scale and flight/impact dispatch.
        row[40..44].copy_from_slice(&1f32.to_le_bytes());
        row[44] = 2 + i as u8;
        let mut sphere = [0; 48];
        sphere[..2].copy_from_slice(&1001u16.to_le_bytes()); // shell origin
        sphere[10..12].copy_from_slice(&0x0848u16.to_le_bytes());
        sphere[12..16].copy_from_slice(&contact.radius.to_le_bytes());
        groups.push(vec![sphere]);
    }
    Ok((shell, ShapeGroups::new(groups).encode()?))
}
