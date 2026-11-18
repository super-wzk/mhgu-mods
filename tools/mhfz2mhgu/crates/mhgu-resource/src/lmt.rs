//! Switch rMotionList (LMT v67); empty slots retain a null offset.
use crate::binary::Fields;
use anyhow::{Context, Result, ensure};
use glam::Vec4;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Usage {
    Rotation = 0,
    Translation = 1,
    Scale = 2,
    RootTranslation = 4,
}

pub struct Track {
    pub bone: u8,
    pub usage: Usage,
    /// One reference sample (constant), or a value for each integer frame.
    pub samples: Vec<Vec4>,
}

pub struct Writer {
    bytes: Vec<u8>,
    slots: u16,
}
impl Writer {
    pub fn new(count: usize) -> Result<Self> {
        let slots = u16::try_from(count)?;
        let mut bytes = b"LMT\0".to_vec();
        bytes.u16(67);
        bytes.u16(slots);
        bytes.resize(8 + 4 * count, 0);
        Ok(Self { bytes, slots })
    }
    /// Keep the native directory and all untouched clips while replacing selected slots.
    pub fn from_existing(bytes: Vec<u8>) -> Result<Self> {
        ensure!(bytes.starts_with(b"LMT\0C\0"), "expected LMT v67");
        let slots = u16::from_le_bytes(bytes.get(6..8).context("short LMT header")?.try_into()?);
        ensure!(
            bytes.len() >= 8 + 4 * usize::from(slots),
            "short LMT slot table"
        );
        Ok(Self { bytes, slots })
    }
    pub fn empty_slots(&self) -> impl Iterator<Item = usize> + '_ {
        (0..usize::from(self.slots))
            .filter(|&slot| self.bytes[8 + 4 * slot..12 + 4 * slot] == [0; 4])
    }
    /// Append one non-looping clip with empty event groups.
    pub fn clip(&mut self, slot: usize, frames: u32, tracks: &[Track]) -> Result<()> {
        ensure!(slot < usize::from(self.slots), "missing LMT slot {slot}");
        self.clip_with_loop(slot, frames, tracks, None)
    }
    pub fn clip_with_loop(
        &mut self,
        slot: usize,
        frames: u32,
        tracks: &[Track],
        loop_frame: Option<u32>,
    ) -> Result<()> {
        ensure!(slot < usize::from(self.slots), "missing LMT slot {slot}");
        ensure!(
            loop_frame.is_none_or(|n| n < frames),
            "loop point outside motion"
        );
        put_clip(&mut self.bytes, slot, frames, tracks, loop_frame)
    }
    pub fn finish(mut self) -> Vec<u8> {
        self.bytes.align(16);
        self.bytes
    }

    /// Set a single native hunter equipment event while this clip is playing.
    /// The final sample clears the event, just as in retail w00 slot 1.
    pub fn equipped_event(&mut self, slot: usize, kind: u16) -> Result<()> {
        self.equipment_timeline(slot, &[kind], &[(1, 0)])
    }

    /// Group zero controls the hunter's equipment sockets. Runs give durations;
    /// the final active run occupies the rest of the clip (except its reset frame).
    pub fn equipment_timeline(
        &mut self,
        slot: usize,
        kinds: &[u16],
        runs: &[(u32, u32)],
    ) -> Result<()> {
        ensure!(
            slot < self.slots as usize,
            "equipment event on nonexistent slot"
        );
        let word = |at| u32::from_le_bytes(self.bytes[at..at + 4].try_into().unwrap());
        let clip = word(8 + 4 * slot) as usize;
        ensure!(clip != 0, "equipment event on empty clip");
        let frames = word(clip + 8);
        let group = word(clip + 52) as usize;
        ensure!(
            frames > 1 && word(group + 64) == 1,
            "expected a baked clip without events"
        );
        ensure!(
            !runs.is_empty() && kinds.len() <= 32,
            "invalid equipment timeline"
        );
        for (i, kind) in kinds.iter().enumerate() {
            self.bytes[group + 2 * i..group + 2 * i + 2].copy_from_slice(&kind.to_le_bytes());
        }
        self.bytes.patch(group + 64, u32::try_from(runs.len() + 1)?);
        self.bytes.align(4);
        self.bytes
            .patch(group + 68, u32::try_from(self.bytes.len())?);
        let mut remaining = frames - 1;
        for (i, &(mask, len)) in runs.iter().enumerate() {
            let len = if i + 1 == runs.len() && len == 0 {
                remaining
            } else {
                len
            };
            ensure!(len > 0 && len <= remaining, "invalid equipment duration");
            self.bytes.u32(mask);
            self.bytes.u32(len);
            remaining -= len;
        }
        ensure!(remaining == 0, "incomplete equipment timeline");
        self.bytes.u32(0);
        self.bytes.u32(1);
        Ok(())
    }
}

fn pack_rotation(q: Vec4, delta: u8) -> u64 {
    let bits = |c: f32| {
        let n = (c.abs() * 4095.75).round() as u64;
        if c < 0. { 0x3fff - n } else { n }
    };
    (u64::from(delta) << 56) | (bits(q.x) << 42) | (bits(q.y) << 28) | (bits(q.z) << 14) | bits(q.w)
}

fn put_track(out: &mut Vec<u8>, t: usize, track: &Track) -> Result<()> {
    let points = &track.samples;
    ensure!(!points.is_empty(), "LMT track has no reference sample");
    let rot = track.usage == Usage::Rotation;
    let ty = match (points.len() == 1, rot) {
        (true, false) => 1,
        (true, true) => 2,
        (false, false) => 4,
        (false, true) => 6,
    };
    out[t..t + 4].copy_from_slice(&[ty, track.usage as u8, 0, track.bone]);
    out.patch(t + 4, 1f32.to_bits());
    for (i, f) in points[0].to_array().into_iter().enumerate() {
        out.patch(t + 16 + 4 * i, f.to_bits());
    }
    if points.len() == 1 {
        return Ok(());
    }
    let (mut low, mut high) = (points[0], points[0]);
    if !rot {
        for &p in points {
            low = low.min(p);
            high = high.max(p);
        }
        out.align(4);
        out.patch(t + 32, u32::try_from(out.len())?);
        out.floats((high - low).to_array());
        out.floats(low.to_array());
    }
    out.align(4);
    out.patch(t + 12, u32::try_from(out.len())?);
    out.patch(t + 8, u32::try_from(8 * points.len())?);
    for (i, &p) in points.iter().enumerate() {
        let delta = u8::from(i + 1 < points.len());
        if rot {
            out.extend(pack_rotation(p, delta).to_le_bytes());
        } else {
            for k in 0..3 {
                let d = high[k] - low[k];
                out.u16(if d == 0. {
                    0
                } else {
                    ((p[k] - low[k]) / d * 65535.).round() as u16
                });
            }
            out.u16(delta.into());
        }
    }
    Ok(())
}

fn put_clip(
    out: &mut Vec<u8>,
    slot: usize,
    frames: u32,
    tracks: &[Track],
    loop_frame: Option<u32>,
) -> Result<()> {
    out.align(16);
    let a = out.len();
    out.patch(8 + 4 * slot, u32::try_from(a)?);
    out.resize(a + 64, 0);
    let tab = out.len();
    out.patch(a, u32::try_from(tab)?);
    out.patch(a + 4, u32::try_from(tracks.len())?);
    out.patch(a + 8, frames);
    out.patch(a + 12, loop_frame.unwrap_or(u32::MAX));
    if let Some(last) = tracks
        .iter()
        .find(|t| t.usage == Usage::RootTranslation && t.bone == 255)
        .and_then(|t| t.samples.last())
    {
        for i in 0..3 {
            out.patch(a + 16 + 4 * i, last[i].to_bits());
        }
    }
    out.patch(a + 44, 1f32.to_bits());
    out.patch(a + 48, 0x00800000);
    out.resize(tab + 36 * tracks.len(), 0);
    for (i, tr) in tracks.iter().enumerate() {
        put_track(out, tab + 36 * i, tr)?;
    }
    // Same four event groups as a native no-event animation: zero event bits for its whole duration.
    out.align(4);
    let ev = out.len();
    out.patch(a + 52, u32::try_from(ev)?);
    out.resize(ev + 4 * 72, 0);
    for g in 0..4 {
        out.patch(ev + 72 * g + 64, 1);
        out.patch(ev + 72 * g + 68, u32::try_from(out.len())?);
        out.u32(0);
        out.u32(frames);
    }
    Ok(())
}
