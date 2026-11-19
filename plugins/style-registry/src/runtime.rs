//! Shared motion hooks and equipment-box selection. GU's equipment byte stays native.
use alloc::{boxed::Box, ffi::CString, format};
use core::{
    ffi::{CStr, c_char},
    ptr,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};
mod attack;
mod commands;
mod effect;
mod guard;
mod menu;
mod weapon;
use mhgu_sdk::style::{
    ActionRequest, Input, PASS, Style, action,
    registry::{self, ServiceV1},
};

struct Entry {
    style: &'static Style,
    name: CString,
    details: CString,
    next: *mut Entry,
}

// Published entries live until exit. Zero selections skip the list entirely.
static HEAD: AtomicPtr<Entry> = AtomicPtr::new(ptr::null_mut());
static REQUESTED: [AtomicU32; crate::config::WEAPONS] =
    [const { AtomicU32::new(0) }; crate::config::WEAPONS];

unsafe extern "C" fn register(
    raw: *const Style,
    name: *const c_char,
    details: *const c_char,
) -> u32 {
    let Some(style) = (unsafe { raw.as_ref() }) else {
        return 1;
    };
    if !(1..=65535).contains(&style.id)
        || style.weapon as usize >= REQUESTED.len()
        || style.art_slots > 3
    {
        return 1;
    }
    let mut head = HEAD.load(Ordering::Acquire);
    loop {
        let mut cur = head;
        while let Some(entry) = unsafe { cur.as_ref() } {
            if entry.style.id == style.id {
                return if ptr::eq(entry.style, style) { 0 } else { 2 };
            }
            cur = entry.next;
        }
        let name = if name.is_null() {
            CString::new(format!("Style {}", style.id)).unwrap()
        } else {
            unsafe { CStr::from_ptr(name) }.into()
        };
        let details = if details.is_null() {
            CString::default()
        } else {
            unsafe { CStr::from_ptr(details) }.into()
        };
        let node = Box::into_raw(Box::new(Entry {
            style,
            name,
            details,
            next: head,
        }));
        match HEAD.compare_exchange(head, node, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return 0,
            Err(current) => {
                drop(unsafe { Box::from_raw(node) });
                head = current;
            }
        }
    }
}

fn entries(weapon: u32) -> impl Iterator<Item = &'static Entry> {
    let first = unsafe { HEAD.load(Ordering::Acquire).as_ref() };
    core::iter::successors(first, |e| unsafe { e.next.as_ref() })
        .filter(move |e| e.style.weapon == weapon)
}

fn choose(weapon: u32, entry: Option<&Entry>) {
    let Some(requested) = REQUESTED.get(weapon as usize) else {
        return;
    };
    let id = entry.map_or(0, |e| e.style.id);
    requested.store(id, Ordering::Release);
    let selection = core::array::from_fn(|i| REQUESTED[i].load(Ordering::Acquire) as u16);
    if !crate::config::save(selection) {
        forge::log::prelude::warn!("Style {} selected for this session; INI write failed", id);
    }
}

fn active(weapon: u32) -> Option<&'static Entry> {
    let id = REQUESTED.get(weapon as usize)?.load(Ordering::Acquire);
    if id == 0 {
        return None;
    }
    entries(weapon).find(|entry| entry.style.id == id)
}

extern "C" fn selected(weapon: u32) -> u32 {
    active(weapon).map_or(0, |e| e.style.id)
}

unsafe fn local_hunter() -> u32 {
    let base = forge::mem::text_addr();
    unsafe {
        let mgr = *((base + 0x1885b58) as *const u32);
        if mgr == 0 {
            return 0;
        }
        let local: unsafe extern "C" fn(u32) -> u32 = core::mem::transmute(base + 0x276278);
        local(mgr)
    }
}

unsafe fn actor_entry(actor: u32) -> Option<&'static Entry> {
    if actor == 0 {
        return None;
    }
    unsafe {
        let equipment = ((actor + 5060) as *const u32).read();
        if equipment == 0 {
            return None;
        }
        active(u32::from(((equipment + 1236) as *const u8).read()))
    }
}

unsafe fn local_style(actor: u32) -> Option<&'static Style> {
    if actor == 0 || unsafe { local_hunter() } != actor {
        return None;
    }
    unsafe { actor_entry(actor) }.map(|e| e.style)
}

unsafe fn independent(actor: u32) -> bool {
    unsafe { local_style(actor) }.is_some_and(|style| style.independent)
}

#[forge::hook(offset = 0x27c6a0)]
unsafe fn encode(actor: u32, request: u32) -> u32 {
    if let Some(f) = unsafe { local_style(actor) }.and_then(|s| s.encode) {
        let out = unsafe { f(actor, request) };
        if out != PASS {
            return out;
        }
    }
    original!(actor, request)
}

#[forge::hook(offset = 0x27c6d4)]
unsafe fn decode(actor: u32, packed: u32) -> u32 {
    if let Some(f) = unsafe { local_style(actor) }.and_then(|s| s.decode) {
        let out = unsafe { f(actor, packed) };
        if out != PASS {
            return out;
        }
    }
    original!(actor, packed)
}

static API: ServiceV1 = ServiceV1 {
    register,
    selected,
    attack: attack::submit,
    take_guard: guard::take,
};
static LAST_ENTRY: AtomicPtr<Entry> = AtomicPtr::new(ptr::null_mut());
static LAST_ACTOR: AtomicU32 = AtomicU32::new(0);

unsafe fn controller_frame(actor: u32, custom_id: Option<u8>) -> u32 {
    unsafe {
        if actor == 0 || actor != local_hunter() {
            return PASS;
        }
        let selected = actor_entry(actor);
        if let Some(id) = custom_id
            && !selected.is_some_and(|entry| entry.style.actions.contains(id))
        {
            return PASS;
        }
        let controller =
            selected.and_then(|entry| entry.style.controller.map(|tick| (entry, tick)));
        if controller.is_none() && LAST_ENTRY.load(Ordering::Relaxed).is_null() {
            return PASS;
        }
        let entry = controller.map_or(ptr::null_mut(), |(entry, _)| {
            ptr::from_ref(entry).cast_mut()
        });
        let prev = LAST_ENTRY.swap(entry, Ordering::Relaxed);
        let owner = LAST_ACTOR.swap(actor, Ordering::Relaxed);
        if prev != entry || actor != owner {
            guard::clear(owner);
            if let Some(stop) = prev.as_ref().and_then(|e| e.style.controller) {
                // A previous scene's actor may already have been destroyed.
                stop(if owner == actor { owner } else { 0 }, ptr::null());
            }
        }
        let Some((_, tick)) = controller else {
            return PASS;
        };
        let (group, slot) = action::current(actor);
        if action::custom(group, slot) != custom_id.is_some() {
            return PASS;
        }
        // Keep the native dispatch prologue, including priority for a forced
        // action (damage, scene transitions), before handing control to a style.
        if *((actor + 9493) as *const u8) & 4 != 0 {
            return PASS;
        }
        *((actor + 9472) as *mut u32) &= !1;
        let auxiliary: unsafe extern "C" fn() -> u32 =
            core::mem::transmute(forge::mem::text_addr() + 0x65fa04);
        let input = Input {
            common: *((actor + 5140) as *const [u32; 4]),
            weapon: *((actor + 5156) as *const [u32; 4]),
            stick_angle: *((actor + 5132) as *const u16),
            stick_strength: *((actor + 5136) as *const i16),
            auxiliary_pressed: u32::from(*((actor + 5112) as *const u16)) & auxiliary(),
        };
        let result = tick(actor, &input);
        if result == PASS {
            let (group, slot) = action::current(actor);
            if action::custom(group, slot) {
                // Keep nested dispatch away from the stock weapon tables.
                return 0;
            }
        }
        result
    }
}
#[forge::hook(offset = 0x282da8)]
unsafe fn dispatch(actor: u32) -> u32 {
    let result = unsafe { crate::runtime::controller_frame(actor, None) };
    if result != mhgu_sdk::style::PASS {
        return result;
    }
    original!(actor)
}

#[forge::hook(offset = 0x2b8178)]
unsafe fn weapon_dispatch(actor: u32) -> u32 {
    let (group, slot) = unsafe { action::current(actor) };
    let Some(id) = action::custom_id(group, slot) else {
        return original!(actor);
    };
    match unsafe { controller_frame(actor, Some(id)) } {
        // Custom IDs have no entry in the retail weapon tables.
        PASS => 0,
        result => result,
    }
}

#[forge::hook(offset = 0x2ab670)]
unsafe fn change_action(actor: u32, group: u32, slot: u32, flags: u32) -> u32 {
    // A registered controller owns the weapon's move selection. Shared GU
    // movement, guarding, damage and interactions remain available, but stock
    // weapon requests must not change state before the new style consumes GU's
    // resolved command buffer. Custom slots use the same native lifecycle.
    let slot = if group == u32::from(action::GROUP) {
        match unsafe { resolve_weapon_slot(actor, slot as u16, flags as u16) } {
            Some(slot) => u32::from(slot),
            None => return 0,
        }
    } else {
        slot
    };
    let out = original!(actor, group, slot, flags);
    unsafe { guard::after_change(actor, group, slot) };
    out
}

unsafe fn resolve_weapon_slot(actor: u32, slot: u16, flags: u16) -> Option<u16> {
    let style = unsafe { local_style(actor) };
    if let Some(id) = action::custom_id(action::GROUP, slot) {
        return style?.actions.contains(id).then_some(slot);
    }
    let Some(style) = style.filter(|style| style.controller.is_some()) else {
        return Some(slot);
    };
    if unsafe { weapon::allows_native_art(actor, style, slot, flags) } {
        return Some(slot);
    }
    unsafe { weapon::resolve_stock_slot(actor, style, slot, flags) }
}

unsafe fn resolve_request(actor: u32, style: &Style, req: ActionRequest) -> Option<u16> {
    let id = unsafe { (style.request?)(actor, &req) };
    if id == mhgu_sdk::style::NATIVE_ACTION && !req.draw_route {
        return Some(req.slot);
    }
    let id = u8::try_from(id).ok()?;
    style.actions.contains(id).then(|| action::slot(id))
}
static NATIVE_MENU: AtomicU32 = AtomicU32::new(0);
const ARTS_MENU: u32 = 2;
fn native_menu<R>(configure_arts: bool, call: impl FnOnce() -> R) -> R {
    let mode = if configure_arts { ARTS_MENU } else { 1 };
    let previous = NATIVE_MENU.swap(mode, Ordering::AcqRel);
    let result = call();
    NATIVE_MENU.store(previous, Ordering::Release);
    result
}
#[forge::hook(offset = 0x268818)]
unsafe fn game_style(actor: u32) -> u32 {
    if crate::runtime::NATIVE_MENU.load(core::sync::atomic::Ordering::Acquire) == 0
        && unsafe { crate::runtime::independent(actor) }
    {
        // This is only the fixed-size engine table index. Custom style IDs are
        // separate; controller-owned weapon actions never enter the stock table.
        return 0;
    }
    original!(actor)
}
#[forge::hook(offset = 0x2a26c8)]
unsafe fn art_ready(actor: u32, slot: u32) -> u32 {
    if unsafe { local_style(actor) }.is_some_and(|style| style.independent && style.art_slots == 0)
    {
        return 0;
    }
    original!(actor, slot)
}

// This native limit also serves preset previews. Override it only for the
// local hunter's equipment-box art selection.
#[forge::hook(offset = 0x2778ac)]
unsafe fn art_capacity(manager: u32, style: u32) -> u32 {
    if NATIVE_MENU.load(Ordering::Acquire) == ARTS_MENU
        && let Some(entry) = unsafe { actor_entry(local_hunter()) }
        && entry.style.art_slots != 0
    {
        return u32::from(entry.style.art_slots);
    }
    original!(manager, style)
}

// The Valor HUD factory (633D1C) uses game_style, but a widget which already
// exists reads only the local gauge. Mask its render bit, leaving the layout's
// requested visibility intact so normal HUD hiding still wins on restoration.
static HIDDEN_VALOR_NODE: AtomicU32 = AtomicU32::new(0);
#[forge::hook(offset = 0x639bd0)]
unsafe fn valor_hud(widget: u32) -> u32 {
    let out = original!(widget);
    unsafe { crate::runtime::update_valor_node(widget) };
    out
}

unsafe fn update_valor_node(widget: u32) {
    unsafe {
        let hide = actor_entry(local_hunter()).is_some_and(|entry| entry.style.independent);
        if !hide && HIDDEN_VALOR_NODE.load(Ordering::Relaxed) == 0 {
            return;
        }
        let layout = *((widget + 36) as *const u32);
        if layout == 0 {
            return;
        }
        let id = *((widget + 188) as *const u32);
        let n = *((layout + 664) as *const u32);
        let (table, index) = if id < n { (248, id) } else { (660, id - n) };
        let table = *((layout + table) as *const u32);
        if table == 0 {
            return;
        }
        let node = *((table + 4 * index) as *const u32);
        if node == 0 {
            return;
        }
        let visible: unsafe extern "C" fn(u32, u32) =
            core::mem::transmute(forge::mem::text_addr() + 0xa33e88);
        let flags = *((node + 84) as *const u32);
        if hide {
            if flags & 1 != 0 {
                HIDDEN_VALOR_NODE.store(node, Ordering::Relaxed);
                visible(node, 0);
            }
        } else if HIDDEN_VALOR_NODE.swap(0, Ordering::Relaxed) == node && flags & 2 != 0 {
            visible(node, 1);
        }
    }
}

#[forge::entry]
fn start() {
    for (requested, id) in REQUESTED.iter().zip(crate::config::load()) {
        requested.store(id.into(), Ordering::Release);
    }
    if !unsafe { registry::publish(&API) } {
        return;
    }
    let base = forge::mem::text_addr();
    forge::install_hook!(base, encode);
    forge::install_hook!(base, decode);
    forge::install_hook!(base, game_style);
    forge::install_hook!(base, art_ready);
    forge::install_hook!(base, art_capacity);
    forge::install_hook!(base, valor_hud);
    forge::install_hook!(base, dispatch);
    forge::install_hook!(base, weapon_dispatch);
    forge::install_hook!(base, change_action);
    attack::install(base);
    commands::install(base);
    effect::install(base);
    guard::install(base);
    weapon::install(base);
    menu::install(base);
}
