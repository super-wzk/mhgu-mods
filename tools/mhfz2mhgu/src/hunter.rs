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

pub fn great_sword(dat: &Path, female: bool) -> Result<Vec<u8>> {
    let sex = if female { "f" } else { "m" };
    let model = pac::decode(read(&dat.join(format!("parts/{sex}00/{sex}_editpl.bin")))?)?;
    let rig = Rig::parse(&pac::member(&model, 1)?)?;
    ensure!(
        rig.joints.len() == 21 && (0..21).all(|i| rig.by_id.get(&(i as u16)) == Some(&i)),
        "unrecognized hunter skeleton"
    );
    let b = pac::decode(read(&dat.join("motion/w00.mot"))?)?;
    let mot = MotionArchive::from_directory(&b)?;
    ensure!(
        mot.groups
            .iter()
            .map(|g| g.motion_offsets.len())
            .eq([100, 100, 15, 15, 80, 80, 0]),
        "unrecognized Frontier great sword motion directory"
    );
    let mut assets = Vec::new();
    for group in [0, 2, 4] {
        let slots = mot.groups[group].motion_offsets.len();
        ensure!(
            mot.groups[group + 1].motion_offsets.len() == slots,
            "mismatched hunter layers"
        );
        let mut w = Writer::new(slots)?;
        for slot in 0..slots {
            let pair = (mot.motion(group, slot)?, mot.motion(group + 1, slot)?);
            let (lower, upper) = match pair {
                (None, None) => continue,
                (Some(a), Some(b)) => (a, b),
                _ => anyhow::bail!("unpaired w00 slot {group}:{slot}"),
            };
            let (lo, a) = bake(&lower, &rig, Some(0), Some(0))
                .with_context(|| format!("w00 lower {group}:{slot}"))?;
            let (hi, b) = bake(&upper, &rig, Some(1), Some(0))
                .with_context(|| format!("w00 upper {}:{slot}", group + 1))?;
            let frames = lo.max(hi);
            let loop_frame = match (lower.metadata_present, upper.metadata_present) {
                (0, 0) => None,
                (1, 1) if lower.metadata == upper.metadata => {
                    let f = f32::from_bits(lower.metadata);
                    ensure!(
                        f.fract() == 0. && (0.0..frames as f32).contains(&f),
                        "invalid w00 loop point {group}:{slot}"
                    );
                    Some(f as u32)
                }
                _ => anyhow::bail!("unmatched hunter loop points at {group}:{slot}"),
            };
            w.clip_with_loop(
                slot,
                frames,
                &merge(&rig, [a, b], frames as usize)?,
                loop_frame,
            )?;
            equipment(&mut w, slot, 1000 + 100 * group + slot)?;
        }
        assets.push(arc::Asset {
            name: format!("player\\mot\\frontier_gs\\{sex}_{group}"),
            kind: arc::MOTION,
            data: w.finish(),
        });
    }
    arc::encode(&assets)
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
pub fn actions(dat: &Path) -> Result<Vec<u8>> {
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
    let weapon = word(data, 0x614)?;
    let n = word(data, weapon)?;
    ensure!(
        n == 74,
        "unrecognized extreme great sword state table: {n} states"
    );
    let records = word(data, weapon + 4)?;
    let mut out = b"FGSA".to_vec();
    out.extend(2u16.to_le_bytes());
    out.extend((n as u16).to_le_bytes());
    // The first SDT bank is the hunter melee bank (selector 0). FZ copies one
    // 40-byte row when spawning a contact; callbacks can compute the ID at run time.
    ensure!(sdt.get(2..4) == Some(&[0, 0]), "expected SDT hunter bank 0");
    let attacks = u16::from_le_bytes(sdt.get(4..6).context("short SDT bank")?.try_into()?);
    let start = word(&sdt, 8)?;
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
