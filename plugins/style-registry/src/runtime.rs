//! Shared motion hooks and equipment-box selection. GU's equipment byte stays native.
use alloc::{boxed::Box, ffi::CString, format};
use core::{
    ffi::{CStr, c_char},
    ptr,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};
mod menu;
use mhgu_sdk::style::{
    Controller, Input, PASS, Request, Style,
    registry::{self, ServiceV1},
};

struct Entry {
    style: &'static Style,
    name: CString,
    details: CString,
    independent: bool,
    controller: Option<Controller>,
    request: Option<Request>,
    next: *mut Entry,
}

// Published entries live until exit. The equipment box can enumerate the list;
// motion dispatch reads one cached pointer, with no allocation or lock.
static HEAD: AtomicPtr<Entry> = AtomicPtr::new(ptr::null_mut());
static REQUESTED: AtomicU32 = AtomicU32::new(0);
static ACTIVE: AtomicPtr<Entry> = AtomicPtr::new(ptr::null_mut());

unsafe extern "C" fn register(raw: *const Style) -> u32 {
    unsafe { register_named(raw, ptr::null(), ptr::null()) }
}

unsafe extern "C" fn register_named(
    raw: *const Style,
    name: *const c_char,
    details: *const c_char,
) -> u32 {
    unsafe { insert(raw, name, details, false, None, None) }
}
unsafe extern "C" fn register_independent(
    raw: *const Style,
    name: *const c_char,
    details: *const c_char,
) -> u32 {
    unsafe { insert(raw, name, details, true, None, None) }
}
unsafe extern "C" fn register_controller(
    raw: *const Style,
    name: *const c_char,
    details: *const c_char,
    controller: Controller,
    request: Option<Request>,
) -> u32 {
    unsafe { insert(raw, name, details, true, Some(controller), request) }
}
unsafe fn insert(
    raw: *const Style,
    name: *const c_char,
    details: *const c_char,
    independent: bool,
    controller: Option<Controller>,
    request: Option<Request>,
) -> u32 {
    let Some(style) = (unsafe { raw.as_ref() }) else {
        return 1;
    };
    if !(1..=65535).contains(&style.id) || style.weapon > 14 {
        return 1;
    }
    let mut head = HEAD.load(Ordering::Acquire);
    loop {
        let mut cur = head;
        while let Some(entry) = unsafe { cur.as_ref() } {
            if entry.style.id == style.id {
                return if ptr::eq(entry.style, style)
                    && entry.independent == independent
                    && entry.controller.map(|f| f as usize) == controller.map(|f| f as usize)
                    && entry.request.map(|f| f as usize) == request.map(|f| f as usize)
                {
                    0
                } else {
                    2
                };
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
            independent,
            controller,
            request,
            next: head,
        }));
        match HEAD.compare_exchange(head, node, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {
                if REQUESTED.load(Ordering::Acquire) == style.id {
                    ACTIVE.store(node, Ordering::Release);
                }
                return 0;
            }
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

fn choose(entry: Option<&Entry>) {
    let id = entry.map_or(0, |e| e.style.id);
    REQUESTED.store(id, Ordering::Release);
    ACTIVE.store(
        entry.map_or(ptr::null_mut(), |e| ptr::from_ref(e).cast_mut()),
        Ordering::Release,
    );
    if !crate::config::save(id as u16) {
        forge::log::prelude::warn!("Style {} selected for this session; INI write failed", id);
    }
}

fn active() -> Option<&'static Entry> {
    unsafe { ACTIVE.load(Ordering::Acquire).as_ref() }
}

extern "C" fn selected(weapon: u32) -> u32 {
    active()
        .filter(|e| e.style.weapon == weapon)
        .map_or(0, |e| e.style.id)
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

unsafe fn local_style(actor: u32) -> Option<&'static Style> {
    let style = active()?.style;
    unsafe {
        if actor == 0 || local_hunter() != actor {
            return None;
        }
        let eq = *((actor + 5060) as *const u32);
        (eq != 0 && *((eq + 1236) as *const u8) as u32 == style.weapon).then_some(style)
    }
}

unsafe fn independent(actor: u32) -> bool {
    active().is_some_and(|entry| entry.independent) && unsafe { local_style(actor) }.is_some()
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

static API: ServiceV1 = ServiceV1 { register, selected };
static MENU_API: registry::MenuV1 = registry::MenuV1 {
    register: register_named,
};
static INDEPENDENT_API: registry::MenuV1 = registry::MenuV1 {
    register: register_independent,
};
static CONTROL_API: registry::ControlV1 = registry::ControlV1 {
    register: register_controller,
};
static LAST_ENTRY: AtomicPtr<Entry> = AtomicPtr::new(ptr::null_mut());
static LAST_ACTOR: AtomicU32 = AtomicU32::new(0);

unsafe fn controller_frame(actor: u32) -> u32 {
    unsafe {
        let selected = active().filter(|e| e.controller.is_some());
        if actor == 0
            || (selected.is_none() && LAST_ENTRY.load(Ordering::Relaxed).is_null())
            || actor != local_hunter()
        {
            return PASS;
        }
        let eq = *((actor + 5060) as *const u32);
        let e = selected
            .filter(|e| eq != 0 && u32::from(*((eq + 1236) as *const u8)) == e.style.weapon);
        let entry = e.map_or(ptr::null_mut(), |e| ptr::from_ref(e).cast_mut());
        let prev = LAST_ENTRY.swap(entry, Ordering::Relaxed);
        let owner = LAST_ACTOR.swap(actor, Ordering::Relaxed);
        if prev != entry || actor != owner {
            if let Some(stop) = prev.as_ref().and_then(|e| e.controller) {
                // A previous scene's actor may already have been destroyed.
                stop(if owner == actor { owner } else { 0 }, ptr::null());
            }
        }
        let Some(tick) = e.and_then(|e| e.controller) else {
            return PASS;
        };
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
        if result == PASS
            && mhgu_sdk::style::action::custom(
                *((actor + 9478) as *const u16),
                *((actor + 9482) as *const u16),
            )
        {
            // A nested dispatch during a transition must not run a stock
            // weapon handler with a controller-owned slot.
            return 0;
        }
        result
    }
}
#[forge::hook(offset = 0x282da8)]
unsafe fn dispatch(actor: u32) -> u32 {
    let result = unsafe { crate::runtime::controller_frame(actor) };
    if result != mhgu_sdk::style::PASS {
        return result;
    }
    original!(actor)
}

#[forge::hook(offset = 0x2ab670)]
unsafe fn change_action(actor: u32, group: u32, slot: u32, flags: u32) -> u32 {
    // A registered controller owns the weapon's move selection. Shared GU
    // movement, guarding, damage and interactions remain available, but stock
    // weapon requests must not change state before the new style consumes GU's
    // resolved command buffer. Custom slots use the same native lifecycle.
    if group == u32::from(mhgu_sdk::style::action::GROUP)
        && slot < u32::from(mhgu_sdk::style::action::FIRST)
        && unsafe { local_style(actor) }.is_some()
        && active().is_some_and(|e| e.controller.is_some())
    {
        if let Some(resolve) = active().and_then(|e| e.request) {
            let move_id = unsafe { resolve(actor, slot, flags) };
            if move_id <= u8::MAX.into() {
                return original!(
                    actor,
                    group,
                    u32::from(mhgu_sdk::style::action::slot(move_id as u8)),
                    flags
                );
            }
        }
        return 0;
    }
    original!(actor, group, slot, flags)
}
static NATIVE_MENU: AtomicU32 = AtomicU32::new(0);
fn native_menu<R>(call: impl FnOnce() -> R) -> R {
    NATIVE_MENU.fetch_add(1, Ordering::AcqRel);
    let result = call();
    NATIVE_MENU.fetch_sub(1, Ordering::Release);
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
// The action and input controllers test this before consuming an equipped
// hunter art. Do not zero the equipment slots: the save writer uses them too.
#[forge::hook(offset = 0x2a26c8)]
unsafe fn art_ready(actor: u32, slot: u32) -> u32 {
    if unsafe { crate::runtime::independent(actor) } {
        return 0;
    }
    original!(actor, slot)
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
        let hide = active().is_some_and(|e| e.independent) && independent(local_hunter());
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
    let value = crate::config::load();
    REQUESTED.store(value.into(), Ordering::Release);
    if !unsafe { registry::publish(&API) } {
        return;
    }
    unsafe {
        registry::publish_menu(&MENU_API);
        registry::publish_independent(&INDEPENDENT_API);
        registry::publish_controller(&CONTROL_API);
    }
    let base = forge::mem::text_addr();
    forge::install_hook!(base, encode);
    forge::install_hook!(base, decode);
    forge::install_hook!(base, game_style);
    forge::install_hook!(base, art_ready);
    forge::install_hook!(base, valor_hud);
    forge::install_hook!(base, dispatch);
    forge::install_hook!(base, change_action);
    menu::install(base);
}
