use super::*;
use mhf_resource::pac;
use std::collections::BTreeMap;
use std::fs;

fn word(b: &[u8], at: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(
        b.get(at..at + 4)
            .context("short sound directory")?
            .try_into()?,
    ) as usize)
}

// The PC loose weapon files are empty placeholders. The MHA directory maps
// the actual equipped model name to its native MOMO, including aliased files.
pub(super) fn iron(dat: &Path) -> Result<Bank> {
    let mut bank = model_bank(dat, "we000snd.abn", b"we010.snd\0", 0..32)?;
    let accent = bank
        .voices
        .get(&14)
        .context("missing great sword charge accent")?;
    let (wave, gain, cents) = (accent.wave, accent.gain, accent.cents);
    for cue in 128..=130 {
        bank.voices.insert(cue, source::Voice { wave, gain, cents });
    }
    Ok(bank)
}

pub(super) fn model_bank(
    dat: &Path,
    archive: &str,
    model: &[u8],
    cues: impl IntoIterator<Item = u8>,
) -> Result<Bank> {
    let bytes = fs::read(dat.join("extend").join(archive))?;
    ensure!(
        bytes.starts_with(b"mha\x01"),
        "expected weapon sound archive"
    );
    let names = word(&bytes, 12)?;
    for i in 0..word(&bytes, 8)? {
        let row = word(&bytes, 4)? + 20 * i;
        let name = names + word(&bytes, row)?;
        if bytes.get(name..name + model.len()) != Some(model) {
            continue;
        }
        let start = word(&bytes, row + 4)?;
        let data = bytes
            .get(start..start + word(&bytes, row + 8)?)
            .context("short weapon model bank")?;
        return Bank::player_bytes(data, cues);
    }
    anyhow::bail!("missing weapon model request in {archive}")
}

// FGSE: clip-indexed spans of 12-byte records. Four (cue, weight) pairs
// retain the source's weighted variants, including 255 = deliberate silence.
pub(super) fn timeline(dat: &Path, weapon: usize) -> Result<(Vec<u8>, [BTreeSet<u8>; 3])> {
    let b = pac::decode(fs::read(dat.join("mhfdat.bin"))?)?;
    let mut clips = vec![Vec::<[u8; 12]>::new(); 500];
    // The fourth charge cue is triggered by gameplay, not the motion table.
    // Contact and deflection are played by GU's own melee resolver.
    let mut cues = [BTreeSet::new(), BTreeSet::new(), BTreeSet::new()];
    if weapon == 4 {
        cues[0].insert(22);
    } else if weapon == 12 {
        cues[0].extend([1, 2, 12, 13, 20, 21, 22, 23, 24, 25, 26, 27, 28]);
        cues[1].extend([78, 79]);
    } else if weapon == 7 {
        cues[0].extend([0, 1, 2, 3, 8, 9, 10, 11, 12]);
        cues[2].insert(12);
        cues[1].insert(235);
    } else if weapon == 9 {
        cues[1].insert(101);
        // 812031BA plays the first chamber sound outside the motion table.
        cues[2].extend([12, 13]);
    } else if weapon == 0 {
        cues[1].insert(210);
    } else if weapon == 6 {
        // w06 action event 16 plays this cue through 818DD2DC.
        cues[0].insert(1);
    }
    let mut compiled = BTreeMap::<(usize, u16, u8), Vec<(u8, u8)>>::new();
    let native = match weapon {
        0 => include_str!("gs_native.txt"),
        6 => include_str!("db_native.txt"),
        7 => include_str!("ls_native.txt"),
        4 => include_str!("sns_native.txt"),
        9 => include_str!("gl_native.txt"),
        12 => "",
        _ => anyhow::bail!("unsupported weapon sound timeline {weapon}"),
    };
    for l in native.lines().filter(|l| !l.starts_with('#')) {
        let mut fields = l.split_whitespace();
        let clip: usize = fields.next().context("no clip")?.parse()?;
        for event in fields {
            let a = event
                .split('/')
                .map(str::parse::<u16>)
                .collect::<Result<Vec<_>, _>>()?;
            let &[f, k, c, w] = a.as_slice() else {
                anyhow::bail!("bad native sound event")
            };
            compiled
                .entry((clip, f, k as u8))
                .or_default()
                .push((c as u8, w as u8));
        }
    }
    let mut emit = |clip: usize, frame: u16, bank: u8, options: &[(u8, u8)]| -> Result<()> {
        if bank == 2 && options.iter().all(|c| c.0 >= 32) {
            return Ok(());
        }
        ensure!(options.len() <= 4, "too many sound variants");
        let mut row = [0u8; 12];
        row[..2].copy_from_slice(&frame.to_le_bytes());
        row[2] = bank;
        row[3] = options.len() as u8;
        for (i, &(cue, weight)) in options.iter().enumerate() {
            row[4 + 2 * i] = cue;
            row[5 + 2 * i] = weight;
            if cue != 255 {
                cues[usize::from(bank)].insert(cue);
            }
        }
        clips
            .get_mut(clip.checked_sub(1000).context("invalid motion")?)
            .context("invalid motion index")?
            .push(row);
        Ok(())
    };
    for ((clip, f, k), mut choices) in compiled {
        for &cue in choices.iter().filter(|cue| cue.1 == 8) {
            emit(clip, f, k, &[cue])?;
        }
        choices.retain(|cue| cue.1 != 8);
        if choices.is_empty() {
            continue;
        }
        let total: u8 = choices.iter().map(|x| x.1).sum();
        ensure!(total <= 8, "unresolved simultaneous sound");
        if total < 8 {
            choices.push((255, 8 - total));
        }
        emit(clip, f, k, &choices)?;
    }
    let dir = word(&b, 0x61c)? + 8 * (weapon + 1); // 8174D866
    for i in 0..word(&b, dir)? {
        let p = word(&b, dir + 4)? + 8 * i;
        let h = |pos| -> Result<u16> {
            Ok(u16::from_le_bytes(
                b.get(pos..pos + 2)
                    .context("short FZ motion SE")?
                    .try_into()?,
            ))
        };
        for j in 0..h(p + 2)? as usize {
            let r = word(&b, p + 4)? + 22 * j;
            let bank = match h(r + 2)? {
                0 => 1,
                3 | 4 => 2,
                6 => 0,
                _ => continue,
            }; // footsteps, armour: GU scene
            if weapon == 12 && h(r + 4)? == 4 {
                // Conditional sheathing accent. The controller emits it in sword form.
                continue;
            }
            ensure!(h(r + 4)? == 0, "unsupported w{weapon:02} cue condition");
            let mut choices = Vec::new();
            for k in 0..4 {
                let w = h(r + 8 + 4 * k)?;
                if w != 0 {
                    choices.push((h(r + 6 + 4 * k)?.min(255) as u8, u8::try_from(w)?));
                }
            }
            if choices.is_empty() {
                choices.push((h(r + 6)?.min(255) as u8, 1));
            }
            emit(h(p)? as usize, h(r)?, bank, &choices)?;
        }
    }
    let mut out = vec![0; 500 * 4];
    for (i, mut clip) in clips.into_iter().enumerate() {
        clip.sort_by_key(|r| u16::from_le_bytes([r[0], r[1]]));
        let offset = u16::try_from(out.len())?;
        out[4 * i..4 * i + 2].copy_from_slice(&offset.to_le_bytes());
        out[4 * i + 2..4 * i + 4].copy_from_slice(&(clip.len() as u16).to_le_bytes());
        for row in clip {
            out.extend(row);
        }
    }
    Ok((out, cues))
}
