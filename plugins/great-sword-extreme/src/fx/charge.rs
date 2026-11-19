//! Sound and particle events for one uninterrupted charge.
use super::{Attachment, CHARGE_ROUTE, route_layers};

// The Iron Sword bank maps 128..=130 to distinct rise requests. Lv4 keeps
// the Frontier accent alongside the common fourth-charge cue.
const LEVEL_CUES: [u8; 4] = [128, 129, 130, 13];
const CHARGE_JOINT: u8 = 2;

pub(crate) struct ChargeEffect {
    level: u8,
    aura_pending: u32,
    flash_pending: u32,
    played_sustain: bool,
}

impl ChargeEffect {
    pub(crate) unsafe fn start(p: u32) -> Self {
        unsafe {
            crate::sound::charge(p, 28);
        }
        Self {
            level: 0,
            aura_pending: u32::MAX,
            flash_pending: 0,
            played_sustain: false,
        }
    }

    pub(crate) unsafe fn update(&mut self, p: u32, ticks: f32, level: u8) {
        unsafe {
            if self.aura_pending != 0 {
                self.aura_pending = route_layers(
                    p,
                    CHARGE_ROUTE,
                    Attachment::Follow(CHARGE_JOINT),
                    self.aura_pending,
                );
            }
            if ticks >= 45. && !self.played_sustain {
                self.played_sustain = true;
                crate::sound::charge(p, 29);
            }
            if level != self.level {
                self.flash_pending = 0;
                if level > self.level
                    && let Some(&cue) = LEVEL_CUES.get(usize::from(level - 1))
                {
                    self.flash_pending = u32::MAX;
                    crate::sound::charge(p, cue);
                }
                self.level = level;
            }
            if self.flash_pending != 0 {
                self.flash_pending = route_layers(
                    p,
                    CHARGE_ROUTE + usize::from(level),
                    Attachment::Follow(CHARGE_JOINT),
                    self.flash_pending,
                );
            }
        }
    }
}

impl Drop for ChargeEffect {
    fn drop(&mut self) {
        super::stop_charge();
    }
}
