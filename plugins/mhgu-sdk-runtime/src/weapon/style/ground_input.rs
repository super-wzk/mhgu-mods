//! Prevent an already buffered ground command from spending a style-owned button.
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use mhgu_sdk::weapon::style::{
    action::{
        self,
        command::native::{CAPACITY, buffer, table},
    },
    input::{Buttons, PadButton},
};

const B: u16 = PadButton::B as u16;
const Y: u16 = PadButton::Y as u16;
const SHOULDERS: u16 = PadButton::R as u16 | PadButton::Zl as u16 | PadButton::Zr as u16;
static OWNER: AtomicU32 = AtomicU32::new(0);
static BLOCKED: AtomicU32 = AtomicU32::new(0);
static CONTROLLER: AtomicU32 = AtomicU32::new(0);

pub(super) fn clear(actor: u32) {
    if OWNER.load(Relaxed) == actor {
        OWNER.store(0, Relaxed);
    }
}

pub(super) fn during_controller<T>(actor: u32, f: impl FnOnce() -> T) -> T {
    let previous = CONTROLLER.swap(actor, Relaxed);
    let result = f();
    CONTROLLER.store(previous, Relaxed);
    result
}

pub(super) unsafe fn blocked(actor: u32, group: u16, slot: u16) -> bool {
    group & 0x7fff == 1
        && slot < 32
        && OWNER.load(Relaxed) == actor
        && actor != 0
        && BLOCKED.load(Relaxed) & (1 << slot) != 0
        && CONTROLLER.load(Relaxed) != actor
        && unsafe { *((actor + 9493) as *const u8) & 4 == 0 }
}

pub(super) unsafe fn claim(actor: u32, before: u16, after: &Buttons) {
    let pass = after.held | after.pressed | after.released;
    let taken = before & !pass;
    let mut slots = 0;
    if taken & (SHOULDERS | Y) != 0 && pass & Y == 0 {
        slots |= (1 << 2) | (1 << 3);
    }
    if taken & (SHOULDERS | B) != 0 && pass & B == 0 {
        slots |= (1 << 6) | (1 << 7);
    }
    if slots == 0 || !unsafe { action::weapon_drawn(actor) } {
        clear(actor);
        return;
    }
    BLOCKED.store(slots, Relaxed);
    OWNER.store(actor, Relaxed);
    unsafe {
        let Some(rows) = table(actor).read().as_ref() else {
            return;
        };
        if rows.entries.is_null() || rows.count as usize > CAPACITY {
            return;
        }
        // 2E9D54 tests previously accepted rows even if this poll's predicate fails.
        for (i, latch) in (&mut *buffer(actor))[..rows.count as usize]
            .iter_mut()
            .enumerate()
        {
            if *latch == 0 {
                continue;
            }
            let row = rows.entries.add(i).read();
            if let Some(res) = row.as_ref().and_then(|r| r.resource.as_ref())
                && blocked(actor, res.command.group, res.command.slot)
            {
                *latch = 0;
            }
        }
    }
}
