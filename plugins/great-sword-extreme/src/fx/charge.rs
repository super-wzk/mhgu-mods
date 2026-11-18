//! Sound and particle events for one uninterrupted charge.
use super::{Attachment, CHARGE_ROUTE, route};

pub(crate) struct ChargeEffect {
    level: u8,
    played_sustain: bool,
}

impl ChargeEffect {
    pub(crate) unsafe fn start(p: u32) -> Self {
        unsafe {
            crate::sound::charge(p, 28);
            route(p, CHARGE_ROUTE, Attachment::Follow(2));
        }
        Self {
            level: 0,
            played_sustain: false,
        }
    }

    pub(crate) unsafe fn update(&mut self, p: u32, ticks: f32, level: u8) {
        unsafe {
            if ticks >= 45. && !self.played_sustain {
                self.played_sustain = true;
                crate::sound::charge(p, 29);
            }
            if level > self.level && (1..=4).contains(&level) {
                route(p, CHARGE_ROUTE + usize::from(level), Attachment::Follow(2));
                crate::sound::charge(p, 13);
            }
            self.level = level;
        }
    }
}

impl Drop for ChargeEffect {
    fn drop(&mut self) {
        super::stop_charge();
    }
}
