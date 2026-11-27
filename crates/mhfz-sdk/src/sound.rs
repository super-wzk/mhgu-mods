//! Trigger cues from the converted Frontier sound timeline.
use mhgu_sdk::weapon::style::sound::{Bank, random_u32};

#[derive(Clone, Copy)]
pub struct Clock {
    clip: u16,
    frame: f32,
}

impl Clock {
    pub const fn new() -> Self {
        Self { clip: 0, frame: 0. }
    }

    /// Play authored cues crossed since the last sample. Each record contains
    /// a motion frame, bank number and up to four weighted cue alternatives.
    ///
    /// # Safety
    /// The actor must be a live hunter on GU's game thread.
    pub unsafe fn update(
        &mut self,
        actor: u32,
        banks: &[Bank],
        timeline: &[u8],
        clip: Option<u16>,
        frame: f32,
        first: Option<f32>,
    ) {
        let Some(clip @ 1000..1500) = clip else {
            self.clip = 0;
            return;
        };
        let from = if let Some(start) = first {
            start - 1.
        } else if self.clip != clip || frame < self.frame {
            frame - 1.
        } else {
            self.frame
        };
        self.clip = clip;
        self.frame = frame;
        unsafe { play_events(timeline, clip, |time| from < time && time <= frame, |bank, cue| {
            if let Some(bank) = banks.get(usize::from(bank)) { bank.play(actor, cue); }
        }) };
    }
}

/// Select the authored variant for each event in the sampled interval.
///
/// # Safety
/// Run on the game thread for access to the hunter random generator.
pub unsafe fn play_events(timeline: &[u8], clip: u16, contains: impl Fn(f32) -> bool, mut play: impl FnMut(u8, u8)) {
    if !(1000..1500).contains(&clip) { return; }
        let index = 4 * usize::from(clip - 1000);
        let Some(&[a, b, c, d]) = timeline.as_chunks::<4>().0.get(index / 4) else {
            return;
        };
        let offset = usize::from(u16::from_le_bytes([a, b]));
        let count = usize::from(u16::from_le_bytes([c, d]));
        let Some(track) = timeline.get(offset..offset + 12 * count) else {
            return;
        };
        for row in track.chunks_exact(12) {
            let time = f32::from(u16::from_le_bytes([row[0], row[1]]));
            if !contains(time) {
                continue;
            }
            let Some(choices) = row.get(4..4 + 2 * usize::from(row[3])) else {
                continue;
            };
            let total: u32 = choices.chunks_exact(2).map(|c| u32::from(c[1])).sum();
            if total == 0 {
                continue;
            }
            let mut roll = if choices.len() > 2 {
                (unsafe { random_u32() }) % total
            } else {
                0
            };
            for c in choices.chunks_exact(2) {
                if roll < u32::from(c[1]) {
                    if c[0] != 255 {
                        play(row[2], c[0]);
                    }
                    break;
                }
                roll -= u32::from(c[1]);
            }
        }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}
