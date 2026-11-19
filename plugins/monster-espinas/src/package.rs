//! Plugin-owned rows appended to GU's native attack table (ESH5 hunt or ESP1 replay).
use super::{byte, f32at, is_espinas, u16at, u32at};
use espinas_behavior::{BattleView, CASES, ROUTE_WIDTH, Request};
use mhgu_sdk::monster::{Monster, attack::AttackTables, motion::MotionId};

const HUNT_TAG: u32 = u32::from_le_bytes(*b"ESH5");
const REPLAY_TAG: u32 = u32::from_le_bytes(*b"ESP1");

pub(super) fn row_id(index: u32) -> u32 {
    if index == 0 { 19 } else { 63 + index }
}

pub(super) struct Batch {
    pub first: u32,
    pub count: u32,
    pub capacity: u32,
    pub cuts: u32,
    pub world: u8,
    pub radius: f32,
}

#[derive(Clone, Copy)]
pub(super) struct Package<'a> {
    pub(super) tables: AttackTables<'a>,
    rows: u32,
    footer: u32,
}
impl<'a> Package<'a> {
    pub(super) fn read(monster: Monster<'a>) -> Option<Self> {
        if !is_espinas(monster) {
            return None;
        }
        let tables = monster.attack_tables()?;
        let count = tables.row_count();
        if count <= 64 {
            return None;
        }
        Some(Self {
            tables,
            rows: tables.row(0)?.as_raw(),
            footer: tables.row(count - 1)?.as_raw(),
        })
    }

    fn tag(self) -> u32 {
        unsafe { u32at(self.footer, 0) }
    }

    pub(super) unsafe fn batch(self, motion: MotionId, id: u32) -> Option<Batch> {
        match self.tag() {
            HUNT_TAG => unsafe { Directory { package: self }.batch(motion, id) },
            REPLAY_TAG if id == 19 && motion == MotionId::new(0, 4) => {
                let count = unsafe { u32at(self.footer, 4) };
                let cut_row = 63 + u64::from(count);
                if count == 0
                    || u64::from(self.tables.row_count() - 1)
                        != cut_row + u64::from(count.div_ceil(32))
                {
                    return None;
                }
                Some(Batch {
                    first: 0,
                    count,
                    capacity: count,
                    cuts: self.tables.row(cut_row as u32)?.as_raw(),
                    world: 0,
                    radius: 0.,
                })
            }
            _ => None,
        }
    }
}

pub(super) struct Directory<'a> {
    package: Package<'a>,
}

pub(super) enum FlightCurve {
    Vertical = 8,
    Forward = 12,
}

impl<'a> Directory<'a> {
    pub(super) fn read(monster: Monster<'a>) -> Option<Self> {
        let package = Package::read(monster)?;
        (package.tag() == HUNT_TAG).then_some(Self { package })
    }
    pub(super) unsafe fn row(&self, field: u32) -> u32 {
        unsafe { self.package.rows + 32 * u32at(self.package.footer, field) }
    }
    unsafe fn physiology(&self) -> u32 {
        unsafe { self.package.rows + 32 * u32::from(u16at(self.row(20), 30)) }
    }
    pub(super) unsafe fn anger_threshold(&self) -> u16 {
        unsafe { u16at(self.physiology(), 0) }
    }
    pub(super) unsafe fn enraged_motion_rate(&self) -> f32 {
        unsafe { f32at(self.physiology(), 4) }
    }
    pub(super) unsafe fn anger_gain_factor(&self, hp_percent: u64) -> f32 {
        let lane = 10 - hp_percent.min(100) as u32 / 10;
        unsafe { f32at(self.physiology(), 16 + 4 * lane) }
    }

    unsafe fn entries(&self) -> impl Iterator<Item = u32> {
        // Both the table start and the count are fixed for the duration of this lookup.
        let base = unsafe { self.row(12) };
        let count = unsafe { u32at(self.package.footer, 16) };
        (0..count).map(move |i| base + 32 * i)
    }
    unsafe fn motion(entry: u32) -> MotionId {
        MotionId::from_raw(unsafe { u16at(entry, 2) })
    }
    pub(super) unsafe fn playback(entry: u32) -> (MotionId, u8) {
        unsafe { (Self::motion(entry), byte(entry, 7)) }
    }
    pub(super) unsafe fn blend(&self, entry: u32, previous: MotionId) -> u8 {
        unsafe {
            let blend = byte(entry, 7);
            if blend != 0 {
                blend
            } else {
                self.entries()
                    .find(|&e| Self::motion(e) == previous)
                    .map_or(0, |e| byte(e, 29))
            }
        }
    }
    pub(super) unsafe fn lookup(&self, req: Request) -> Option<u32> {
        unsafe {
            self.entries()
                .find(|&e| byte(e, 0) == req.0 && byte(e, 1) == req.1)
        }
    }
    pub(super) unsafe fn contains_motion(&self, motion: MotionId) -> bool {
        unsafe { self.entries().any(|entry| Self::motion(entry) == motion) }
    }
    unsafe fn batch(&self, motion: MotionId, id: u32) -> Option<Batch> {
        unsafe {
            let mut capacity = 0;
            let mut matched = None;
            for entry in self.entries() {
                let count = u32::from(byte(entry, 6));
                capacity = capacity.max(count);
                if count == 0 || Self::motion(entry) != motion {
                    continue;
                }
                let first = u32::from(u16at(entry, 4));
                if row_id(first) == id {
                    matched = Some((entry, first, count));
                }
            }
            let (entry, first, count) = matched?;
            let world = if byte(entry, 0) == 3 && matches!(byte(entry, 1), 8 | 10) {
                byte(entry, 28)
            } else {
                0
            };
            Some(Batch {
                first,
                count,
                capacity,
                cuts: self.row(8),
                world,
                radius: f32at(entry, 24),
            })
        }
    }
    /// Slope of the first segment ending at or after this frame.
    pub(super) unsafe fn curve_slope(
        &self,
        entry: u32,
        field: FlightCurve,
        frame: f32,
    ) -> Option<f32> {
        unsafe {
            let curve = self.package.rows + 32 * u32at(entry, field as u32);
            for i in 1..u32at(curve, 0) {
                let point = curve + 4 + i * 8;
                let end = f32at(point, 0);
                if frame <= end {
                    let previous = point - 8;
                    return Some(
                        (f32at(point, 4) - f32at(previous, 4)) / (end - f32at(previous, 0)),
                    );
                }
            }
            None
        }
    }
    /// Forward displacement across two motion frames, including the authored loop point.
    pub(super) unsafe fn travel_delta(&self, entry: u32, previous: f32, frame: f32) -> f32 {
        unsafe {
            let samples = self.package.rows + 32 * u32at(entry, 8);
            let last = u32::from(u16at(entry, 12)) - 1;
            let sample = |i| f32at(samples, i * 4);
            let position = |t: f32| {
                let t = t.clamp(0., last as f32);
                let i = t as u32;
                let a = sample(i);
                a + (sample((i + 1).min(last)) - a) * (t - i as f32)
            };
            let mut delta = position(frame) - position(previous);
            if frame < previous {
                delta += position(last as f32) - position(f32::from(u16at(entry, 14)));
            }
            delta
        }
    }
    pub(super) unsafe fn routes(&self) -> &[u8] {
        unsafe {
            core::slice::from_raw_parts(
                self.row(24) as *const u8,
                u32at(self.package.footer, 28) as usize * ROUTE_WIDTH,
            )
        }
    }
    pub(super) unsafe fn policy(&self) -> BattleView<'_> {
        unsafe {
            let p = self.row(20);
            BattleView {
                thresholds: core::array::from_fn(|i| f32at(p, 4 * i as u32)),
                decisions: core::slice::from_raw_parts((p + 32) as *const u8, CASES),
                routes: self.routes(),
            }
        }
    }
}
