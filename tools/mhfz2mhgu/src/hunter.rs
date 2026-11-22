//! Frontier great sword w00: paired lower/upper MOT directories. Keep the three
//! namespaces separate; a source slot is not a GU action or hit-data number.
use super::{motion::bake, read};
use anyhow::{Context, Result, ensure};
use glam::{Quat, Vec4};
use mhf_resource::{motion::MotionArchive, pac, rig::Rig};
use mhgu_resource::{
    arc,
    lmt::{Track, Usage, Writer},
};
use std::path::Path;

// Source FSKL id -> GU logical bone ID, not the index in an armor part MOD.
// The first two FSKL nodes are the unanimated container and the motion carrier.
const BONES: [u8; 19] = [
    0, 14, 15, 16, 17, 18, 19, 1, 2, 5, 6, 7, 8, 9, 10, 11, 12, 3, 4,
];

fn sample(tracks: &[Track], bone: u8, usage: Usage, frame: usize, default: Vec4) -> Vec4 {
    tracks
        .iter()
        .find(|t| t.bone == bone && t.usage == usage)
        .map_or(default, |t| t.samples[frame.min(t.samples.len() - 1)])
}

fn merge(rig: &Rig, layers: [Vec<Track>; 2], frames: usize) -> Result<Vec<Track>> {
    let mut tracks: Vec<_> = layers
        .into_iter()
        .flatten()
        .filter(|t| t.bone != 255)
        .collect();
    ensure!(
        tracks
            .iter()
            .all(|t| t.bone != 0 && (t.bone != 1 || t.usage != Usage::Scale)),
        "unexpected animated scene root or carrier scale"
    );
    let mut root = Vec::new();
    let mut hips = Vec::new();
    let mut pos = Vec::new();
    let bind = Vec4::from_array(mhf_resource::rig::rotation(rig.joints[2].angles).to_array());
    for frame in 0..frames {
        let carrier =
            Quat::from_array(sample(&tracks, 1, Usage::Rotation, frame, Vec4::W).to_array());
        let rot = sample(&tracks, 2, Usage::Rotation, frame, bind);
        let offset = sample(
            &tracks,
            2,
            Usage::Translation,
            frame,
            rig.joints[2].pos.extend(1.),
        );
        let q = Vec4::from_array((carrier * Quat::from_array(rot.to_array())).to_array());
        hips.push(if hips.last().is_some_and(|p: &Vec4| p.dot(q) < 0.) {
            -q
        } else {
            q
        });
        pos.push((carrier * offset.truncate()).extend(1.));
        root.push(sample(&tracks, 1, Usage::Translation, frame, Vec4::W));
    }
    tracks.retain(|t| t.bone > 2 || t.bone == 2 && t.usage == Usage::Scale);
    for t in &mut tracks {
        t.bone = BONES[t.bone as usize - 2];
        if t.samples.len() > 1 {
            t.samples.resize(frames, *t.samples.last().unwrap());
        }
    }
    for (bone, usage, mut samples) in [
        (255, Usage::RootTranslation, root),
        (0, Usage::Translation, pos),
        (0, Usage::Rotation, hips),
    ] {
        if samples.iter().all(|v| *v == samples[0]) {
            samples.truncate(1);
        }
        tracks.push(Track {
            bone,
            usage,
            samples,
        });
    }
    Ok(tracks)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Weapon {
    GreatSword = 0,
    SwordShield = 4,
    DualBlades = 6,
    LongSword = 7,
    Gunlance = 9,
    SwitchAxe = 12,
}

pub fn motions(dat: &Path, female: bool, weapon: Weapon) -> Result<Vec<u8>> {
    let sex = if female { "f" } else { "m" };
    let (source, stem, slots) = match weapon {
        Weapon::GreatSword => ("w00", "frontier_gs", [100, 15, 80]),
        Weapon::SwordShield => ("w04", "frontier_sns", [100, 15, 65]),
        Weapon::DualBlades => ("w06", "frontier_db", [100, 15, 60]),
        Weapon::LongSword => ("w07", "frontier_ls", [100, 15, 60]),
        Weapon::Gunlance => ("w09", "frontier_gl", [100, 15, 85]),
        Weapon::SwitchAxe => ("w12", "frontier_sa", [70, 25, 100]),
    };
    let model = pac::decode(read(&dat.join(format!("parts/{sex}00/{sex}_editpl.bin")))?)?;
    let rig = Rig::parse(&pac::member(&model, 1)?)?;
    ensure!(
        rig.joints.len() == 21 && (0..21).all(|i| rig.by_id.get(&(i as u16)) == Some(&i)),
        "unrecognized hunter skeleton"
    );
    let b = pac::decode(read(&dat.join(format!("motion/{source}.mot")))?)?;
    let mot = MotionArchive::from_directory(&b)?;
    ensure!(
        mot.groups.iter().map(|g| g.motion_offsets.len()).eq([
            slots[0], slots[0], slots[1], slots[1], slots[2], slots[2], 0
        ]),
        "unrecognized Frontier {source} motion directory"
    );
    let instant_travel = if weapon == Weapon::LongSword {
        let data = pac::decode(read(&dat.join("mhfdat.bin"))?)?;
        let directory = u32::from_le_bytes(
            data.get(1836..1840)
                .context("missing Frontier parameter directory")?
                .try_into()?,
        ) as usize;
        let at = directory + 173 * 32 + 4;
        f32::from_le_bytes(
            data.get(at..at + 4)
                .context("missing instant travel")?
                .try_into()?,
        )
    } else {
        0.
    };
    let mut assets = Vec::new();
    for group in [0, 2, 4] {
        let slots = mot.groups[group].motion_offsets.len();
        ensure!(
            mot.groups[group + 1].motion_offsets.len() == slots,
            "mismatched hunter layers"
        );
        let stepping_slash = weapon == Weapon::LongSword && group == 4;
        let advancing_shield = weapon == Weapon::SwordShield && group == 4;
        let extra = if advancing_shield { 4 } else { usize::from(stepping_slash) };
        let mut w = Writer::new(slots + extra)?;
        for slot in 0..slots {
            let pair = (mot.motion(group, slot)?, mot.motion(group + 1, slot)?);
            let (lower, upper) = match pair {
                (None, None) => continue,
                (Some(a), Some(b)) => (a, b),
                _ => anyhow::bail!("unpaired {source} slot {group}:{slot}"),
            };
            let (lo, a) = bake(&lower, &rig, Some(0), Some(0))
                .with_context(|| format!("{source} lower {group}:{slot}"))?;
            let (hi, b) = bake(&upper, &rig, Some(1), Some(0))
                .with_context(|| format!("{source} upper {}:{slot}", group + 1))?;
            // FZ 8173208A plays the last sample of the Shining Sword loop
            // for one 30 Hz tick. GU 951A80 wraps as soon as it reaches the
            // end, so hold that pose for two motion frames in the LMT.
            let frames = lo.max(hi)
                + if weapon == Weapon::GreatSword && group == 4 && slot == 65 {
                    2
                } else {
                    0
                };
            let loop_frame = match (lower.metadata_present, upper.metadata_present) {
                (0, 0) => None,
                (1, 1) if lower.metadata == upper.metadata => {
                    let f = f32::from_bits(lower.metadata);
                    ensure!(
                        f.fract() == 0. && (0.0..frames as f32).contains(&f),
                        "invalid {source} loop point {group}:{slot}"
                    );
                    Some(f as u32)
                }
                _ => anyhow::bail!("unmatched hunter loop points at {group}:{slot}"),
            };
            let mut tracks = merge(&rig, [a, b], frames as usize)?;
            if weapon == Weapon::LongSword && group == 4 && slot == 51 {
                // FZ 811F9916 scales the horizontal root delta on 30 Hz ticks
                // ending in [28,46). Apply it to both GU samples of each tick.
                let root = tracks
                    .iter_mut()
                    .find(|t| t.usage == Usage::RootTranslation)
                    .context("instant slash lacks a root translation track")?;
                let mut extra = Vec4::ZERO;
                let mut previous = root.samples[0];
                for (frame, sample) in root.samples.iter_mut().enumerate() {
                    let authored = *sample;
                    if (28..46).contains(&frame.next_multiple_of(2)) {
                        let delta = authored - previous;
                        extra += Vec4::new(delta.x, 0., delta.z, 0.) * instant_travel;
                    }
                    previous = authored;
                    *sample += extra;
                }
            }
            w.clip_with_loop(slot, frames, &tracks, loop_frame)?;
            if advancing_shield && (54..=57).contains(&slot) {
                // 811EB426..811EB49E: +4 local Z each 30 Hz tick before the clip gate.
                // These source clips are also used by the stationary flurry.
                let root = tracks.iter_mut().find(|t| t.usage == Usage::RootTranslation)
                    .context("shield blow lacks a root track")?;
                root.samples.resize(frames as usize, *root.samples.last().unwrap());
                let gate = [16, 12, 14, 20][slot - 54];
                for (frame, point) in root.samples.iter_mut().enumerate() {
                    point.z += 2. * frame.min(gate) as f32;
                }
                let extra_slot = slots + slot - 54;
                w.clip_with_loop(extra_slot, frames, &tracks, loop_frame)?;
                sword_shield_equipment(&mut w, extra_slot, 1400 + slot)?;
            }
            if stepping_slash && slot == 1 {
                // 811F98BC adds 7.5 forward on ticks 40..56 in state 12 only.
                // Private clip 1460 lets GU apply this movement against terrain.
                let root = tracks
                    .iter_mut()
                    .find(|t| t.usage == Usage::RootTranslation)
                    .context("stepping slash lacks a root translation track")?;
                root.samples
                    .resize(frames as usize, *root.samples.last().unwrap());
                for (frame, point) in root.samples.iter_mut().enumerate() {
                    point.z += 7.5 * ((frame.saturating_sub(38) as f32 * 0.5).min(8.));
                }
                w.clip_with_loop(slots, frames, &tracks, loop_frame)?;
                long_sword_equipment(&mut w, slots, 1401)?;
            }
            if frames > 1 {
                let motion = 1000 + 100 * group + slot;
                match weapon {
                    Weapon::DualBlades => dual_blades_equipment(&mut w, slot, motion, frames)?,
                    Weapon::SwordShield => sword_shield_equipment(&mut w, slot, motion)?,
                    Weapon::Gunlance => gunlance_equipment(&mut w, slot, motion)?,
                    Weapon::GreatSword => equipment(&mut w, slot, motion)?,
                    Weapon::SwitchAxe => switch_axe_equipment(&mut w, slot, motion)?,
                    Weapon::LongSword => long_sword_equipment(&mut w, slot, motion)?,
                }
            }
        }
        assets.push(arc::Asset {
            name: format!("player\\mot\\{stem}\\{sex}_{group}"),
            kind: arc::MOTION,
            data: w.finish(),
        });
    }
    arc::encode(&assets)
}

// FZ 8120F906, sampled with the drawn flag set. Runs use durations, not end frames.
fn switch_axe_equipment(w: &mut Writer, slot: usize, clip: usize) -> Result<()> {
    let runs: &[(u32, u32)] = match clip {
        1002 => &[(4, 6), (1, 52), (2, 0)],
        1003 | 1009 | 1499 => &[(2, 34), (1, 58), (4, 0)],
        1412 => &[(2, 4), (1, 28), (2, 0)],
        1422 => &[(2, 36), (1, 76), (2, 0)],
        1423 => &[(1, 0)],
        1425 => &[(1, 52), (2, 0)],
        1493 => &[(1, 46), (2, 0)],
        _ => &[(2, 0)],
    };
    w.equipment_timeline(slot, &[1, 3, 2], runs)
}

// Vita 819070E2, weapon 7, integer playback frames. GU w07's equipment
// events 1, 3 and 2 select the right wrist, left wrist and back respectively.
fn long_sword_equipment(w: &mut Writer, slot: usize, clip: usize) -> Result<()> {
    let runs: &[(u32, u32)] = match clip {
        1002 => &[(4, 16), (1, 40), (2, 0)],
        1003 => &[(1, 52), (4, 0)],
        1005 => &[(1, 44), (4, 0)],
        1212 => &[(1, 94), (2, 0)],
        1412 => &[(1, 70), (4, 0)],
        1415 => &[(4, 8), (1, 20), (2, 0)],
        1426 => &[(4, 4), (1, 0)],
        1437 => &[(2, 2), (1, 216), (4, 0)],
        1441 | 1446 => &[(2, 4), (1, 0)],
        1442 => &[(2, 10), (1, 0)],
        1443 => &[(1, 0)],
        1447 => &[(4, 2), (1, 0)],
        1448 => &[(2, 40), (1, 162), (2, 0)],
        1452 => &[(2, 24), (1, 94), (2, 0)],
        _ => &[(2, 0)],
    };
    w.equipment_timeline(slot, &[1, 3, 2], runs)
}

fn dual_blades_equipment(w: &mut Writer, slot: usize, motion: usize, frames: u32) -> Result<()> {
    // GU w11 uses separate left/right hand flags (11 and 12). The common
    // draw/sheathe clips also keep the native back socket flag (2).
    if motion == 1002 && frames > 8 {
        w.equipment_timeline(slot, &[2, 12, 11], &[(1, 5), (5, 2), (7, 0)])
    } else if motion == 1003 && frames > 26 {
        w.equipment_timeline(slot, &[2, 12, 11], &[(7, 17), (5, 8), (1, 0)])
    } else {
        w.equipment_timeline(slot, &[11, 12], &[(3, 0)])
    }
}

fn gunlance_equipment(w: &mut Writer, slot: usize, motion: usize) -> Result<()> {
    // GU w09.lmt slots 2 and 3 switch between attachment 2 (back) and
    // attachment 3 (drawn). Attachment 1 belongs to other weapon types.
    let keys: &[(u32, u32)] = match motion {
        1003 => &[(1, 32), (2, 0)],
        _ => &[(1, 0)],
    };
    w.equipment_timeline(slot, &[3, 2], keys)
}

fn sword_shield_equipment(w: &mut Writer, slot: usize, motion: usize) -> Result<()> {
    // GU w01 attaches the blade to the left wrist and leaves the buckler on the right.
    let runs: &[(u32, u32)] = match motion {
        1003 => &[(1, 32), (2, 0)],
        1009 => &[(1, 30), (2, 0)],
        1450 => &[(1, 68), (2, 0)],
        _ => &[(1, 0)],
    };
    w.equipment_timeline(slot, &[3, 2], runs)
}

fn equipment(w: &mut Writer, slot: usize, motion: usize) -> Result<()> {
    // Frontier 819070E2, great-sword branch, at each integer motion frame.
    // Its grips 0/1/2 are GU events 1/3/2: right wrist, left wrist, back.
    // A fixed "drawn" event loses the draw/guard handoffs inside these clips.
    // Blend-only exceptions stay in the runtime adapter because the source
    // tests the interpolation clock, not the motion frame (notably 1448).
    let runs: &[(u32, u32)] = match motion {
        1002 => &[(4, 16), (1, 0)],
        1003 => &[(1, 38), (4, 0)],
        1017 => &[(1, 44), (4, 0)],
        1032 => &[(2, 40), (1, 48), (4, 0)],
        1431 => &[(4, 6), (1, 14), (2, 0)],
        1447 => &[(1, 18), (2, 0)],
        1450 | 1475 => &[(1, 108), (2, 0)],
        1473 => &[(1, 52), (2, 0)],
        1474 => &[(1, 106), (2, 0)],
        1476 => &[(1, 30), (2, 0)],
        1410 | 1412..=1415 | 1432 | 1434..=1436 | 1448 | 1449 | 1461 => &[(1, 0)],
        _ => &[(2, 0)],
    };
    w.equipment_timeline(slot, &[1, 3, 2], runs)
}

// The Vita player dispatcher at 81815D60, 81815F30 and 81816422 reads these
// same three arrays. Repack the GS records, retaining every field, including
// the unknown ones. Stored addresses are offsets from the start of this file.
pub fn dual_blades_parameters(data: &[u8]) -> Result<&[u8]> {
    ensure!(data.starts_with(b"mhf\x1a"), "expected decoded mhfdat.bin");
    let pointer = data
        .get(0x660..0x664)
        .context("missing Frontier dual-blades parameter offset")?;
    let start = u32::from_le_bytes(pointer.try_into()?) as usize;
    data.get(start..)
        .and_then(|tail| tail.get(..216))
        .context("truncated Frontier dual-blades parameters")
}

/// GU HTS group zero, built from the standard-reach column of the FZ shape directory.
pub fn blade_shapes(dat: &Path, output: &Path, selectors: &[usize], count: usize) -> Result<()> {
    let data = pac::decode(read(&dat.join("mhfdat.bin"))?)?;
    let word = |at| -> Result<usize> {
        Ok(u32::from_le_bytes(
            data.get(at..at + 4)
                .context("short shape pointer")?
                .try_into()?,
        ) as usize)
    };
    let directory = word(216)?;
    for &selector in selectors {
        let mut at = word(directory + 32 * selector)?;
        let mut rows = Vec::new();
        loop {
            let node = u16::from_le_bytes(
                data.get(at..at + 2)
                    .context("short shape node")?
                    .try_into()?,
            );
            if node == u16::MAX {
                break;
            }
            let shape = data.get(at..at + 40).context("short blade capsule")?;
            let joint = node
                .checked_sub(2)
                .and_then(|id| BONES.get(usize::from(id)))
                .context("unknown blade joint")?;
            ensure!(shape[2..4] == [1, 0], "expected a blade capsule");
            rows.extend(u16::from(*joint).to_le_bytes());
            rows.extend([0, 0, 1, 0, 0, 0]);
            rows.extend(0f32.to_le_bytes());
            rows.extend(&shape[12..28]);
            rows.extend(0f32.to_le_bytes());
            rows.extend(&shape[28..40]);
            rows.extend(0f32.to_le_bytes());
            at += 40;
        }
        ensure!(
            rows.len() == count * 48,
            "unexpected blade capsule count in group {selector}"
        );
        let mut hts = b"HTS\0".to_vec();
        for field in [0x20131015u32, 1, 0, (rows.len() / 48) as u32, 24] {
            hts.extend(field.to_le_bytes());
        }
        hts.extend(rows);
        super::save(&output.join(format!("blade_{selector}.hts")), hts)?;
    }
    Ok(())
}

pub fn actions(dat: &Path, kind: Weapon) -> Result<Vec<u8>> {
    let motion_data = pac::decode(read(&dat.join("mhfdat.bin"))?)?;
    let data = &motion_data[..];
    let sdt = pac::decode(read(&dat.join("mhfsdt.bin"))?)?;
    fn word(b: &[u8], at: usize) -> Result<usize> {
        let bytes = b
            .get(at..at + 4)
            .context("truncated mhfdat action directory")?;
        Ok(u32::from_le_bytes(bytes.try_into()?) as usize)
    }
    ensure!(data.starts_with(b"mhf\x1a"), "expected decoded mhfdat.bin");
    // First entry: 大剣. Entry 3 is ランス, not the great sword.
    let weapon = word(data, 0x614)? + kind as usize * 8;
    let (states, magic) = match kind {
        Weapon::SwordShield => (53, b"FSNS"),
        Weapon::GreatSword => (74, b"FGSA"),
        Weapon::DualBlades => (89, b"FDBA"),
        Weapon::LongSword => (85, b"FLSA"),
        Weapon::Gunlance => (82, b"FGLA"),
        Weapon::SwitchAxe => (178, b"FSAA"),
    };
    let n = word(data, weapon)?;
    ensure!(n == states, "unrecognized Frontier state table: {n} states");
    let records = word(data, weapon + 4)?;
    let mut out = magic.to_vec();
    out.extend((if kind == Weapon::SwitchAxe { 4u16 } else { 2u16 }).to_le_bytes());
    out.extend((n as u16).to_le_bytes());
    // The first SDT bank is the hunter melee bank (selector 0). FZ copies one
    // 40-byte row when spawning a contact; callbacks can compute the ID at run time.
    // Switch Axe event 4 constructs family 106 (81163D4C).
    let (bank, family) = if kind == Weapon::SwitchAxe {
        (6 * 28, 106)
    } else {
        (0, 0)
    };
    ensure!(word(&sdt, bank)? == family << 16, "unexpected SDT family");
    let attacks = u16::from_le_bytes(
        sdt.get(bank + 4..bank + 6)
            .context("short SDT bank")?
            .try_into()?,
    );
    let start = word(&sdt, bank + 8)?;
    ensure!(attacks > 0 && start >= 28, "empty SDT hunter table");
    let rows = sdt
        .get(start..)
        .and_then(|t| t.get(..usize::from(attacks) * 40))
        .context("truncated SDT hunter contacts")?;
    let attack_offset = 16 + 24 * n;
    out.extend(u32::from(attacks).to_le_bytes());
    out.extend((attack_offset as u32).to_le_bytes());
    out.resize(attack_offset, 0);
    out.extend(rows);
    for state in 0..n {
        for (field, stride) in [12, 40, 12].into_iter().enumerate() {
            let index = 24 * state + 8 * field;
            let count = word(data, records + index)?;
            if count == 0 {
                continue;
            }
            let off = word(data, records + index + 4)?;
            let bytes = count.checked_mul(stride).context("action array size")?;
            let src = data
                .get(off..)
                .and_then(|tail| tail.get(..bytes))
                .context("action record array out of bounds")?;
            let ptr = u32::try_from(out.len())?;
            out[16 + index..20 + index].copy_from_slice(&(count as u32).to_le_bytes());
            out[20 + index..24 + index].copy_from_slice(&ptr.to_le_bytes());
            out.extend_from_slice(src);
        }
    }
    if kind == Weapon::SwitchAxe {
        // PC DAT 0xa88 corresponds to the Vita parameter directory at +2692.
        let directory = word(data, 0xa88)?;
        ensure!(
            data.get(directory..directory + 2) == Some(&75u16.to_le_bytes()),
            "unknown w12 parameter layout"
        );
        let params = word(data, directory + 24)?;
        let bank = 7 * 28;
        ensure!(
            word(&sdt, bank)? == 107 << 16 && word(&sdt, bank + 4)? == 4 << 16 | 6,
            "unknown discharge bank"
        );
        let discharge = word(&sdt, bank + 8)?;
        // 812102B8 selects slash-powered actions even during temporary sword poses.
        // 81213F18 skips its pre-transition turn for these states. 81212276
        // installs the discharge counter callback only on the listed blade rows.
        for (count_field, table_field, limit) in [(20, 60, 256), (4, 48, 256), (16, 44, 128)] {
            let count = u16::from_le_bytes(data[directory+count_field..][..2].try_into()?) as usize;
            let start = word(data, directory + table_field)?;
            let mut mask = vec![0u8; limit / 8];
            for row in data.get(start..start+2*count).context("short switch axe selector list")?.chunks_exact(2) {
                let id = usize::from(u16::from_le_bytes(row.try_into()?));
                ensure!(id < limit, "switch axe selector exceeds bitmap");
                mask[id / 8] |= 1 << (id % 8);
            }
            out.extend(mask);
        }
        let models = word(data, directory+64)?;
        let count = u16::from_le_bytes(data[directory+10..directory+12].try_into()?) as usize;
        ensure!(count == 72, "unexpected w12 model motion count");
        out.extend(data.get(models..models+8*count).context("short w12 model motions")?);
        let shape_dir=word(&sdt,bank+16)?;
        let offsets=word(&sdt,bank+20)?;
        // 81164796..811649BA use the first three SDT parameters for the
        // explosion anchor. Node 126 is the fixed origin at effect +68.
        for i in 0..4 {
            let center=offsets+32*i;
            let shape=word(&sdt,shape_dir+32*i)?;
            out.extend(sdt.get(center..center+12).context("short discharge offset")?);
            out.extend(sdt.get(shape+12..shape+16).context("short discharge radius")?);
        }
        out.extend(b"SAFX");
        out.extend(
            sdt.get(discharge..discharge + 6 * 40)
                .context("short w12 discharge table")?,
        );
        out.extend(
            data.get(params..params + 150)
                .context("short w12 parameters")?,
        );
        return Ok(out);
    }
    if kind == Weapon::Gunlance {
        // 812031BA and 81206EFE read the w09 artillery and ignition timers
        // from the parameter block, while 81205E60 uses the reload table.
        let base = word(data, 1632)? + 268;
        let cooldowns = word(data, 1680)?;
        let barrage = word(data, 1692)?;
        out.extend(b"GLP1");
        for (offset, bytes) in [(base, 24), (cooldowns, 96), (barrage, 96)] {
            out.extend(data.get(offset..offset + bytes).context("short w09 parameters")?);
        }
        return Ok(out);
    }
    if kind == Weapon::SwordShield {
        // Type 87/1 initializes from parameter rows 60/61 at 81E98634.
        let offset = word(data, 1836)? + 60 * 32;
        out.extend(data.get(offset..offset + 64).context("short vacuum wave parameters")?);
        return Ok(out);
    }
    if kind == Weapon::LongSword {
        // 817C3E88 indexes 32-byte parameter groups. 811FA43E and 81847470
        // use groups 173/174 for the cut gauge, sprint and successful parry.
        let start = word(data, 1836)? + 32 * 173;
        // 81E93F44 selects family 85 in the 28-byte SDT directory. The
        // executable uses rows 1..4 for the two projectiles created at 128.
        let family = sdt
            .get(4 * 28..5 * 28)
            .context("missing long sword discharge bank")?;
        ensure!(
            family[..6] == [0, 0, 85, 0, 5, 0],
            "unexpected discharge bank"
        );
        let contacts = word(family, 8)? + 40;
        let bank = 8 * 28;
        ensure!(
            sdt.get(bank + 2..bank + 8) == Some(&[108, 0, 19, 0, 4, 0]),
            "missing instant slash bank"
        );
        let strike = word(&sdt, bank + 8)? + 4 * 40;
        let record = sdt
            .get(strike..strike + 40)
            .context("short instant slash contact")?;
        let selector = usize::from(u16::from_le_bytes(record[16..18].try_into()?));
        ensure!(selector < 4, "invalid instant slash shape selector");
        let pos = word(&sdt, word(&sdt, bank + 16)? + 32 * selector)?;
        let shape = sdt
            .get(pos..pos + 40)
            .context("short instant slash shape")?;
        ensure!(shape[..4] == [126, 0, 0, 0], "unknown instant slash shape");
        out.extend(b"LS04");
        out.extend(record);
        out.extend(&shape[12..16]);
        out.extend(b"LS85");
        out.extend(
            sdt.get(contacts..contacts + 4 * 40)
                .context("truncated discharge contacts")?,
        );
        let reach = word(data, word(data, 216)?)?;
        for offset in [64, 104, 92, 116] {
            out.extend(
                data.get(reach + offset..reach + offset + 4)
                    .context("short standard reach row")?,
            );
        }
        let common = word(data, 0x660)?;
        out.extend(
            data.get(common + 240..common + 242)
                .context("missing full spirit timer")?,
        );
        out.extend(b"LSPR");
        out.extend(
            data.get(start..start + 64)
                .context("short long sword parameters")?,
        );
        return Ok(out);
    }
    if kind == Weapon::DualBlades {
        out.extend(b"DBPR");
        out.extend(dual_blades_parameters(data)?);
        return Ok(out);
    }
    // 816D815C resolves hunter contacts through mhfdat's shape directory.
    // GU's weapons use the standard reach (column zero). The counter's
    // capsule follows FZ node 0, independent of the hand and sword model.
    let counter = rows
        .get(247 * 40..248 * 40)
        .context("missing guard counter contact")?;
    let selector = u16::from_le_bytes(counter[16..18].try_into()?);
    let shape = word(data, word(data, 216)? + 32 * usize::from(selector))?;
    let capsule = data
        .get(shape..shape + 42)
        .context("missing guard counter shape")?;
    ensure!(
        selector == 39 && capsule[..4] == [0, 0, 1, 0] && capsule[40..42] == [255, 255],
        "unsupported guard counter shape"
    );
    out.extend(b"CTR0");
    out.extend(&capsule[..40]);
    // 81159BD8 spawns effect family 108; 8115BA10 uses 81898B02
    // to fetch slot 8, then the row's shape selector. This is a separate
    // world-space hit volume, not a row in the hunter's blade bank.
    let bank = 8 * 28;
    ensure!(
        sdt.get(bank + 2..bank + 8) == Some(&[108, 0, 19, 0, 4, 0]),
        "unknown great sword effect bank"
    );
    let pulse = word(&sdt, bank + 8)? + 8 * 40;
    let record = sdt
        .get(pulse..pulse + 40)
        .context("missing radiance contact")?;
    let shape_id = u16::from_le_bytes(record[16..18].try_into()?);
    ensure!(shape_id < 4, "missing radiance shape");
    let shape = word(&sdt, word(&sdt, bank + 16)? + 32 * usize::from(shape_id))?;
    let capsule = sdt
        .get(shape..shape + 40)
        .context("truncated radiance capsule")?;
    ensure!(capsule[..4] == [127, 0, 1, 0], "unsupported radiance shape");
    out.extend(b"FX08");
    out.extend(record);
    out.extend(capsule);
    Ok(out)
}

pub fn switch_axe_shapes(dat: &Path, output: &Path) -> Result<()> {
    let sdt = pac::decode(read(&dat.join("mhfsdt.bin"))?)?;
    let word = |at| -> Result<usize> {
        Ok(u32::from_le_bytes(
            sdt.get(at..at + 4)
                .context("short SDT pointer")?
                .try_into()?,
        ) as usize)
    };
    for (bank, family, count) in [(6 * 28, 106, 7), (7 * 28, 107, 4)] {
        ensure!(
            word(bank)? == family << 16 && word(bank + 4)? >> 16 == count,
            "unexpected w12 shape bank"
        );
        for selector in 0..count {
            let row = word(word(bank + 16)? + selector * 32)?;
            let shape = sdt.get(row..row + 42).context("short w12 shape")?;
            ensure!(shape[40..42] == [255, 255], "expected one w12 shape");
            let joint = match u16::from_le_bytes(shape[..2].try_into()?) {
                14 => BONES[12],
                0 | 126 => 0,
                node => anyhow::bail!("unknown w12 shape joint {node}"),
            };
            let mut hts = b"HTS\0".to_vec();
            for value in [0x20131015u32, 1, 0, 1, 24] {
                hts.extend(value.to_le_bytes());
            }
            hts.extend(u16::from(joint).to_le_bytes());
            hts.extend([0, 0]);
            hts.extend(&shape[2..4]);
            hts.extend([0; 6]);
            hts.extend(&shape[12..28]);
            hts.extend([0; 4]);
            hts.extend(&shape[28..40]);
            hts.extend([0; 4]);
            super::save(&output.join(format!("hit_{family}_{selector}.hts")), hts)?;
        }
    }
    Ok(())
}
