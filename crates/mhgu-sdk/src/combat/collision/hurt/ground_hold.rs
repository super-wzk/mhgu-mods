//! A timed ground restraint through GU's common monster action graph.
use super::{HurtScene, Monster, MonsterSpawn, ROSTER_SLOTS, monster_in_area};
use crate::{
    memory::{Field, Object, singleton},
    service,
};

const NAME: &core::ffi::CStr = c"mhgu::GroundHoldV1";
const MAGIC: u32 = u32::from_le_bytes(*b"GHD1");
const ACTION_GROUP: Field<u8> = Field::at(29664);
const ACTION_SLOT: Field<u8> = Field::at(29665);
const AILMENTS: Field<u8> = Field::at(24072);
const PENDING: Field<u8> = Field::at(24073);
const TIMER: Field<f32> = Field::at(24076);
const PARALYSIS: u8 = 4;
const HARD_CONTROL: u8 = 0x1d;

#[doc(hidden)]
#[repr(transparent)]
pub struct GroundHoldService(pub unsafe extern "C" fn(u32, f32) -> bool);

/// # Safety
/// The provider must be installed for the lifetime of the process.
#[doc(hidden)]
pub unsafe fn publish_ground_hold(api: &'static GroundHoldService) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// An ongoing GU ground restraint. Queries use the live roster, including
/// the spawn revision, because a roster slot can be freed or reused.
pub struct GroundHold {
    area: u8,
    slot: u8,
    spawn: MonsterSpawn,
}

impl GroundHold {
    pub fn spawn(&self) -> MonsterSpawn {
        self.spawn
    }

    fn live_monster(&self) -> Option<Monster<'_>> {
        if !service::on_game_thread() {
            return None;
        }
        let monster = monster_in_area(unsafe { singleton(super::ROSTER) }?, self.slot, self.area)?;
        (monster.spawn == self.spawn).then_some(monster)
    }

    /// Resolve the same living spawn in this hunter's area, including after
    /// GU has transitioned it out of the restraint action.
    pub fn resolve<'a>(&'a self, scene: &'a HurtScene<'_>) -> Option<Monster<'a>> {
        if scene.area() != self.area {
            return None;
        }
        self.live_monster()
    }

    fn monster(&self) -> Option<Monster<'_>> {
        self.live_monster()
            .filter(|monster| ground_hold_action(monster.actor, monster.status))
    }

    pub fn locally_owned(&self) -> bool {
        self.live_monster()
            .is_some_and(|monster| monster.locally_owned())
    }

    pub fn network_controller(&self) -> Option<u8> {
        self.live_monster()?.network_controller()
    }

    pub fn active(&self) -> bool {
        self.monster()
            .is_some_and(|monster| monster.status.get(TIMER) > 0.)
    }

    /// World position of the held monster's root, while the hold is active.
    pub fn position(&self) -> Option<[f32; 3]> {
        let monster = self.monster()?;
        let position = monster.actor.get::<[f32; 3]>(Field::at(64));
        (monster.status.get(TIMER) > 0. && position.into_iter().all(f32::is_finite))
            .then_some(position)
    }

    /// End the timer and let the native action graph run its recovery.
    pub fn release(self) {
        if let Some(monster) = self.monster().filter(Monster::locally_owned) {
            monster.status.set(TIMER, 0.);
        }
    }
}

fn ground_hold_action(actor: Object<'_>, status: Object<'_>) -> bool {
    actor.get(ACTION_GROUP) == 10
        && actor.get(ACTION_SLOT) == 31
        && status.get(AILMENTS) & PARALYSIS == 0
}

impl MonsterSpawn {
    /// Look up this exact living spawn in the hunter's area on the game thread.
    pub fn accepts_paralysis_in(&self, area: u8) -> bool {
        if !service::on_game_thread() {
            return false;
        }
        let Some(roster) = (unsafe { singleton(super::ROSTER) }) else {
            return false;
        };
        (0..ROSTER_SLOTS)
            .filter_map(|slot| monster_in_area(roster, slot, area))
            .find(|monster| monster.spawn == *self)
            .is_some_and(|monster| monster.accepts_paralysis())
    }
}

impl Monster<'_> {
    /// Innate paralysis tuning. A2AA8 initializes the threshold from this row,
    /// A2B10 handles buildup, and A2C04 reads the signed reaction duration.
    /// Temporary action and status restrictions are checked when starting a hold.
    pub fn accepts_paralysis(&self) -> bool {
        let Some(tune) = (unsafe { Object::at(self.actor.get(Field::at(30188))) }) else {
            return false;
        };
        let Some(row) = (unsafe { Object::at(tune.get(Field::at(160))) }) else {
            return false;
        };
        row.get::<i16>(Field::at(0)) > 0 && row.get::<i16>(Field::at(6)) > 0
    }

    /// Whether this spawn is currently in the SDK restraint on this console.
    /// Available on both the controller and replicas of the native action.
    pub fn ground_held(&self) -> bool {
        ground_hold_action(self.actor, self.status)
            && self.status.get::<u8>(AILMENTS) & HARD_CONTROL == 0
            && self.status.get(TIMER) > 0.
    }

    /// Borrow an existing restraint from this spawn without restarting it.
    /// Only the current monster controller can release it.
    pub fn observe_ground_hold(&self) -> Option<GroundHold> {
        if !self.ground_held() {
            return None;
        }
        Some(GroundHold {
            area: self.area,
            slot: self.slot,
            spawn: self.spawn,
        })
    }

    /// Check the current ground reaction and native status restrictions.
    pub fn can_ground_hold(&self) -> bool {
        if self.actor.get(ACTION_GROUP) >= 9
            || self.status.get::<u8>(Field::at(442)) != 0
            || self.status.get::<u8>(Field::at(1317)) != 0
            || [930, 931, 932]
                .into_iter()
                .any(|at| self.status.get::<u8>(Field::at(at)) != 0)
            || (self.status.get(AILMENTS) | self.status.get(PENDING)) & HARD_CONTROL != 0
            || self.status.get(TIMER) != 0.
        {
            return false;
        }
        // GU 9B90C uses 10/31 for the normal ground reaction. Monsters
        // with a special reaction or without paralysis tuning use other paths.
        unsafe {
            if !self.accepts_paralysis() {
                return false;
            }
            let Some(vtable) = Object::at(self.actor.get(Field::at(0))) else {
                return false;
            };
            let status_blocked: unsafe extern "C" fn(u32) -> u32 =
                core::mem::transmute(vtable.get::<usize>(Field::at(924)));
            let special_reaction: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(vtable.get::<usize>(Field::at(1016)));
            status_blocked(self.actor.addr()) == 0 && special_reaction(self.actor.addr(), 0) == 0
        }
    }

    /// Start a timed restraint on a locally controlled ground monster.
    /// Its native paralysis buildup and ailment flags are unaffected.
    #[must_use]
    pub fn try_ground_hold(&self, frames: f32) -> Option<GroundHold> {
        if !frames.is_finite() || frames <= 0. || !self.locally_owned() || !self.can_ground_hold() {
            return None;
        }
        unsafe {
            let api = service::find::<GroundHoldService>(NAME, MAGIC)?;
            if !(api.0)(self.actor.addr(), frames) {
                return None;
            }
        }
        Some(GroundHold {
            area: self.area,
            slot: self.slot,
            spawn: self.spawn,
        })
    }
}

/// # Safety
/// The runtime must supply the live target of a confirmed hit, inside GU's
/// monster reaction dispatcher after native HP and part damage settlement.
#[doc(hidden)]
pub unsafe fn prepare_ground_topple(actor: u32) {
    const HIT_REACTION: Field<u16> = Field::at(942);
    // GU 97DDC recognizes the successful rider dismount (A93A0, hunter
    // action 4/13). 9A828 dispatches reaction 33 to ground action 10/167.
    const GROUND_KNOCKDOWN: u16 = 33;
    let Some(monster) = (unsafe { Object::at(actor) }) else {
        return;
    };
    let Some(status) = (unsafe { Object::at(monster.get(Field::at(5160))) }) else {
        return;
    };
    let action = (monster.get(ACTION_GROUP), monster.get(ACTION_SLOT));
    let held = ground_hold_action(monster, status);
    if status.get::<i32>(Field::at(880)) <= 0
        || status.get::<u8>(Field::at(1092)) == 0
        || status.get::<u8>(Field::at(442)) != 0
        || status.get::<u8>(Field::at(1317)) != 0
        || (status.get(AILMENTS) | status.get(PENDING)) & HARD_CONTROL != 0
        || [930, 931, 932]
            .into_iter()
            .any(|at| status.get::<u8>(Field::at(at)) != 0)
        || !matches!(
            status.get(HIT_REACTION),
            0 | 2 | 3 | 14 | 28 | GROUND_KNOCKDOWN
        )
        || !(action.0 < 9 || held || matches!(action, (10, 0..=6)))
    {
        return;
    }
    let Some(vtable) = (unsafe { Object::at(monster.get(Field::at(0))) }) else {
        return;
    };
    let special_reaction: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(vtable.get::<usize>(Field::at(1016))) };
    let blocks_reactions: unsafe extern "C" fn(u32) -> u32 =
        unsafe { core::mem::transmute(vtable.get::<usize>(Field::at(1012))) };
    if unsafe { special_reaction(actor, 0) != 0 || blocks_reactions(actor) != 0 } {
        return;
    }
    status.set(HIT_REACTION, GROUND_KNOCKDOWN);
    if held {
        status.set(TIMER, 0.);
    }
}

/// # Safety
/// Only called by the runtime provider from the matching GU action entry.
#[doc(hidden)]
pub unsafe fn initialize_ground_hold(actor: u32, frames: f32) -> Option<u32> {
    let actor = unsafe { Object::at(actor) }?;
    let status = unsafe { Object::at(actor.get(Field::at(5160))) }?;
    ground_hold_action(actor, status).then(|| {
        status.set(TIMER, frames);
        status.addr()
    })
}

/// Clear a proxy's temporary hold before its next authoritative action.
///
/// # Safety
/// The runtime must supply the live monster in GU's remote action dispatcher.
#[doc(hidden)]
pub unsafe fn end_ground_hold_replica(actor: u32) {
    let Some(actor) = (unsafe { Object::at(actor) }) else {
        return;
    };
    let Some(status) = (unsafe { Object::at(actor.get(Field::at(5160))) }) else {
        return;
    };
    if ground_hold_action(actor, status) && status.get::<u8>(AILMENTS) & HARD_CONTROL == 0 {
        status.set(TIMER, 0.);
    }
}
