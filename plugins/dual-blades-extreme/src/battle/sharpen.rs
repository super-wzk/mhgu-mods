//! Frontier blade-polish stacks and their bonus at GU's melee physical-attack lookup.
use super::{OWNER, Relaxed};
use core::sync::atomic::AtomicU32;
use mhgu_sdk::style::{
    action::melee,
    physical::{self, Rule, Scope},
};

static LEVEL: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];

#[derive(Default)]
pub(super) struct Polish {
    pub(super) slot: usize,
    stacks: u8,
    remaining: f32,
    held: f32,
}

impl Polish {
    pub fn active(&self) -> bool {
        self.remaining > 0.
    }

    pub fn add_stack(&mut self) {
        self.stacks = (self.stacks + 1).min(4);
        // Before the first activation the event only prepares a stack.
        if self.active() {
            LEVEL[self.slot].store(u32::from(self.stacks), Relaxed);
        }
    }

    pub fn activate(&mut self, duration: f32) {
        self.held = duration;
        self.remaining = duration;
        LEVEL[self.slot].store(u32::from(self.stacks), Relaxed);
    }

    pub fn leave_action(&mut self) {
        self.held = 0.;
    }

    pub unsafe fn tick(&mut self, actor: u32, elapsed: f32, drawn: bool, in_attack_chain: bool) {
        // 81820BD4 skips the entire blade-polish update during hitstop.
        if unsafe { melee::hitstop_active(actor) } {
            return;
        }
        if drawn {
            let next = (self.remaining - elapsed).max(self.held);
            self.remaining = if next == 0. && self.active() && in_attack_chain {
                // 8182000E restores one FZ tick when the timer expires in a weapon action.
                2.
            } else {
                next
            };
        } else {
            self.held = 0.;
            self.remaining = 0.;
        }
        if self.stacks != 0 && (!drawn || !in_attack_chain) {
            self.stacks = 0;
            LEVEL[self.slot].store(0, Relaxed);
        }
    }
}

pub(super) fn clear_active_level() {
    LEVEL[0].store(0, Relaxed);
}

pub(super) fn active_level() -> u32 {
    LEVEL[0].load(Relaxed)
}

unsafe extern "C" fn adjust(actor: u32, raw: i32) -> i32 {
    let slot = if OWNER.load(Relaxed) == actor {
        Some(0)
    } else {
        super::peer::slot_of(actor)
    };
    slot.map_or(raw, |slot| {
        raw * (100 + 5 * LEVEL[slot].load(Relaxed) as i32) / 100
    })
}

pub(super) unsafe fn install() {
    static POWER: Rule = Rule {
        scope: Scope::BladeSlots,
        adjust,
    };
    if !unsafe { physical::bind(crate::STYLE_ID, &POWER) } {
        forge::log::prelude::warn!("Extreme DB: physical power service unavailable");
    }
}

impl Drop for Polish {
    fn drop(&mut self) {
        LEVEL[self.slot].store(0, Relaxed);
    }
}
