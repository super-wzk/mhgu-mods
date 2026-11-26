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
            let local = hunter::local_in(manager);
            if local == 0 || !hunter::is_quest(local) {
                return None;
            }
            Some(Self {
                players: ((manager + 28) as *const [u32; PLAYER_COUNT]).read(),
                local,
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
        (unsafe { hunter::is_quest(actor) && hunter::equipped_weapon(actor) == Some(weapon) })
            .then_some(actor)
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

const NAME: &core::ffi::CStr = c"mhgu::StylePlaybackV1";
const MAGIC: u32 = u32::from_le_bytes(*b"SPB1");

pub type Tick = unsafe extern "C" fn(actor: u32, state: u8);
/// A per-frame observer for peer actions that return to GU's native dispatcher.
pub type Observe = unsafe extern "C" fn(local: u32);

/// Native sheathe variants a style can replay without the preceding action.
#[repr(C)]
pub struct Sheathe {
    pub sprints: &'static [u8],
    pub ordinary: u32,
    pub sprint: u32,
}

impl Sheathe {
    pub const ACTION: (u16, u16) = (1, 2);
    pub const MOTION: u32 = 5003;

    pub fn follows_sprint(&self, previous: (u16, u16)) -> bool {
        super::action::custom_id(previous.0, previous.1)
            .is_some_and(|id| self.sprints.contains(&id))
    }
}

const SHEATHE_NAME: &core::ffi::CStr = c"mhgu::StyleRemoteSheatheV1";
const SHEATHE_MAGIC: u32 = u32::from_le_bytes(*b"SRS1");

#[doc(hidden)]
#[repr(C)]
pub struct SheatheService(pub unsafe extern "C" fn(u32, &'static Sheathe) -> bool);

/// # Safety
/// Publish a resident service during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish_sheathe(api: &'static SheatheService) -> bool {
    unsafe { service::publish(SHEATHE_NAME, SHEATHE_MAGIC, api) }
}

/// # Safety
/// Call after style registration. The descriptor and motion callbacks must remain loaded.
pub unsafe fn bind_sheathe(id: u32, motions: &'static Sheathe) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<SheatheService>(SHEATHE_NAME, SHEATHE_MAGIC) }
            .is_some_and(|api| unsafe { (api.0)(id, motions) })
}

/// Optional state carried by a style's GU action packets. Encode 0..=7 to
/// include a value, or return `u8::MAX` to omit it. Decode receives
/// `u8::MAX` when the sender omitted the value.
#[repr(C)]
pub struct ActionState {
    pub encode: unsafe extern "C" fn(actor: u32) -> u8,
    pub decode: unsafe extern "C" fn(actor: u32, bits: u8),
}

/// A style's remote action playback and optional companion callbacks.
#[repr(C)]
pub struct Playback {
    pub tick: Tick,
    pub observe: Option<Observe>,
    pub action_state: Option<&'static ActionState>,
}

#[doc(hidden)]
#[repr(C)]
pub struct Service(pub unsafe extern "C" fn(u32, &'static Playback) -> bool);

/// # Safety
/// Publish a resident service during Forge initialization.
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// Bind a registered style's playback on the game thread. Returns false if
/// the service is unavailable or a companion callback is rejected.
///
/// # Safety
/// Call during style initialization after registration succeeds. The style,
/// descriptor and callbacks must remain loaded.
pub unsafe fn bind_playback(id: u32, playback: &'static Playback) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<Service>(NAME, MAGIC) }
            .is_some_and(|api| unsafe { (api.0)(id, playback) })
}

const POSITION_NAME: &core::ffi::CStr = c"mhgu::StylePositionStateV1";
const POSITION_MAGIC: u32 = u32::from_le_bytes(*b"SPS1");

#[doc(hidden)]
#[repr(C)]
pub struct PositionService(pub extern "C" fn(u32, &'static ActionState) -> bool);

/// # Safety
/// Publish a resident service during Forge initialization.
#[doc(hidden)]
pub unsafe fn publish_position(api: &'static PositionService) -> bool {
    unsafe { service::publish(POSITION_NAME, POSITION_MAGIC, api) }
}

/// Carry a value on GU's periodic position updates during a private action.
/// This binding is for expanded weapons with registered playback. Encode
/// 0..=7 to attach a value. Decode runs only for the matching accepted action.
pub fn bind_position_state(id: u32, codec: &'static ActionState) -> bool {
    service::on_game_thread()
        && unsafe { service::find::<PositionService>(POSITION_NAME, POSITION_MAGIC) }
            .is_some_and(|api| (api.0)(id, codec))
}
