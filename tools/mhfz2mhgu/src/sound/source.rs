//! Frontier 82031336 resolves request -> program key range -> Smpl -> Vagi.
fn half(b: &[u8], p: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        b.get(p..p + 2)
            .context("short FZ sound field")?
            .try_into()?,
    ))
}
fn word(b: &[u8], p: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4)
            .context("short FZ sound field")?
            .try_into()?,
    ) as usize)
}
use anyhow::{Context, Result, ensure};
use lewton::inside_ogg::OggStreamReader;
use mhf_resource::pac;
use std::{collections::BTreeMap, fs, io::Cursor, path::Path};

pub struct Voice {
    pub wave: usize,
    pub gain: u8,
    pub cents: i16,
}
pub struct Bank {
    pub voices: BTreeMap<u8, Voice>,
    pub waves: Vec<Vec<i16>>,
    pub rate: u32,
}

fn member(file: &[u8], i: usize) -> Result<Vec<u8>> {
    ensure!(file.starts_with(b"MOMO"), "expected FZ sound envelope");
    ensure!(i < word(file, 4)?, "missing FZ sound member {i}");
    let start = word(file, 8 + 8 * i)?;
    let size = word(file, 12 + 8 * i)?;
    pac::decode(
        file.get(start..start + size)
            .context("truncated FZ sound member")?
            .to_vec(),
    )
}

impl Bank {
    pub fn read(path: &Path) -> Result<Self> {
        let file = fs::read(path.join("em080.snp"))?;
        let seq = member(&fs::read(path.join("em999.snd"))?, 2)?;
        Self::parse(&file, &seq, 0..100, Some(80))
    }
    pub fn player(path: &Path, cues: impl IntoIterator<Item = u8>) -> Result<Self> {
        let file = fs::read(path)?;
        Self::player_bytes(&file, cues)
    }
    pub fn player_bytes(file: &[u8], cues: impl IntoIterator<Item = u8>) -> Result<Self> {
        Self::parse(file, &member(file, 2)?, cues, None)
    }
    fn parse(
        file: &[u8],
        seq: &[u8],
        cues: impl IntoIterator<Item = u8>,
        fixed: Option<u8>,
    ) -> Result<Self> {
        let h = member(file, 0)?;
        ensure!(
            h.starts_with(b"Head") && seq.starts_with(b"dbsT"),
            "unexpected FZ sound bank"
        );
        let mut ogg = OggStreamReader::new(Cursor::new(member(file, 1)?))?;
        let rate = ogg.ident_hdr.audio_sample_rate;
        ensure!(
            ogg.ident_hdr.audio_channels == 1,
            "expected mono Frontier audio"
        );
        let mut pcm = vec![];
        while let Some(packet) = ogg.read_dec_packet_itl()? {
            pcm.extend(packet);
        }
        let prog = word(&h, 20)?;
        let smpl = word(&h, 24)?;
        let vagi = word(&h, 28)?;
        ensure!(
            h.get(prog..prog + 4) == Some(b"Prog")
                && h.get(smpl..smpl + 4) == Some(b"Smpl")
                && h.get(vagi..vagi + 4) == Some(b"Vagi"),
            "missing FZ instrument tables"
        );
        let mut waves = Vec::new();
        for i in 0..=word(&h, vagi + 8)? {
            let p = vagi + 16 + 20 * i;
            let byte = word(&h, p)?;
            let n = word(&h, p + 4)?;
            ensure!(
                byte % 2 == 0 && n > 0 && u32::from(half(&h, p + 18)?) == rate,
                "invalid FZ sample slice"
            );
            waves.push(
                pcm.get(byte / 2..byte / 2 + n)
                    .context("short FZ waveform")?
                    .to_vec(),
            );
        }
        let mut voices = BTreeMap::new();
        for cue in cues {
            let p = 16 + 32 * usize::from(cue);
            let r = seq.get(p..p + 32).context("missing FZ sound request")?;
            if fixed.is_none() && r[0] == 2 {
                // 82031AF8 dispatches a voice release command.
                continue;
            }
            ensure!(
                r[0] == 1 && (fixed.is_none() || r[6] == 0),
                "unsupported FZ request {cue}"
            );
            let program = usize::from(fixed.unwrap_or(r[6]));
            ensure!(
                program <= word(&h, prog + 8)?,
                "invalid instrument {program}"
            );
            let inst = prog + word(&h, prog + 16 + 4 * program)?;
            let instrument = h.get(inst..inst + 8).context("missing Frontier program")?;
            let note = r[7];
            for i in 0..usize::from(instrument[0]) {
                let p = inst + 8 + 14 * i;
                let zone = h.get(p..p + 14).context("truncated instrument zone")?;
                if !(zone[2]..=zone[3]).contains(&note) {
                    continue;
                }
                let p = smpl + 16 + 12 * usize::from(half(zone, 12)?);
                let sample = h.get(p..p + 12).context("invalid sample index")?;
                let wave = usize::from(half(sample, 10)?);
                ensure!(wave < waves.len(), "invalid waveform");
                let gain = (u32::from(instrument[2]) * u32::from(zone[8]) / 127
                    * u32::from(sample[6])
                    / 127
                    * u32::from(r[12])
                    / 127) as u8;
                let semis = |x: u8| i16::from(x as i8) * 100;
                let cents = semis(instrument[4])
                    + semis(zone[10])
                    + semis(sample[8])
                    + 100 * (i16::from(note) - i16::from(sample[5]))
                    + i16::from(zone[11] as i8)
                    + i16::from(instrument[5] as i8)
                    + i16::from(sample[9] as i8);
                ensure!(
                    voices.insert(cue, Voice { wave, gain, cents }).is_none(),
                    "layered Frontier voice"
                );
            }
        }
        Ok(Self {
            voices,
            waves,
            rate,
        })
    }
}
