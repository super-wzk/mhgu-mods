//! GU 169548 returns true weapon attack / 100 in s0.
use core::{ffi::c_void, sync::atomic::Ordering};
use mhgu_sdk::style::action::melee::BLADE_CONTACT_OFFSETS;
use mhgu_sdk::style::physical::{self, Rule, Scope};

static mut ORIGINAL: *const c_void = core::ptr::null();
static SERVICE: physical::Service = physical::Service(bind);
unsafe extern "C" fn bind(id: u32, rule: *const Rule) -> bool {
    let Some(entry) = super::registered_entry(id) else {
        return false;
    };
    entry.physical.store(rule.cast_mut(), Ordering::Release);
    true
}

core::arch::global_asm!(
    ".syntax unified",
    ".arm",
    ".fpu vfpv3",
    ".text",
    ".align 2",
    ".global mhgu_style_physical",
    ".hidden mhgu_style_physical",
    ".hidden mhgu_style_melee_factor",
    ".type mhgu_style_physical,%function",
    "mhgu_style_physical:",
    "push {r0,lr}",
    "bl mhgu_style_melee_factor",
    "mov ip,r0",
    "cmp r1,#0",
    "pop {r0,lr}",
    "bxeq ip",
    "vmov s0,r1",
    "bx lr",
    options(raw),
);
unsafe extern "C" {
    fn mhgu_style_physical();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn mhgu_style_melee_factor(hit: u32) -> u64 {
    unsafe {
        let owner = *((hit + 12) as *const u32);
        if *((hit + 16) as *const u8) == 0
            && *((hit + 48) as *const u32) & 0x100020 == 0x20
            && owner != 0
            && super::local_hunter() == owner
            && let Some(entry) = super::actor_entry(owner)
            && let Some(rule) = entry.physical.load(Ordering::Acquire).as_ref()
            && (matches!(rule.scope, Scope::WeaponContacts)
                || BLADE_CONTACT_OFFSETS.contains(&hit.wrapping_sub(owner)))
        {
            let eq = *((owner + 5060) as *const u32);
            let raw = i32::from(*((eq + 800) as *const i16));
            let power = (rule.adjust)(owner, raw);
            if raw > 0 && power != raw {
                return u64::from((power as f32 / 100.).to_bits()) << 32;
            }
        }
        ORIGINAL as u64
    }
}

pub(super) unsafe fn install(base: u32) {
    unsafe {
        if physical::publish(&SERVICE) {
            forge::sys::hook::forge_hook_create(
                (base + 0x169548) as *const _,
                mhgu_style_physical as *const _,
                &raw mut ORIGINAL,
            );
        }
    }
}
