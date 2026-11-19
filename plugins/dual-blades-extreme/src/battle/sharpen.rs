//! Frontier blade-polish stacks and their bonus at GU's melee physical-attack lookup.
use super::{CONTACTS, OWNER, Relaxed, read};
use core::{ffi::c_void, sync::atomic::AtomicU32};

static LEVEL: AtomicU32 = AtomicU32::new(0);
static mut ORIGINAL: *const c_void = core::ptr::null();

#[derive(Default)]
pub(super) struct Polish {
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
            LEVEL.store(u32::from(self.stacks), Relaxed);
        }
    }

    pub fn activate(&mut self, duration: f32) {
        self.held = duration;
        self.remaining = duration;
        LEVEL.store(u32::from(self.stacks), Relaxed);
    }

    pub fn leave_action(&mut self) {
        self.held = 0.;
    }

    pub fn tick(&mut self, elapsed: f32, drawn: bool, in_attack_chain: bool) {
        let was_active = self.active();
        if drawn {
            self.remaining = (self.remaining - elapsed).max(self.held);
        } else {
            self.held = 0.;
            self.remaining = 0.;
        }
        if self.stacks != 0 && (!drawn || !in_attack_chain || (was_active && !self.active())) {
            self.stacks = 0;
            clear_active_level();
        }
    }
}

pub(super) fn clear_active_level() {
    LEVEL.store(0, Relaxed);
}

pub(super) fn active_level() -> u32 {
    LEVEL.load(Relaxed)
}

// GU returns its attack factor in s0. Delegate every other contact to its
// original lookup, including fixed-damage rows that don't use weapon raw.
core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global frontier_db_physical",
    ".hidden frontier_db_physical",
    ".hidden frontier_db_polish_factor",
    ".type frontier_db_physical,%function",
    "frontier_db_physical:",
    "push {r0,lr}",
    "bl frontier_db_polish_factor",
    "mov ip,r0",
    "cmp r1,#0",
    "pop {r0,lr}",
    "bxeq ip",
    "vmov s0,r1",
    "bx lr",
    options(raw),
);

unsafe extern "C" {
    fn frontier_db_physical();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn frontier_db_polish_factor(hit: u32) -> u64 {
    let actor = OWNER.load(Relaxed);
    let level = LEVEL.load(Relaxed);
    unsafe {
        if level != 0
            && actor != 0
            && CONTACTS.contains(&hit.wrapping_sub(actor))
            && read::<u8>(hit, 16) == 0
            && read::<u32>(hit, 12) == actor
            && read::<u32>(hit, 48) & 0x20 != 0
        {
            let equipment = read::<u32>(actor, 5060);
            if equipment != 0 {
                let raw = i32::from(read::<i16>(equipment, 800));
                if raw > 0 {
                    let boosted = raw * (100 + 5 * level as i32) / 100;
                    return u64::from((boosted as f32 / 100.).to_bits()) << 32;
                }
            }
        }
        ORIGINAL as u64
    }
}

pub(super) unsafe fn install(base: u32) {
    unsafe {
        forge::sys::hook::forge_hook_create(
            (base + 0x169548) as *const _,
            frontier_db_physical as *const _,
            &raw mut ORIGINAL,
        );
    }
}
