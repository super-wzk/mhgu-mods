//! The quest services available during a monster update and its patrol selection.
use super::{Monster, MonsterState, MonsterUpdate};
use crate::{
    memory::{Field, Object, entry},
    quest::Quest,
};

impl MonsterState<'_> {
    /// GU C4BA8: a hunter target is either a slot or a bitmask. 0xff is no target.
    pub fn target_player(self) -> Option<u8> {
        let id = self.0.get(Field::<u8>::at(564));
        if self.0.get(Field::<u8>::at(560)) != 1 || id == 255 {
            return None;
        }
        if self.0.get(Field::<u8>::at(562)) == 10 {
            let mask = id & 15;
            (mask != 0).then(|| mask.trailing_zeros() as u8)
        } else {
            (id < 4).then_some(id)
        }
    }
}

impl<'a> MonsterUpdate<'a> {
    pub fn quest(self) -> Quest<'a> {
        // MonsterUpdate guarantees an active quest on this game thread.
        unsafe { Quest::current() }
    }

    /// Enter the normal hunter-alert state and refresh GU's primary target. Keep
    /// a valid native selection (including bitmasks) when its slot already agrees.
    pub fn engage_hunter(self, slot: u8) -> Option<[f32; 3]> {
        let state = self.monster().state()?;
        let position = self
            .quest()
            .player_position_in_area(self.monster().area(), slot)?;
        self.set_alert(1); // Same transition as 7E2A4 on hunter discovery.
        if state.target_player() != Some(slot) {
            self.set_primary_target(1, 2, slot);
        }
        self.refresh_primary_target();
        Some(position)
    }

    /// Leave hunter alert and clear the primary descriptor only if it targets a hunter.
    pub fn disengage_hunter(self) {
        if self
            .monster()
            .state()
            .is_some_and(|s| s.0.get(Field::<u8>::at(560)) == 1)
        {
            self.set_alert(0);
            self.set_primary_target(255, 0, 0);
            self.refresh_primary_target();
        }
    }

    fn set_alert(self, mode: u32) {
        // BCB9C is idempotent and performs the proper change notifications.
        let change: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0xbcb9c)) };
        unsafe {
            change(self.monster().as_raw(), mode);
        }
    }

    fn set_primary_target(self, kind: u8, mode: u8, id: u8) {
        let actor = self.monster().as_raw();
        let set: unsafe extern "C" fn(u32, u32, u32, u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0xc6e14)) };
        unsafe {
            set(actor, kind.into(), mode.into(), id.into(), 0);
        }
    }

    fn refresh_primary_target(self) {
        let refresh: unsafe extern "C" fn(u32, u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0xc46c8)) };
        unsafe {
            refresh(self.monster().as_raw(), 0, 0);
        }
    }
}

impl<'a> Monster<'a> {
    /// A route in this monster's rAreaPatrolData for the current map and area.
    /// No route is distinct from a route on the stage's shared resource.
    pub fn patrol_route(self, index: u8) -> Option<PatrolRoute<'a>> {
        let resource = unsafe { Object::at(self.0.get(Field::at(30220)))? };
        let vtable = unsafe { Object::at(self.0.get(Field::at(0)))? };
        // Same virtual map number and native route lookup as GU's patrol command.
        let map: unsafe extern "C" fn(u32) -> u32 =
            unsafe { core::mem::transmute(vtable.get(Field::<u32>::at(156))) };
        let find: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0x69e6f0)) };
        unsafe {
            Object::at(find(
                resource.addr(),
                map(self.as_raw()),
                self.area().into(),
                index.into(),
            ))
        }
        .map(PatrolRoute)
    }
}

/// GU cAreaPatrolDataSet; indices are preserved even if a point is absent.
#[derive(Clone, Copy)]
pub struct PatrolRoute<'a>(Object<'a>);

impl PatrolRoute<'_> {
    pub fn len(self) -> usize {
        self.0.get::<u32>(Field::at(12)) as usize
    }
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
    pub fn point(self, index: usize) -> Option<[f32; 3]> {
        if index >= self.len() {
            return None;
        }
        // 69E664: route +24 is a GU pointer array; each point stores XYZ at +16.
        let array = unsafe { Object::at(self.0.get(Field::at(24)))? };
        let point = unsafe { Object::at(array.get(Field::at(index.checked_mul(4)?)))? };
        Some(point.get(Field::at(16)))
    }
}
