//! Bind FZ sound events while allocating GU motion slots, before the ARC is encoded.
mod gs;
mod source;
mod wave;
use anyhow::{Context, Result, ensure};
use mhgu_resource::arc::Asset;
use source::Bank;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const MSS: u32 = 0x10ebe843;
const REQUEST: u32 = 0x1bcc4966;
const BANK: u32 = 0x15d782fb;
const WAVE: u32 = 0x79c47b59;
const PRIVATE: &str = r"sound\se\em\em107_00\snd_em107_00";
// Outside vanilla request IDs and the 0..30 game-side material remap.
const REQUEST_BASE: u16 = 4096;
const BANK_GROUP: u16 = 107;
type Event = (u16, u8, u8); // frame, FZ request, joint
fn put(b: &mut [u8], p: usize, x: usize) {
    b[p..p + 4].copy_from_slice(&(x as u32).to_le_bytes());
}
fn asset(name: impl Into<String>, kind: u32, data: Vec<u8>) -> Asset {
    Asset {
        name: name.into(),
        kind,
        data,
    }
}
fn wave_path(i: usize) -> String {
    format!(r"sound\se\em\em107_00\wav\fz{i:02}")
}
pub struct Timeline {
    source: BTreeMap<(u8, usize), Vec<Event>>,
    tracks: BTreeMap<usize, (u16, Vec<Event>)>,
}
impl Timeline {
    pub fn new() -> Result<Self> {
        let mut source = BTreeMap::new();
        for line in include_str!("espinas.cues")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let mut fields = line.split_ascii_whitespace();
            let key = (
                fields.next().context("missing motion")?.parse()?,
                fields.next().context("missing overlay")?.parse()?,
            );
            let mut events = Vec::new();
            for s in fields {
                let (frame, tail) = s.split_once(':').context("missing frame")?;
                let (cue, joint) = tail.split_once(':').context("missing cue")?;
                events.push((frame.parse()?, cue.parse()?, joint.parse()?));
            }
            ensure!(
                source.insert(key, events).is_none(),
                "duplicate FZ sound track"
            );
        }
        Ok(Self {
            source,
            tracks: BTreeMap::new(),
        })
    }
    pub fn bind(
        &mut self,
        key: usize,
        motions: &[u8],
        bounds: &[u32],
        overlay: usize,
        ids: &[u8],
    ) -> Result<()> {
        ensure!(
            bounds.len() == motions.len() + 1,
            "missing sound phase bounds"
        );
        let mut events = Vec::new();
        for (&m, span) in motions.iter().zip(bounds.windows(2)) {
            // Base 34's tagged poses have their own branches; other overlays use the base dispatch.
            let cues = self
                .source
                .get(&(m, overlay))
                .or_else(|| self.source.get(&(m, 0)));
            for &(f, c, j) in cues.into_iter().flatten() {
                let frame = span[0] + u32::from(f);
                if frame < span[1] {
                    events.push((
                        frame.try_into()?,
                        c,
                        *ids.get(usize::from(j)).context("unmapped FZ sound joint")?,
                    ));
                }
            }
        }
        ensure!(
            self.tracks
                .insert(key, ((*bounds.last().unwrap()).try_into()?, events))
                .is_none(),
            "duplicate GU motion sound key"
        );
        Ok(())
    }
    pub fn assets(self, path: &Path) -> Result<Vec<Asset>> {
        let bank = Bank::read(path)?;
        let mut cues: BTreeSet<_> = self
            .tracks
            .values()
            .flat_map(|(_, e)| e.iter().map(|x| x.1))
            .collect();
        cues.insert(37); // Breath 31 impact: 818DDC94 requests cue 37.
        cues.insert(87); // 81CD8D14: periodic sleeping breath, independent of motion phase.
        let mut assets = bank_assets(&bank, cues, PRIVATE, BANK_GROUP, wave_path)?;
        assets.push(self.motion_asset(&bank)?);
        Ok(assets)
    }
}
fn bank_assets(
    bank: &Bank,
    cues: BTreeSet<u8>,
    path: &str,
    group: u16,
    wave_path: impl Fn(usize) -> String,
) -> Result<Vec<Asset>> {
    let active: Vec<_> = cues
        .into_iter()
        .filter_map(|id| bank.voices.get(&id).map(|v| (id, v)))
        .collect();
    let mut assets = vec![];
    for i in active.iter().map(|(_, v)| v.wave).collect::<BTreeSet<_>>() {
        assets.push(asset(
            wave_path(i),
            WAVE,
            wave::encode(&bank.waves[i], bank.rate),
        ));
    }
    // SBKR: one note group; an explicit wave range for each active FZ cue.
    ensure!(active.len() < 128, "too many voices in Espinas bank");
    let mut sb = b"SBKR\x04\0\0\0".to_vec();
    for n in [1u32, active.len() as u32, 0] {
        sb.extend(n.to_le_bytes());
    }
    sb.extend(group.to_le_bytes());
    sb.extend([(active.len() * 2) as u8, 8]);
    sb.extend([0; 8]); // first wave and mix index
    for (i, (_, v)) in active.iter().enumerate() {
        sb.extend(wave_path(v.wave).bytes());
        sb.push(0);
        let mut row = [0u8; 80]; // one spatial mono voice
        row[..4].copy_from_slice(&WAVE.to_le_bytes());
        row[5] = 87;
        row[9] = v.gain;
        // Neutral GU spatial playback range, also used by common quest waves.
        row[10..12].fill(127);
        row[13..15].copy_from_slice(&[0xc1, 0x3f]);
        row[16..18].copy_from_slice(&v.cents.to_le_bytes());
        row[25] = 127;
        row[27] = 127;
        row[28..32].copy_from_slice(&(1f32 / 3.).to_le_bytes());
        row[32..36].copy_from_slice(&(2f32 / 3.).to_le_bytes());
        row[42..44].copy_from_slice(&350u16.to_le_bytes());
        row[52] = 127;
        row[64..66].copy_from_slice(&u16::MAX.to_le_bytes());
        row[67] = 24;
        row[70..72].copy_from_slice(&2000u16.to_le_bytes());
        row[75..78].fill((i + 1) as u8);
        sb.extend(row);
    }
    assets.push(asset(path, BANK, sb));

    // SRQR: one private bank, no auxiliary paths. There are no inherited
    // request IDs; all motion events and the impact cue address FZ waves.
    let mut sr = vec![0; 52];
    sr[..8].copy_from_slice(b"SRQR\x05\0\0\0");
    put(&mut sr, 8, active.len());
    put(&mut sr, 28, 0x0291df77); // GU request format revision
    for (i, (cue, _)) in active.iter().enumerate() {
        let mut row = [0; 132];
        row[..2].copy_from_slice(&(REQUEST_BASE + u16::from(*cue)).to_le_bytes());
        row[2] = (i + 1) as u8;
        row[3] = 127;
        for (index, value) in [
            (8, 1),
            (16, 100),
            (18, 1),
            (20, 100),
            (21, 1),
            (24, 1),
            (104, 100),
        ] {
            row[index] = value;
        }
        row[28..30].fill(255);
        row[30..32].copy_from_slice(&group.to_le_bytes());
        row[32..34].fill(255);
        row[36..40].copy_from_slice(&(-2.1f32).to_le_bytes());
        row[40..44].copy_from_slice(&(-2.1f32).to_le_bytes());
        row[56] = 47;
        row[60] = 47;
        row[96..100].copy_from_slice(&(-96f32).to_le_bytes());
        row[100..104].fill(255);
        row[124..128].fill(255);
        sr.extend(row);
    }
    let table = sr.len();
    put(&mut sr, 32, table);
    sr.extend(((table + 4) as u32).to_le_bytes());
    sr.extend(path.bytes());
    sr.push(0);
    sr.resize(sr.len().next_multiple_of(4), 0);
    let end = sr.len();
    for at in (36..52).step_by(4) {
        put(&mut sr, at, end);
    }
    assets.push(asset(path, REQUEST, sr));
    Ok(assets)
}
pub fn great_sword(dat: &Path) -> Result<(Vec<Asset>, Vec<u8>)> {
    let mut assets = Vec::new();
    let (timeline, [_, common, _]) = gs::timeline(dat, 0)?;
    for (name, bank) in [
        ("blade", Bank::player(&dat.join("sound/we00.snd"), 0..35)?),
        (
            "common",
            Bank::player(&dat.join("sound/common01.snd"), common)?,
        ),
        ("iron", gs::iron(dat)?),
    ] {
        let path = format!(r"sound\se\pl\frontier_gs\{name}");
        assets.extend(bank_assets(
            &bank,
            bank.voices.keys().copied().collect(),
            &path,
            1,
            |i| format!(r"{path}\w{i:02}"),
        )?);
    }
    Ok((assets, timeline))
}

pub fn gunlance(dat: &Path) -> Result<(Vec<Asset>, Vec<u8>)> {
    hunter_banks(dat, 9, "frontier_gl", "wi000snd.abn", b"wi010.snd\0")
}

pub fn sword_shield(dat: &Path) -> Result<(Vec<Asset>, Vec<u8>)> {
    hunter_banks(dat, 4, "frontier_sns", "wd000snd.abn", b"wd000.snd\0")
}

pub fn dual_blades(dat: &Path) -> Result<(Vec<Asset>, Vec<u8>)> {
    hunter_banks(dat, 6, "frontier_db", "wh000snd.abn", b"wh006.snd\0")
}

pub fn switch_axe(dat: &Path) -> Result<(Vec<Asset>, Vec<u8>)> {
    hunter_banks(dat, 12, "frontier_sa", "wk000snd.abn", b"wk000.snd\0")
}

pub fn long_sword(dat: &Path) -> Result<(Vec<Asset>, Vec<u8>)> {
    hunter_banks(dat, 7, "frontier_ls", "we000snd.abn", b"we000.snd\0")
}

fn hunter_banks(
    dat: &Path,
    id: usize,
    namespace: &str,
    archive: &str,
    model: &[u8],
) -> Result<(Vec<Asset>, Vec<u8>)> {
    let (timeline, [blade, common, weapon]) = gs::timeline(dat, id)?;
    let mut model_bank = gs::model_bank(
        dat,
        archive,
        model,
        weapon.into_iter().filter(|&id| id < 32),
    )?;
    if id == 9 {
        // wi010 lacks zones for 20/28 and points 3/12/13 to a silent placeholder.
        for (model, cues) in [
            (b"wi037.snd\0", &[20, 28][..]),
            (b"wi011.snd\0", &[3, 12, 13][..]),
        ] {
            let mut alternate = gs::model_bank(dat, archive, model, cues.iter().copied())?;
            ensure!(
                alternate.rate == model_bank.rate,
                "gunlance weapon banks differ in sample rate"
            );
            for &cue in cues {
                let mut voice = alternate
                    .voices
                    .remove(&cue)
                    .with_context(|| format!("missing gunlance weapon cue {cue}"))?;
                let wave = &alternate.waves[voice.wave];
                ensure!(
                    voice.gain != 0 && wave.iter().any(|s| s.unsigned_abs() > 256),
                    "silent gunlance weapon cue {cue}"
                );
                voice.wave = model_bank.waves.len();
                model_bank.waves.push(wave.clone());
                model_bank.voices.insert(cue, voice);
            }
        }
    }
    let mut assets = Vec::new();
    for (name, bank) in [
        (
            "blade",
            Bank::player(&dat.join(format!("sound/we{id:02}.snd")), blade)?,
        ),
        (
            "common",
            Bank::player(&dat.join("sound/common01.snd"), common)?,
        ),
        ("weapon", model_bank),
    ] {
        if bank.voices.is_empty() {
            continue;
        }
        let path = format!(r"sound\se\pl\{namespace}\{name}");
        assets.extend(bank_assets(
            &bank,
            bank.voices.keys().copied().collect(),
            &path,
            1,
            |i| format!(r"{path}\w{i:02}"),
        )?);
    }
    Ok((assets, timeline))
}
impl Timeline {
    fn motion_asset(self, bank: &Bank) -> Result<Asset> {
        let mut tracks = BTreeMap::new();
        for (key, (n, events)) in self.tracks {
            let mut frames: BTreeMap<u16, Vec<(u8, u8)>> = BTreeMap::new();
            for (f, c, j) in events {
                if bank.voices.contains_key(&c) {
                    frames.entry(f).or_default().push((c, j));
                }
            }
            tracks.insert(key, encode_track(key, n, frames));
        }
        ensure!(tracks.len() <= 512, "MSS directory overflow");
        let mut out = vec![0; 16];
        out[..8].copy_from_slice(b"MSS\0\x15\x10\x13\x20");
        out[11] = 1;
        out[8..10].copy_from_slice(&(tracks.len() as u16).to_le_bytes());
        out.resize(1040, 255);
        out.resize(1040 + 8 * tracks.len(), 0);
        for (i, (key, track)) in tracks.into_iter().enumerate() {
            out[16 + 2 * i..18 + 2 * i].copy_from_slice(&(i as u16).to_le_bytes());
            let p = out.len();
            put(&mut out, 1040 + 8 * i, key);
            put(&mut out, 1044 + 8 * i, p);
            out.extend(track);
        }
        Ok(asset(r"sound\call\em\em107", MSS, out))
    }
}
fn encode_track(key: usize, duration: u16, frames: BTreeMap<u16, Vec<(u8, u8)>>) -> Vec<u8> {
    let start = 16 + 8 * frames.len();
    let mut t = vec![0; start];
    put(&mut t, 0, key);
    t[4..6].copy_from_slice(&(frames.len() as u16).to_le_bytes());
    t[6..8].copy_from_slice(&duration.to_le_bytes());
    put(&mut t, 8, u32::MAX as usize);
    put(&mut t, 12, start);
    t.extend([255, 255, 0, 0]);
    for (i, (f, events)) in frames.into_iter().enumerate() {
        let p = t.len();
        put(&mut t, 16 + i * 8, usize::from(f));
        put(&mut t, 20 + i * 8, p);
        t.extend(f.to_le_bytes());
        t.extend((events.len() as u16).to_le_bytes());
        t.resize(p + 4 + 4 * events.len(), 0);
        for (j, (cue, bone)) in events.into_iter().enumerate() {
            let rel = t.len() - p;
            put(&mut t, p + 4 + 4 * j, rel);
            t.extend([0x63, 1, 3, 0]);
            for ty in [3u32, 6, 3] {
                t.extend(ty.to_le_bytes());
            }
            t.extend((REQUEST_BASE + u16::from(cue)).to_le_bytes());
            t.extend(u16::from(bone).to_le_bytes());
            t.extend(0u16.to_le_bytes());
        }
    }
    t.resize(t.len().next_multiple_of(4), 0);
    t
}
