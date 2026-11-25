//! GU action packets and dispatch for remote weapon styles.
use super::{Entry, entries, local_entry, local_hunter, registered_entries, registered_entry};
use core::{
    ptr,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};
use mhgu_sdk::style::{action, hunter, remote};

pub(super) struct Bindings {
    tick: AtomicU32,
    peer_frame: AtomicU32,
    action_state: AtomicPtr<remote::ActionState>,
}

impl Bindings {
    pub(super) const fn new() -> Self {
        Self {
            tick: AtomicU32::new(0),
            peer_frame: AtomicU32::new(0),
            action_state: AtomicPtr::new(ptr::null_mut()),
        }
    }

    fn action_state(&self) -> Option<&'static remote::ActionState> {
        // bind_action_state stores a resident descriptor with a static lifetime.
        unsafe { self.action_state.load(Ordering::Acquire).as_ref() }
    }
}

static REMOTE: remote::Service = remote::Service {
    bind: bind_remote,
    observe: bind_peer_frame,
    bind_action_state,
};

unsafe extern "C" fn bind_peer_frame(id: u32, callback: remote::Observe) -> bool {
    let Some(entry) = registered_entry(id) else {
        return false;
    };
    entry
        .multiplayer
        .peer_frame
        .store(callback as u32, Ordering::Release);
    true
}

unsafe extern "C" fn bind_action_state(id: u32, state: &'static remote::ActionState) -> bool {
    // These weapons leave the packet flag nibble at bits 16..19 free.
    let Some(entry) = registered_entry(id).filter(|e| matches!(e.style.weapon, 0 | 1 | 8 | 9 | 11))
    else {
        return false;
    };
    entry
        .multiplayer
        .action_state
        .store(ptr::from_ref(state).cast_mut(), Ordering::Release);
    true
}

// Retail 1FED90 and 1FF308 use bits 16..19 at packet +16 only for
// weapon 4, 6 and 14. W00, W01, W08, W09 and W11 ignore that nibble.
const STATE_PRESENT: u8 = 0b1000;
const STATE_VALUE: u8 = 0b0111;
const STATE_SHIFT: u32 = 16;

#[forge::hook(offset = 0x1fed90)]
unsafe fn pack_action(packet: u32, actor: u32, flags: u32) -> u32 {
    let result = original!(packet, actor, flags);
    unsafe {
        if let Some(codec) = local_entry(actor).and_then(|entry| entry.multiplayer.action_state()) {
            let value = (codec.encode)(actor);
            if value <= STATE_VALUE {
                let word = (packet + 16) as *mut u32;
                let mask = u32::from(STATE_PRESENT | STATE_VALUE) << STATE_SHIFT;
                word.write(
                    (word.read() & !mask) | (u32::from(STATE_PRESENT | value) << STATE_SHIFT),
                );
            }
        }
    }
    result
}

#[forge::hook(offset = 0x1ff308)]
unsafe fn unpack_action(packet: u32, actor: u32) -> u32 {
    let result = original!(packet, actor);
    unsafe {
        if packet == 0 || actor == 0 {
            return result;
        }
        if let Some(codec) = peer_binding(actor, |entry| entry.multiplayer.action_state()) {
            let group = *((packet + 54) as *const u16);
            let slot = *((packet + 56) as *const u16);
            if action::current(actor) == (group, slot) {
                let bits = (*((packet + 16) as *const u32) >> STATE_SHIFT) as u8;
                let value = if bits & STATE_PRESENT != 0 {
                    bits & STATE_VALUE
                } else {
                    u8::MAX
                };
                (codec.decode)(actor, value);
            }
        }
    }
    result
}

unsafe extern "C" fn bind_remote(id: u32, tick: remote::Tick) {
    if let Some(entry) = registered_entry(id) {
        entry.multiplayer.tick.store(tick as u32, Ordering::Release);
    }
}

// GU's netPlAction carries the weapon action (1FED90 / 1FF308), independent
// of the receiver's equipment-box selection. V1 private IDs are per weapon.
pub(super) unsafe fn peer_entry(actor: u32) -> Option<&'static Entry> {
    unsafe {
        peer_binding(actor, |e| {
            (e.multiplayer.tick.load(Ordering::Acquire) != 0).then_some(e)
        })
    }
}

unsafe fn peer_binding<T>(actor: u32, binding: impl Fn(&'static Entry) -> Option<T>) -> Option<T> {
    unsafe {
        // World contacts also call here with monster or object owners.
        remote::Roster::current()?.peer_index(actor)?;
        if !hunter::is_quest(actor) {
            return None;
        }
        let mut candidates =
            entries(u32::from(hunter::equipped_weapon(actor)?)).filter_map(binding);
        let first = candidates.next()?;
        candidates.next().is_none().then_some(first)
    }
}

pub(super) unsafe fn observe_peers(actor: u32) {
    if actor != 0 && actor == unsafe { local_hunter() } {
        for entry in registered_entries() {
            let callback = entry.multiplayer.peer_frame.load(Ordering::Acquire);
            if callback != 0 {
                let poll: remote::Observe = unsafe { core::mem::transmute(callback) };
                unsafe { poll(actor) };
            }
        }
    }
}

pub(super) unsafe fn dispatch_peer(actor: u32, id: u8) -> bool {
    let Some(entry) = (unsafe { peer_entry(actor) }) else {
        return false;
    };
    if entry.style.actions.contains(id) {
        let tick: remote::Tick =
            unsafe { core::mem::transmute(entry.multiplayer.tick.load(Ordering::Acquire)) };
        unsafe { tick(actor, id) };
    }
    true
}

pub(super) fn install(base: u32) {
    if unsafe { remote::publish(&REMOTE) } {
        forge::install_hook!(base, pack_action);
        forge::install_hook!(base, unpack_action);
    }
}
