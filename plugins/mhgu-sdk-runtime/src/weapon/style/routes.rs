//! GU weapon-specific entry points used by registered styles.
use super::{local_style, resolve_request};
use mhgu_sdk::weapon::style::{ActionRequest, Style, action, rules::GroundMotion};

fn great_sword_art(slot: u16) -> bool {
    // w00_m Hunter Art entries and their continuations in 1160A04.
    matches!(slot, 51..=57 | 62..=65 | 90 | 91)
}

pub(super) unsafe fn finish_ground_motion(
    actor: u32,
    style: &Style,
    slot: u16,
    finish: GroundMotion,
) {
    unsafe {
        let packed = *((actor + 1204) as *const u16);
        if !(0x800..0x1000).contains(&packed) {
            return;
        }
        let Some(decode) = style.decode else { return };
        let phase = *((actor + 5) as *const u8);
        // 2CD978, 2CDB38 and 2CF8A4 wait on 950BCC for these phases.
        if finish(slot, phase, decode(actor, packed.into())) {
            action::signal_stopped_motion(actor);
        }
    }
}

fn weapon_art(weapon: u32, slot: u16) -> bool {
    match weapon {
        0 => great_sword_art(slot),
        // w01_m art starters and continuations dispatched by 1169A7C.
        1 => matches!(slot, 29..=32 | 40..=45 | 51..=54),
        // w07_m's four art starters and 11998B0's follow-up handlers.
        7 => matches!(slot, 51..=62 | 90),
        // w11_m's four art groups and the continuations in 11BB0F8.
        11 => matches!(slot, 39..=43 | 45..=56 | 76..=83 | 86..=93),
        _ => false,
    }
}

pub(super) unsafe fn allows_native_art(actor: u32, style: &Style, slot: u16, flags: u16) -> bool {
    if style.art_slots == 0 {
        return false;
    }
    if flags & 0x40 != 0 {
        return true;
    }
    // w07 119C738 and 119C9C8 issue the Sakura Slash follow-ups
    // without the activation flag. Its counter also requests 16:61
    // through GU's hit reaction path, which can change the current group.
    if style.weapon == 7 && matches!(slot, 54 | 61 | 62) {
        return true;
    }
    if !weapon_art(style.weapon, slot) {
        return false;
    }
    let (group, current) = unsafe { action::current(actor) };
    group == action::GROUP && weapon_art(style.weapon, current)
}

pub(super) unsafe fn resolve_stock_slot(
    actor: u32,
    style: &Style,
    slot: u16,
    flags: u16,
) -> Option<u16> {
    if unsafe { shared_weapon_slot(actor, slot) } {
        return Some(slot);
    }
    unsafe {
        resolve_request(
            actor,
            style,
            ActionRequest {
                group: action::GROUP,
                slot,
                flags,
                draw_route: false,
            },
        )
    }
}

pub(super) unsafe fn shared_weapon_slot(actor: u32, slot: u16) -> bool {
    // GU's shared weapon dispatcher (2B81A0) owns the kick and mounted
    // knife attacks. Drawn special input still belongs to the style.
    matches!(slot, 803..=805 | 810)
        || (slot == 800 && unsafe { *((actor + 10057) as *const u8) & 1 == 0 })
}

unsafe fn keeps_native_slot(actor: u32, style: &Style, slot: u16, flags: u16) -> bool {
    slot < action::FIRST && unsafe { resolve_stock_slot(actor, style, slot, flags) } == Some(slot)
}

#[forge::hook(offset = 0x1160650)]
unsafe fn great_sword_change(actor: u32, group: u32, slot: u32, flags: u32) -> u32 {
    unsafe {
        if let Some(style) =
            local_style(actor).filter(|s| s.weapon == 0 && s.independent && s.controller.is_some())
        {
            let (group_id, slot_id, flag_bits) = (group as u16, slot as u16, flags as u16);
            let entering_native = group_id == action::GROUP
                && ((great_sword_art(slot_id)
                    && allows_native_art(actor, style, slot_id, flag_bits))
                    || keeps_native_slot(actor, style, slot_id, flag_bits));
            let (current_group, current_slot) = action::current(actor);
            // An opted-in native action also gets its weapon-specific cleanup
            // when GU transitions to a custom attack or shared movement.
            let leaving_native = (group_id != action::GROUP
                || action::custom_id(group_id, slot_id)
                    .is_some_and(|id| style.actions.contains(id)))
                && current_group == action::GROUP
                && keeps_native_slot(actor, style, current_slot, 0);
            if entering_native || leaving_native {
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

unsafe fn draw_route(actor: u32, native: u32, weapon: u32) -> u32 {
    unsafe {
        if let Some(style) =
            local_style(actor).filter(|s| s.weapon == weapon && s.controller.is_some())
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

#[forge::hook(offset = 0x115fcf4)]
unsafe fn great_sword_draw_route(actor: u32, kind: u32) -> u32 {
    let native = original!(actor, kind);
    // Routes 2 and 4 explicitly request draw-guard.
    if matches!(kind, 0 | 1 | 3) {
        return unsafe { draw_route(actor, native, 0) };
    }
    native
}

#[forge::hook(offset = 0x1169020)]
unsafe fn sword_shield_draw_route(actor: u32, kind: u32) -> u32 {
    let native = original!(actor, kind);
    if matches!(kind, 0 | 1 | 3) {
        unsafe { draw_route(actor, native, 1) }
    } else {
        native
    }
}

#[forge::hook(offset = 0x1198bb4)]
unsafe fn long_sword_draw_route(actor: u32, kind: u32) -> u32 {
    let native = original!(actor, kind);
    unsafe { draw_route(actor, native, 7) }
}

#[forge::hook(offset = 0x119fcf4)]
unsafe fn switch_axe_draw_route(actor: u32, kind: u32) -> u32 {
    let native = original!(actor, kind);
    if matches!(kind, 0 | 1 | 4) {
        unsafe { draw_route(actor, native, 8) }
    } else {
        native
    }
}

#[forge::hook(offset = 0x11ba2a0)]
unsafe fn dual_blades_draw_route(actor: u32, kind: u32) -> u32 {
    let native = original!(actor, kind);
    unsafe { draw_route(actor, native, 11) }
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
    forge::install_hook!(base, dual_blades_draw_route);
    forge::install_hook!(base, long_sword_draw_route);
    forge::install_hook!(base, sword_shield_draw_route);
    forge::install_hook!(base, switch_axe_draw_route);
}
