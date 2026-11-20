//! Frontier action event and buffered branch records in authored motion frames.

use core::ops::RangeInclusive;
use mhgu_sdk::style::action::MotionFrame;

const fn half(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

#[derive(Clone, Copy)]
pub struct Step {
    pub op: u16,
    pub clip: u16,
    pub start: u16,
    pub blend: u16,
    pub until: u16,
    pub loops: u16,
}

impl Step {
    pub fn read(rows: &[u8], index: u8) -> Option<Self> {
        let row = rows.chunks_exact(12).nth(usize::from(index))?;
        Some(Self {
            op: half(row, 0),
            clip: half(row, 2),
            start: half(row, 4),
            blend: half(row, 6),
            until: half(row, 8),
            loops: half(row, 10),
        })
    }

    pub fn reached_frame(self, frame: f32) -> bool {
        frame >= f32::from(self.until)
    }

    /// Playback gate from the Frontier step dispatcher at 81815D60. An open
    /// step (op 5) is advanced by the weapon's native or input branch.
    pub fn complete(self, time: Time, sample: MotionFrame, wrapped: bool) -> bool {
        match self.op {
            3 => {
                let ended = sample.complete();
                (ended || wrapped) && time.completed_cycles(ended, wrapped) >= self.loops
            }
            4 => {
                time.completed_cycles(sample.ended, wrapped) >= self.loops
                    && (self.reached_frame(sample.frame) || sample.ended)
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Time {
    pub step: u8,
    pub cycles: u16,
    pub frame: f32,
    pub blend: f32,
}

impl Time {
    /// Record a motion sample for event and branch evaluation.
    pub fn observe(&mut self, frame: f32, blend: f32) -> bool {
        let wrapped = frame < self.frame;
        self.cycles = self.cycles.saturating_add(u16::from(wrapped));
        self.frame = frame;
        self.blend = blend;
        wrapped
    }

    /// GU can set the layer-end bit on the same tick as a visible loop wrap.
    fn completed_cycles(self, ended: bool, wrapped: bool) -> u16 {
        self.cycles.saturating_add(u16::from(ended && !wrapped))
    }

    fn reached(self, record: &[u8]) -> bool {
        let step = half(record, 0);
        if u16::from(self.step) != step {
            return u16::from(self.step) > step;
        }
        match record[2] {
            1 => true,
            0 if record[3] as i8 > 0 => self.blend <= f32::from(record[3]),
            0 => {
                let cycle = half(record, 6);
                self.cycles > cycle
                    || self.cycles == cycle && self.frame >= f32::from(half(record, 4))
            }
            _ => false,
        }
    }
}

pub struct BranchRules {
    pub order: &'static [u16],
    /// GU can resolve the chord after a plain X has already been queued.
    pub combo_override: Option<(RangeInclusive<u16>, RangeInclusive<u16>)>,
    pub art_commands: u32,
}

pub struct Cursor {
    pub state: u8,
    pub time: Time,
    events: &'static [u8],
    branches: &'static [u8],
    rules: &'static BranchRules,
    event: usize,
    pending: Option<usize>,
}

impl Cursor {
    pub fn new(
        state: u8,
        branches: &'static [u8],
        events: &'static [u8],
        rules: &'static BranchRules,
    ) -> Self {
        Self {
            state,
            time: Time::default(),
            events,
            branches,
            rules,
            event: 0,
            pending: None,
        }
    }

    pub fn next_event(&mut self) -> Option<(u16, u16, f32)> {
        let row = self.events.chunks_exact(12).nth(self.event)?;
        if !self.time.reached(row) {
            return None;
        }
        self.event += 1;
        let late = if half(row, 0) == u16::from(self.time.step)
            && row[2] == 0
            && row[3] == 0
            && half(row, 6) == self.time.cycles
        {
            (self.time.frame - f32::from(half(row, 4))).max(0.)
        } else {
            0.
        };
        Some((half(row, 8), half(row, 10), late))
    }

    pub fn branch(&mut self, commands: u32, eligible: impl Fn(u16) -> bool) -> Option<u16> {
        let table = self.branches;
        self.pending = self
            .pending
            .filter(|&at| !self.time.reached(&table[at + 32..]));
        let previous = self
            .pending
            .map(|at| (half(table, at), half(table, at + 4)));
        for &command in self.rules.order {
            if commands & (1 << command) == 0 {
                continue;
            }
            let candidate = table
                .chunks_exact(40)
                .enumerate()
                .filter(|(_, row)| {
                    half(row, 4) == command
                        && previous.is_none_or(|(priority, pending)| {
                            let next = half(row, 0);
                            next > priority
                                || (next == priority
                                    && self.rules.combo_override.as_ref().is_some_and(
                                        |(plain, chord)| {
                                            plain.contains(&pending) && chord.contains(&command)
                                        },
                                    ))
                        })
                        && eligible(half(row, 2))
                        && self.time.reached(&row[6..])
                        && !self.time.reached(&row[14..])
                })
                .max_by_key(|(_, row)| half(row, 0));
            if let Some((i, _)) = candidate {
                self.pending = Some(i * 40);
                break;
            }
        }
        let at = self.pending?;
        let row = &table[at..at + 40];
        if eligible(half(row, 2)) && self.time.reached(&row[24..]) && !self.time.reached(&row[32..])
        {
            // GU may defer the change during hitstop. The next action gets a
            // new cursor on acceptance, so keep this input until its window ends.
            Some(half(row, 22))
        } else {
            None
        }
    }

    pub fn pending_action(&self) -> Option<u16> {
        self.pending.map(|at| half(self.branches, at + 22))
    }

    pub fn art_window(&self, eligible: impl Fn(u16) -> bool) -> bool {
        self.branches.chunks_exact(40).any(|row| {
            self.rules
                .art_commands
                .checked_shr(u32::from(half(row, 4)))
                .unwrap_or(0)
                & 1
                != 0
                && eligible(half(row, 2))
                && self.time.reached(&row[24..])
                && !self.time.reached(&row[32..])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn branch_row(
        priority: u16,
        command: u16,
        action: u16,
        open: u16,
        close: u16,
    ) -> [u8; 40] {
        let mut row = [0; 40];
        let words = [
            (0, priority),
            (4, command),
            (18, close),
            (22, action),
            (28, open),
            (36, close),
        ];
        let mut i = 0;
        while i < words.len() {
            let (at, value) = words[i];
            let [low, high] = value.to_le_bytes();
            row[at] = low;
            row[at + 1] = high;
            i += 1;
        }
        row
    }

    #[test]
    fn rejected_transition_can_retry_and_expired_priority_releases_buffer() {
        static RULES: BranchRules = BranchRules {
            order: &[0, 1],
            combo_override: None,
            art_commands: 0,
        };
        static ROWS: [[u8; 40]; 2] = [branch_row(10, 0, 74, 8, 16), branch_row(1, 1, 20, 12, 30)];
        let mut cursor = Cursor::new(1, ROWS.as_flattened(), &[], &RULES);
        assert_eq!(cursor.branch(1, |_| true), None);
        assert_eq!(cursor.pending_action(), Some(74));
        cursor.time.observe(8., 0.);
        assert_eq!(cursor.branch(0, |_| true), Some(74));
        assert_eq!(cursor.branch(0, |_| true), Some(74));
        cursor.time.observe(16., 0.);
        assert_eq!(cursor.branch(2, |_| true), Some(20));
    }
}
