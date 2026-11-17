//! Shared services accessed during a live quest callback on the game thread.
use crate::memory::{Field, Object, entry, singleton};
use core::{cell::UnsafeCell, marker::PhantomData};

const PLAYERS: u32 = 0x1885b58;
const AREAS: u32 = 0x18977a0;
const PLAYER_AREA: Field<u8> = Field::at(4180);
const PLAYER_POSITION: Field<[f32; 3]> = Field::at(64);

/// Borrows the active quest for this callback. Cannot move to another thread.
#[derive(Clone, Copy)]
pub struct Quest<'a>(PhantomData<&'a UnsafeCell<()>>);

impl<'a> Quest<'a> {
    /// The caller must be in a live quest callback on the owning thread throughout 'a.
    pub(crate) unsafe fn current() -> Self {
        Self(PhantomData)
    }

    /// Draw once from GU's gameplay RNG (496F0).
    pub fn random_u32(self) -> u32 {
        let draw: unsafe extern "C" fn() -> u32 = unsafe { core::mem::transmute(entry(0x496f0)) };
        unsafe { draw() }
    }

    /// GU C4BA8: the hunter must be present, active and in the requested area.
    pub fn player_position_in_area(self, area: u8, slot: u8) -> Option<[f32; 3]> {
        if slot >= 4 {
            return None;
        }
        unsafe {
            let manager = singleton(PLAYERS)?;
            let player: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(entry(0x274ce4));
            let player = Object::at(player(manager.addr(), slot.into()))?;
            if player.get(PLAYER_AREA) != area {
                return None;
            }
            let vtable = Object::at(player.get(Field::<u32>::at(0)))?;
            let active: unsafe extern "C" fn(u32) -> u32 =
                core::mem::transmute(vtable.get(Field::<u32>::at(136)));
            let unavailable: unsafe extern "C" fn(u32) -> u32 =
                core::mem::transmute(entry(0x49240));
            if active(player.addr()) != 1
                || unavailable(player.get(Field::<u8>::at(4177)).into()) != 0
            {
                return None;
            }
            let pos = player.get(PLAYER_POSITION);
            pos.iter().all(|v| v.is_finite()).then_some(pos)
        }
    }

    pub fn area(self, id: u8) -> Option<Area<'a>> {
        let manager = unsafe { singleton(AREAS)? };
        let find: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0x4ff9c0)) };
        unsafe { Object::at(find(manager.addr(), id.into())) }.map(Area)
    }
}

#[derive(Clone, Copy)]
pub struct Area<'a>(Object<'a>);

impl<'a> Area<'a> {
    /// Iterate live navigation nodes in engine order, skipping absent entries.
    pub fn nodes(self) -> impl Iterator<Item = AreaNode<'a>> {
        let count = self.0.get(Field::at(104));
        let at: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0x3ce060)) };
        (0..count).filter_map(move |i| unsafe { Object::at(at(self.0.addr(), i)) }.map(AreaNode))
    }
}

#[derive(Clone, Copy)]
pub struct AreaNode<'a>(Object<'a>);

impl AreaNode<'_> {
    pub fn position(self) -> [f32; 3] {
        self.0.get(Field::at(16))
    }
}
