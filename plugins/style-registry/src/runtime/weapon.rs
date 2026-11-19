//! GU weapon-specific entry points used by registered styles.
use super::{local_style, resolve_request};
use mhgu_sdk::style::{ActionRequest, Style, action};

fn great_sword_art(slot: u16) -> bool {
    // w00_m Hunter Art entries and their continuations in 1160A04.
    matches!(slot, 51..=57 | 62..=65 | 90 | 91)
}

pub(super) unsafe fn allows_native_art(actor: u32, style: &Style, slot: u16, flags: u16) -> bool {
    if style.art_slots == 0 {
        return false;
    }
    if flags & 0x40 != 0 {
        return true;
    }
    if style.weapon != 0 || !great_sword_art(slot) {
        return false;
    }
    let (group, current) = unsafe { action::current(actor) };
    group == action::GROUP && great_sword_art(current)
}

#[forge::hook(offset = 0x1160650)]
unsafe fn great_sword_change(actor: u32, group: u32, slot: u32, flags: u32) -> u32 {
    unsafe {
        if let Some(style) = local_style(actor).filter(|s| s.independent && s.controller.is_some())
        {
            if group == u32::from(action::GROUP)
                && great_sword_art(slot as u16)
                && allows_native_art(actor, style, slot as u16, flags as u16)
            {
                return original!(actor, group, slot, flags);
            }
            // A custom move can wind up, change clips, or be cancelled before
            // it creates a hit. Its first strike drives the weapon boost.
            let change: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
                core::mem::transmute(forge::mem::text_addr() + 0x2ab670);
            return change(actor, group, slot, flags);
        }
    }
    original!(actor, group, slot, flags)
}

// The draw route is returned by reference and read by GU after this call.
static DRAW_ACTIONS: [[u16; 3]; 256] = {
    let mut routes = [[0; 3]; 256];
    let mut id = 0;
    while id < routes.len() {
        routes[id] = [action::GROUP, action::slot(id as u8), 0];
        id += 1;
    }
    routes
};

#[forge::hook(offset = 0x115fcf4)]
unsafe fn great_sword_draw_route(actor: u32, kind: u32) -> u32 {
    let native = original!(actor, kind);
    unsafe {
        // Routes 2 and 4 explicitly request draw-guard. Both can carry X+A
        // in the resolved weapon commands.
        if matches!(kind, 0 | 1 | 3)
            && let Some(style) = local_style(actor).filter(|s| s.controller.is_some())
            && let Some(&[group, slot, flags]) = (native as *const [u16; 3]).as_ref()
            && let Some(resolved) = resolve_request(
                actor,
                style,
                ActionRequest {
                    group,
                    slot,
                    flags,
                    draw_route: true,
                },
            )
        {
            return DRAW_ACTIONS[usize::from(resolved - action::FIRST)].as_ptr() as u32;
        }
    }
    native
}

/// Arm and consume GS's next-strike state around the native hit constructor
/// and weapon adjustment. Empty motion phases and interrupted windups keep it.
pub(super) unsafe fn strike(actor: u32, style: &Style, flags: u32, submit: impl FnOnce(u32)) {
    unsafe {
        let boost = (actor + 13096) as *mut u32;
        if style.weapon == 0 && style.art_slots != 0 && matches!(boost.read(), 1..=3) {
            boost.write(3);
            submit(flags | 0x20);
            let clear: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(forge::mem::text_addr() + 0x116893c);
            clear(actor, 1);
        } else {
            submit(flags);
        }
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, great_sword_change);
    forge::install_hook!(base, great_sword_draw_route);
}
