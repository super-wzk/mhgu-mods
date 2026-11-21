//! Playback of a peer's accepted private weapon action on the game thread.
use super::hunter;
use crate::service;
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

pub const PLAYER_COUNT: usize = 4;

/// The live hunter pointers at the time of a game-thread callback.
pub struct Roster {
    players: [u32; PLAYER_COUNT],
    local: u32,
}

impl Roster {
    /// # Safety
    /// Read on GU's game thread. Do not retain the snapshot across callbacks.
    pub unsafe fn current() -> Option<Self> {
        unsafe {
            let manager = hunter::manager()?;
            Some(Self {
                players: ((manager + 28) as *const [u32; PLAYER_COUNT]).read(),
                local: hunter::local_in(manager),
            })
        }
    }

    pub fn peer(&self, index: usize) -> Option<u32> {
        self.players
            .get(index)
            .copied()
            .filter(|&p| p != 0 && p != self.local)
    }

    pub fn peer_index(&self, actor: u32) -> Option<usize> {
        if actor == 0 || actor == self.local {
            return None;
        }
        self.players.iter().position(|&p| p == actor)
    }
}

struct Peer<T> {
    actor: AtomicU32,
    busy: AtomicBool,
    playback: UnsafeCell<Option<T>>,
}

/// Per-player playback, retired before reuse of a GU roster slot.
pub struct PeerStates<T>([Peer<T>; PLAYER_COUNT]);

// The busy flag makes access to each playback exclusive, including reentrant callbacks.
unsafe impl<T: Send> Sync for PeerStates<T> {}

impl<T> PeerStates<T> {
    pub const fn new() -> Self {
        Self(
            [const {
                Peer {
                    actor: AtomicU32::new(0),
                    busy: AtomicBool::new(false),
                    playback: UnsafeCell::new(None),
                }
            }; PLAYER_COUNT],
        )
    }

    /// Check the recorded owner against the roster before reading its equipment.
    ///
    /// # Safety
    /// Called on GU's game thread. The roster must come from this callback.
    pub unsafe fn live(&self, index: usize, roster: &Roster, weapon: u8) -> Option<u32> {
        let recorded = self.0.get(index)?.actor.load(Ordering::Acquire);
        let actor = roster.peer(index).filter(|&actor| recorded == actor)?;
        (unsafe { hunter::equipped_weapon(actor) } == Some(weapon)).then_some(actor)
    }

    /// Retire departed hunters before visiting peers that still wield this weapon.
    /// # Safety
    /// Call once per game frame on the game thread.
    pub unsafe fn visit_live(&self, weapon: u8, mut visit: impl FnMut(usize, u32, &mut T)) {
        let roster = unsafe { Roster::current() };
        for index in 0..PLAYER_COUNT {
            let actor = roster
                .as_ref()
                .and_then(|r| unsafe { self.live(index, r, weapon) })
                .unwrap_or(0);
            self.update(index, actor, |saved, _| {
                if let Some(battle) = saved {
                    visit(index, actor, battle);
                }
            });
        }
    }

    /// `actor` is validated by the caller against the current roster and equipment.
    /// Zero retires the slot. The callback runs only when the slot is exclusively held.
    pub fn update(&self, index: usize, actor: u32, visit: impl FnOnce(&mut Option<T>, bool)) {
        let Some(peer) = self.0.get(index) else {
            return;
        };
        if peer.busy.swap(true, Ordering::Acquire) {
            return;
        }
        let changed = peer.actor.swap(actor, Ordering::AcqRel) != actor;
        let playback = unsafe { &mut *peer.playback.get() };
        if changed {
            *playback = None;
        }
        visit(playback, changed);
        peer.busy.store(false, Ordering::Release);
    }
}

impl<T> Default for PeerStates<T> {
    fn default() -> Self {
        Self::new()
    }
}

const NAME: &core::ffi::CStr = c"mhgu::StyleRemoteActionV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SRA1");

pub type Tick = unsafe extern "C" fn(actor: u32, state: u8);
/// A per-frame observer for peer actions that return to GU's native dispatcher.
pub type Observe = unsafe extern "C" fn(local: u32);

/// Optional state carried by a style's GU action packets. Encode 0..=7 to
/// include a value, or return `u8::MAX` to omit it. Decode receives
/// `u8::MAX` when the sender omitted the value.
#[repr(C)]
pub struct ActionState {
    pub encode: unsafe extern "C" fn(actor: u32) -> u8,
    pub decode: unsafe extern "C" fn(actor: u32, bits: u8),
}

#[doc(hidden)]
#[repr(C)]
pub struct Service {
    pub bind: unsafe extern "C" fn(u32, Tick),
    pub observe: unsafe extern "C" fn(u32, Observe) -> bool,
    pub bind_action_state: unsafe extern "C" fn(u32, &'static ActionState) -> bool,
}

/// # Safety
/// Publish a resident service during Forge initialization.
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

fn current() -> Option<&'static Service> {
    if !service::on_game_thread() {
        return None;
    }
    unsafe { service::find(NAME, MAGIC) }
}

/// # Safety
/// The registered style and callback must remain loaded.
pub unsafe fn bind(id: u32, tick: Tick) -> bool {
    let Some(api) = current() else { return false };
    unsafe { (api.bind)(id, tick) };
    true
}

/// Register a per-frame observer for a style.
///
/// # Safety
/// The callback must remain valid throughout the lifetime of the style.
pub unsafe fn observe(id: u32, callback: Observe) -> bool {
    current().is_some_and(|api| unsafe { (api.observe)(id, callback) })
}

/// Register state callbacks for a style. Returns false if the registry cannot
/// carry the value in this weapon's native packet.
///
/// # Safety
/// The descriptor and callbacks must remain valid throughout the lifetime of the style.
pub unsafe fn bind_action_state(id: u32, state: &'static ActionState) -> bool {
    current().is_some_and(|api| unsafe { (api.bind_action_state)(id, state) })
}
