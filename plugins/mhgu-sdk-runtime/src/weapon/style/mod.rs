//! Registered hunting styles and their shared GU action hooks.
use super::{inventory, local_hunter, profile};
use crate::save::CHARACTER;
use alloc::{boxed::Box, ffi::CString, format};
use core::{
    ffi::{CStr, c_char},
    ptr,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};
mod archive;
mod attack;
mod camera;
mod commands;
mod deflection;
mod effect;
mod ground_input;
mod guard;
mod manual;
mod menu;
pub(super) use menu::validate_arts;
mod multiplayer;
mod physical;
mod routes;
mod world_contact;
use mhgu_sdk::weapon::style::{
    ActionRequest, Input, PASS, Style, action, hunter, input, movement, part_motion, preload,
    registry::{self, ServiceV3},
    remote::Sheathe,
    rules,
};
pub(crate) use world_contact::expand as expand_world_contact;

pub(super) struct Entry {
    manual: mhgu_sdk::sync::Shared<Option<&'static [manual::Page]>>,
    pub(super) style: &'static Style,
    pub(super) kind: Option<mhgu_sdk::weapon::Weapon>,
    pub(super) weapon_name: Option<&'static CStr>,
    pub(super) name: CString,
    pub(super) details: CString,
    next: Option<&'static Entry>,
    multiplayer: multiplayer::Bindings,
    input_filter: AtomicPtr<()>,
    physical: AtomicPtr<mhgu_sdk::weapon::style::physical::Rule>,
    world_contact: AtomicPtr<()>,
    collision_pass: AtomicPtr<()>,
    attack_pass: AtomicPtr<()>,
    hurt_pass: AtomicPtr<()>,
    hurt_observer: AtomicPtr<()>,
    blade_contact: AtomicPtr<()>,
    strike_contact: AtomicPtr<()>,
    monster_strikes: AtomicPtr<action::melee::MonsterStrikes>,
    world_strike_power: AtomicPtr<()>,
    particle_release: AtomicPtr<()>,
    prepare: AtomicPtr<()>,
    pub(super) part_motion: AtomicPtr<part_motion::Motions>,
    pub(super) meter: AtomicPtr<()>,
    rules: AtomicPtr<rules::Rules>,
}

// Published entries live until exit. Omitted and native preferences skip the list.
static HEAD: AtomicPtr<Entry> = AtomicPtr::new(ptr::null_mut());
unsafe extern "C" fn register(
    raw: *const Style,
    name: *const c_char,
    details: *const c_char,
) -> u32 {
    let Some(style) = (unsafe { raw.as_ref() }) else {
        return 1;
    };
    unsafe { register_entry(style, None, name, details) }
}

pub(super) unsafe fn register_entry(
    style: &'static Style,
    weapon: Option<(mhgu_sdk::weapon::Weapon, &'static CStr)>,
    name: *const c_char,
    details: *const c_char,
) -> u32 {
    let kind = weapon.map(|(kind, _)| kind);
    if crate::save::custom_id(style.id).is_none()
        || (kind.is_none() && style.weapon as usize >= crate::save::WEAPONS)
        || style.art_slots > 3
    {
        return 1;
    }
    let mut head = HEAD.load(Ordering::Acquire);
    loop {
        let mut entries = unsafe { entries_from(head) };
        if let Some(entry) = entries.find(|entry| {
            entry.style.id == style.id
                || kind
                    .zip(entry.kind)
                    .is_some_and(|(new, old)| new.id == old.id)
        }) {
            return if ptr::eq(entry.style, style) && entry.kind == kind {
                0
            } else {
                2
            };
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
            manual: mhgu_sdk::sync::Shared::new(None),
            style,
            kind,
            weapon_name: weapon.map(|(_, name)| name),
            name,
            details,
            next: unsafe { head.as_ref() },
            multiplayer: multiplayer::Bindings::new(),
            input_filter: AtomicPtr::new(ptr::null_mut()),
            physical: AtomicPtr::new(ptr::null_mut()),
            world_contact: AtomicPtr::new(ptr::null_mut()),
            collision_pass: AtomicPtr::new(ptr::null_mut()),
            attack_pass: AtomicPtr::new(ptr::null_mut()),
            hurt_pass: AtomicPtr::new(ptr::null_mut()),
            hurt_observer: AtomicPtr::new(ptr::null_mut()),
            blade_contact: AtomicPtr::new(ptr::null_mut()),
            strike_contact: AtomicPtr::new(ptr::null_mut()),
            monster_strikes: AtomicPtr::new(ptr::null_mut()),
            world_strike_power: AtomicPtr::new(ptr::null_mut()),
            particle_release: AtomicPtr::new(ptr::null_mut()),
            prepare: AtomicPtr::new(ptr::null_mut()),
            part_motion: AtomicPtr::new(ptr::null_mut()),
            meter: AtomicPtr::new(ptr::null_mut()),
            rules: AtomicPtr::new(ptr::null_mut()),
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

// The head must be null or a node already published in HEAD.
unsafe fn entries_from(head: *mut Entry) -> impl Iterator<Item = &'static Entry> {
    let first = unsafe { head.as_ref() };
    core::iter::successors(first, |entry| entry.next)
}

pub(super) fn registered_entries() -> impl Iterator<Item = &'static Entry> {
    unsafe { entries_from(HEAD.load(Ordering::Acquire)) }
}

pub(super) fn registered_entry(id: u32) -> Option<&'static Entry> {
    registered_entries().find(|entry| entry.style.id == id)
}

struct Subscription {
    entry: &'static Entry,
    next: AtomicPtr<Subscription>,
}

/// Subscribers in registry order. Their nodes live until exit, like the styles.
struct Subscribers(AtomicPtr<Subscription>);

impl Subscribers {
    const fn new() -> Self {
        Self(AtomicPtr::new(ptr::null_mut()))
    }

    // SDK bindings run on the serialized game thread. Insert at bind time
    // so the hot readers need neither a registry scan nor a lock.
    fn include(&self, entry: &'static Entry) {
        let mut link = &self.0;
        for registered in registered_entries() {
            let next = link.load(Ordering::Acquire);
            let existing = unsafe { next.as_ref() }.filter(|node| ptr::eq(node.entry, registered));
            if ptr::eq(entry, registered) {
                if existing.is_none() {
                    let node = Box::new(Subscription {
                        entry,
                        next: AtomicPtr::new(next),
                    });
                    link.store(Box::into_raw(node), Ordering::Release);
                }
                return;
            }
            if let Some(node) = existing {
                link = &node.next;
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.0.load(Ordering::Acquire).is_null()
    }

    fn entries(&self) -> impl Iterator<Item = &'static Entry> {
        let first = unsafe { self.0.load(Ordering::Acquire).as_ref() };
        core::iter::successors(first, |node| unsafe {
            node.next.load(Ordering::Acquire).as_ref()
        })
        .map(|node| node.entry)
    }
}

pub(super) fn entries(weapon: u32) -> impl Iterator<Item = &'static Entry> {
    registered_entries().filter(move |entry| entry.style.weapon == weapon)
}

pub(super) fn choose_style(weapon: u32, entry: Option<&Entry>) {
    let id = entry
        .filter(|entry| entry.kind.is_none())
        .and_then(|entry| crate::save::custom_id(entry.style.id));
    let changed = CHARACTER.with(|prefs| {
        prefs
            .weapons
            .get_mut(weapon as usize)
            .is_some_and(|current| core::mem::replace(current, id) != id)
    });
    if changed {
        profile::selection_changed();
    }
}

pub(super) unsafe fn active_equipment(equipment: u32) -> Option<&'static Entry> {
    if equipment == 0 {
        return None;
    }
    active(unsafe { profile::equipment_weapon(equipment) })
}

pub(super) fn active(weapon: u32) -> Option<&'static Entry> {
    let id = CHARACTER.with(|prefs| prefs.weapons.get(weapon as usize).copied().flatten())?;
    entries(weapon).find(|entry| entry.kind.is_none() && entry.style.id == u32::from(id))
}

extern "C" fn selected(weapon: u32) -> u32 {
    inventory::selected()
        .filter(|(_, entry)| entry.style.weapon == weapon)
        .map(|(_, entry)| entry)
        .or_else(|| active(weapon))
        .map_or(0, |entry| entry.style.id)
}

unsafe fn actor_entry(actor: u32) -> Option<&'static Entry> {
    let equipment = unsafe { inventory::actor_equipment(actor) };
    if let Some(weapon) = unsafe { crate::weapon::records::display(equipment) } {
        return Some(weapon.definition.entry);
    }
    unsafe { active_equipment(equipment) }
}

pub(super) unsafe fn local_entry(actor: u32) -> Option<&'static Entry> {
    if actor == 0 || unsafe { local_hunter() } != actor || !unsafe { hunter::is_quest(actor) } {
        return None;
    }
    unsafe { actor_entry(actor) }
}

unsafe fn local_style(actor: u32) -> Option<&'static Style> {
    Some(unsafe { local_entry(actor) }?.style)
}

impl Entry {
    fn rules(&self) -> Option<&'static rules::Rules> {
        // Only process-lifetime descriptors are bound during style init.
        unsafe { self.rules.load(Ordering::Acquire).as_ref() }
    }
}

unsafe extern "C" fn bind_input_filter(id: u32, filter: input::Filter) -> bool {
    let Some(entry) = registered_entry(id) else {
        return false;
    };
    entry
        .input_filter
        .store(filter as *mut (), Ordering::Release);
    true
}

// 27ECCC has already suppressed gameplay input for menus and disabled hunters.
// The virtual weapon mapper, buffered ground transitions and 282DA8 run later.
#[forge::hook(offset = 0x27ebbc)]
unsafe fn filter_pad(actor: u32) -> u32 {
    unsafe {
        ground_input::clear(actor);
        if let Some(entry) = local_entry(actor) {
            let filter = entry.input_filter.load(Ordering::Acquire);
            if !filter.is_null() {
                let filter: input::Filter = core::mem::transmute(filter);
                let pad = &mut *((actor + 5108) as *mut input::Buttons);
                let before = pad.held | pad.pressed | pad.released;
                filter(actor, pad);
                ground_input::claim(actor, before, pad);
            }
        }
    }
    original!(actor)
}

unsafe extern "C" fn bind_rules(id: u32, rules: *const rules::Rules) -> bool {
    let Some(entry) = registered_entry(id) else {
        return false;
    };
    let Some(rules) = (unsafe { rules.as_ref() }) else {
        return false;
    };
    if rules.guard_half_angle > 0x8000 {
        return false;
    }
    entry
        .rules
        .store(ptr::from_ref(rules).cast_mut(), Ordering::Release);
    true
}

static PRELOAD: preload::Service = preload::Service(bind_preload);

unsafe extern "C" fn bind_preload(id: u32, prepare: preload::Prepare) -> bool {
    let Some(entry) = registered_entry(id) else {
        return false;
    };
    entry.prepare.store(prepare as *mut (), Ordering::Release);
    true
}

// GU 29B7D4 finishes hunt setup, including the weapon-specific initializer.
#[forge::hook(offset = 0x29b7d4)]
unsafe fn init_hunter(actor: u32) -> u32 {
    let result = original!(actor);
    if actor != 0 && unsafe { local_hunter() == actor && hunter::is_quest(actor) } {
        attack::begin_hunt();
    }
    if let Some(entry) = unsafe { local_entry(actor) } {
        let callback = entry.prepare.load(Ordering::Acquire);
        if !callback.is_null() {
            if entry.kind.is_some()
                && let Some((item, _)) = inventory::selected()
                && let Some((_, equipment)) = inventory::definition(item.equipment.get())
                && let Some(model) = equipment.model
            {
                // Expanded equipment can carry its private motions alongside
                // the mesh. Make them visible before dispatching preload.
                crate::weapon::resource::load(0x2124df0, model.archive);
            }
            let prepare: preload::Prepare = unsafe { core::mem::transmute(callback) };
            unsafe { prepare(actor) };
        }
    }
    result
}

unsafe fn playback_style(actor: u32) -> Option<&'static Style> {
    if actor == 0 {
        return None;
    }
    unsafe {
        local_style(actor).or_else(|| {
            let entry = multiplayer::peer_entry(actor)?;
            let style = entry.style;
            let current = action::current(actor);
            // Only the confirmed expanded equipment profile can select its
            // locomotion bank. Retail peers can share a fallback weapon type.
            if current.0 == 1 && entry.kind.is_some() {
                return Some(style);
            }
            let (group, slot) = if current == Sheathe::ACTION {
                // Common sheathe can bind a custom follow-up to the preceding action.
                action::previous(actor)
            } else {
                current
            };
            let id = action::custom_id(group, slot)?;
            style.actions.contains(id).then_some(style)
        })
    }
}

unsafe fn peer_sheathe(actor: u32) -> Option<(&'static Style, u32)> {
    unsafe {
        let entry = multiplayer::peer_entry(actor)?;
        if action::current(actor) != Sheathe::ACTION {
            return None;
        }
        Some((entry.style, multiplayer::sheathe_request(actor, entry)?))
    }
}

unsafe fn independent(actor: u32) -> bool {
    unsafe { local_style(actor) }.is_some_and(|style| style.independent)
}

#[forge::hook(offset = 0x27c6a0)]
unsafe fn encode(actor: u32, request: u32) -> u32 {
    let (style, selected) = unsafe {
        if request == Sheathe::MOTION
            && let Some((style, selected)) = peer_sheathe(actor)
        {
            (Some(style), selected)
        } else {
            (playback_style(actor), request)
        }
    };
    if let Some(f) = style.and_then(|style| style.encode) {
        let out = unsafe { f(actor, selected) };
        if out != PASS {
            return out;
        }
    }
    original!(actor, request)
}

#[forge::hook(offset = 0x27c6d4)]
unsafe fn decode(actor: u32, packed: u32) -> u32 {
    let style = unsafe {
        peer_sheathe(actor)
            .map(|(style, _)| style)
            .or_else(|| playback_style(actor))
    };
    if let Some(f) = style.and_then(|style| style.decode) {
        let out = unsafe { f(actor, packed) };
        if out != PASS {
            return out;
        }
    }
    original!(actor, packed)
}

static API: ServiceV3 = ServiceV3 {
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
        // The lobby also calls 282DA8, but its actors are too small for
        // quest-only weapon state and must use the native dispatcher.
        let in_quest = hunter::is_quest(actor);
        let selected = if in_quest { actor_entry(actor) } else { None };
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
            deflection::clear(owner);
            attack::clear(owner);
            guard::clear(owner);
            if let Some(stop) = prev.as_ref().and_then(|e| e.style.controller) {
                // A previous scene's actor may already have been destroyed.
                stop(
                    if owner == actor && in_quest { owner } else { 0 },
                    ptr::null(),
                );
            }
        }
        let Some((entry, tick)) = controller else {
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
        if group == 1
            && let Some(finish) = entry.rules().and_then(|rules| rules.ground_motion)
        {
            routes::finish_ground_motion(actor, entry.style, slot, finish);
        }
        *((actor + 9472) as *mut u32) &= !1;
        let pressed = if movement::sheathed_movement((group, slot))
            && *((actor + 10057) as *const u8) & 1 == 0
        {
            // 2A79CC saves the press in +5124 before its sprint filter
            // reduces +5112 to the dash shoulder. Recover only the extra key.
            *((actor + 5124) as *const u32)
        } else {
            u32::from(*((actor + 5112) as *const u16))
        };
        let input = Input {
            common: *((actor + 5140) as *const [u32; 4]),
            weapon: *((actor + 5156) as *const [u32; 4]),
            stick_angle: *((actor + 5132) as *const u16),
            stick_strength: *((actor + 5136) as *const i16),
            auxiliary_pressed: pressed & input::auxiliary_shoulder_mask(),
        };
        let result = ground_input::during_controller(actor, || tick(actor, &input));
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
    unsafe { multiplayer::observe_peers(actor) };
    let result = unsafe { crate::weapon::style::controller_frame(actor, None) };
    if result != mhgu_sdk::weapon::style::PASS {
        return result;
    }
    if let Some((style, _)) = unsafe { peer_sheathe(actor) } {
        unsafe {
            routes::finish_ground_motion(actor, style, Sheathe::ACTION.1, finish_peer_sheathe)
        };
    } else if let Some(entry) =
        unsafe { multiplayer::peer_entry(actor) }.filter(|e| e.kind.is_some())
    {
        let (group, slot) = unsafe { action::current(actor) };
        if group == 1
            && let Some(finish) = entry.rules().and_then(|r| r.ground_motion)
        {
            unsafe { routes::finish_ground_motion(actor, entry.style, slot, finish) };
        }
    }
    original!(actor)
}

extern "C" fn finish_peer_sheathe(slot: u16, phase: u8, motion: u32) -> bool {
    (slot, phase, motion) == (Sheathe::ACTION.1, 1, Sheathe::MOTION)
}

#[forge::hook(offset = 0x2b8178)]
unsafe fn weapon_dispatch(actor: u32) -> u32 {
    let (group, slot) = unsafe { action::current(actor) };
    let Some(id) = action::custom_id(group, slot) else {
        return original!(actor);
    };
    if unsafe { multiplayer::dispatch_peer(actor, id) } {
        return 0;
    }
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
    if unsafe { ground_input::blocked(actor, group as u16, slot as u16) } {
        return 0;
    }
    let slot = if group == u32::from(action::GROUP) {
        match unsafe { resolve_weapon_slot(actor, slot as u16, flags as u16) } {
            Some(slot) => u32::from(slot),
            None => return 0,
        }
    } else {
        slot
    };
    let out = original!(actor, group, slot, flags);
    if unsafe { action::current(actor) } == (group as u16, slot as u16)
        && unsafe { *((actor + 5) as *const u8) } == 0
    {
        deflection::clear(actor);
    }
    unsafe { guard::after_change(actor, group, slot) };
    out
}

unsafe fn resolve_weapon_slot(actor: u32, slot: u16, flags: u16) -> Option<u16> {
    let entry = unsafe { local_entry(actor) };
    if let Some(id) = action::custom_id(action::GROUP, slot) {
        return entry
            .or_else(|| unsafe { multiplayer::peer_entry(actor) })?
            .style
            .actions
            .contains(id)
            .then_some(slot);
    }
    let Some(entry) = entry.filter(|e| e.kind.is_some() || e.style.controller.is_some()) else {
        return Some(slot);
    };
    if unsafe { routes::allows_native_art(actor, entry.style, slot, flags) } {
        return Some(slot);
    }
    unsafe { routes::resolve_stock_slot(actor, entry.style, slot, flags) }
}

unsafe fn resolve_request(actor: u32, style: &Style, req: ActionRequest) -> Option<u16> {
    let id = unsafe { (style.request?)(actor, &req) };
    if id == mhgu_sdk::weapon::style::NATIVE_ACTION
        && !req.draw_route
        && style.weapon < mhgu_sdk::weapon::WeaponKind::COUNT as u32
    {
        return Some(req.slot);
    }
    let id = u8::try_from(id).ok()?;
    style.actions.contains(id).then(|| action::slot(id))
}
static NATIVE_MENU: AtomicU32 = AtomicU32::new(0);
const ARTS_MENU: u32 = 2;
pub(super) fn native_menu<R>(configure_arts: bool, call: impl FnOnce() -> R) -> R {
    let mode = if configure_arts { ARTS_MENU } else { 1 };
    let previous = NATIVE_MENU.swap(mode, Ordering::AcqRel);
    let result = call();
    NATIVE_MENU.store(previous, Ordering::Release);
    result
}
#[forge::hook(offset = 0x268818)]
unsafe fn game_style(actor: u32) -> u32 {
    if crate::weapon::style::NATIVE_MENU.load(core::sync::atomic::Ordering::Acquire) == 0
        && unsafe { crate::weapon::style::independent(actor) }
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
        && let Some(entry) = inventory::selected()
            .map(|(_, entry)| entry)
            .or_else(|| unsafe { actor_entry(local_hunter()) })
        && (entry.kind.is_some() || entry.style.art_slots != 0)
    {
        return u32::from(entry.style.art_slots);
    }
    original!(manager, style)
}

/// GU's own compatibility check maps exclusive Arts to their retail weapon.
/// Supply the SDK weapon identity for the local box menus and equip cleanup.
#[forge::hook(offset = 0x277848)]
unsafe fn art_compatible(manager: u32, weapon: u32, art: u32) -> u32 {
    if NATIVE_MENU.load(Ordering::Acquire) == ARTS_MENU
        && mhgu_sdk::service::on_game_thread()
        && let Some((_, entry)) = inventory::selected()
    {
        return original!(manager, entry.style.weapon, art);
    }
    original!(manager, weapon, art)
}

// The Valor widget shares HUD0 with other status widgets. Mask its render bit
// while retaining the layout's requested visibility for later restoration.
static HIDDEN_VALOR_NODE: AtomicU32 = AtomicU32::new(0);
#[forge::hook(offset = 0x639bd0)]
unsafe fn valor_hud(widget: u32) -> u32 {
    let out = original!(widget);
    unsafe { crate::weapon::style::update_valor_node(widget) };
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

pub(super) fn install(base: u32) -> bool {
    if !unsafe { registry::publish(&API) } {
        return false;
    }
    static RULES: rules::Service = rules::Service(bind_rules);
    unsafe { rules::publish(&RULES) };
    static INPUT_FILTER: input::FilterService = input::FilterService(bind_input_filter);
    if unsafe { input::publish_filter(&INPUT_FILTER) } {
        forge::install_hook!(base, filter_pad);
    }
    archive::install(base);
    forge::install_hook!(base, encode);
    forge::install_hook!(base, decode);
    forge::install_hook!(base, game_style);
    forge::install_hook!(base, art_ready);
    forge::install_hook!(base, art_capacity);
    forge::install_hook!(base, art_compatible);
    forge::install_hook!(base, valor_hud);
    forge::install_hook!(base, dispatch);
    forge::install_hook!(base, weapon_dispatch);
    forge::install_hook!(base, change_action);
    if unsafe { preload::publish(&PRELOAD) } {
        forge::install_hook!(base, init_hunter);
    }
    multiplayer::install(base);
    attack::install(base);
    deflection::install(base);
    world_contact::install(base);
    camera::install(base);
    unsafe { physical::install(base) };
    commands::install(base);
    effect::install(base);
    mhfz_sdk::effects::model::install();
    guard::install(base);
    routes::install(base);
    manual::install(base);
    menu::install(base);
    true
}
