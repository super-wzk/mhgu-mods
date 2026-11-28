//! Use GU's equipped shell receiver, effects and delayed native hit requests.
use crate::{native, program, read, write};
use mhgu_sdk::weapon::style::{action, hunter};

const AMMO: u32 = 11138;
const MAX_AMMO: u32 = 11140;
const COOLDOWN: u32 = 13096;
const SHOT_QUEUE: u32 = 13100;

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global extreme_gunlance_fire",
    ".hidden extreme_gunlance_fire",
    "extreme_gunlance_fire:",
    "ldr ip,[sp]",
    "vmov s0,ip",
    "ldr ip,[sp,#4]",
    "bx ip",
);

unsafe extern "C" {
    fn extreme_gunlance_fire(
        actor: u32,
        kind: u32,
        variant: u32,
        delay: u32,
        gain: u32,
        address: u32,
    ) -> u32;
}

pub unsafe fn ammo(actor: u32) -> u8 {
    unsafe { read(actor, AMMO) }
}

pub unsafe fn capacity(actor: u32) -> u8 {
    unsafe { read(actor, MAX_AMMO) }
}

pub unsafe fn sync_ammo(actor: u32, count: u8) {
    unsafe { write(actor, AMMO, count.min(capacity(actor))) };
}

pub unsafe fn can_reload(actor: u32) -> bool {
    unsafe { ammo(actor) < capacity(actor) }
}

pub unsafe fn reload(actor: u32, full: bool) {
    unsafe {
        let add = if full { capacity(actor) } else { 1 };
        native!(0x2ad4f4, actor, u32::from(add));
    }
}

pub unsafe fn cooled(actor: u32) -> bool {
    unsafe { cooldown(actor) <= 0. }
}

pub unsafe fn cooldown(actor: u32) -> f32 {
    unsafe { read(actor, COOLDOWN) }
}

pub unsafe fn artillery_expert(actor: u32) -> bool {
    [175, 287]
        .into_iter()
        .any(|id| unsafe { hunter::has_skill(actor, id) })
}

unsafe fn set_wyvern_cooldown(actor: u32, duration: f32) {
    unsafe {
        let artillery = if artillery_expert(actor) {
            program::WYVERN_COOLDOWN_STRONG_REDUCTION
        } else if hunter::has_skill(actor, 176) {
            program::WYVERN_COOLDOWN_WEAK_REDUCTION
        } else {
            1.
        };
        let ticks = (duration * 0.5 * artillery) as u16;
        write(actor, COOLDOWN, 2. * f32::from(ticks));
    }
}

// AA Flare's final native action fires in phase 0. After GU has queued the
// shot, use the same timer as Extreme's regular Wyvern Fire.
#[forge::hook(offset = 0x11b0244)]
unsafe fn aa_flare(actor: u32) -> u32 {
    let starting = unsafe {
        crate::active(actor)
            && matches!(action::current(actor), (action::GROUP, 135..=137))
            && read::<u8>(actor, 5) == 0
    };
    let result = original!(actor);
    if starting && unsafe { read::<u8>(actor, 5) == 1 } {
        unsafe { set_wyvern_cooldown(actor, program::timer(program::WYVERN_COOLDOWN)) };
    }
    result
}

pub fn install() {
    forge::install_hook!(forge::mem::text_addr(), aa_flare);
}

pub unsafe fn shell_type(actor: u32) -> u32 {
    unsafe { native!(0x27c708, actor, 27) % 3 }
}

pub unsafe fn consume(actor: u32, limit: u8) -> u8 {
    unsafe {
        let count = ammo(actor).min(limit);
        if count != 0 {
            native!(0x2ad4f4, actor, (-i32::from(count)) as u32);
        }
        count
    }
}

pub unsafe fn available(actor: u32) -> bool {
    unsafe { (0..3).any(|index| read::<u8>(actor, SHOT_QUEUE + index) == 0) }
}

pub unsafe fn queue(actor: u32, kind: u32, variant: u32, gain: f32) -> bool {
    unsafe {
        let base = forge::mem::text_addr();
        if !available(actor) {
            return false;
        }
        extreme_gunlance_fire(actor, kind, variant, 0, gain.to_bits(), base + 0x11b0544);
        true
    }
}

pub unsafe fn feedback(actor: u32, kind: u32) {
    unsafe {
        native!(0x2a063c, actor, 17, 1024);
        native!(0x11b0c38, actor, kind.min(3));
    }
}

pub unsafe fn fire(actor: u32, kind: u32, variant: u32, gain: f32) -> bool {
    unsafe {
        if !queue(actor, kind, variant, gain) {
            return false;
        }
        feedback(actor, kind);
        true
    }
}
pub unsafe fn wyvern_fire(actor: u32, kind: u32, gain: f32, duration: Option<f32>) -> bool {
    unsafe {
        if !available(actor) {
            return false;
        }
        extreme_gunlance_fire(
            actor,
            kind,
            0,
            0,
            gain.to_bits(),
            forge::mem::text_addr() + 0x11b086c,
        );
        if let Some(duration) = duration {
            set_wyvern_cooldown(actor, duration);
        }
        true
    }
}
