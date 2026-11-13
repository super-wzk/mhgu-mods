//! Live GU monster handles and the scope of an action update.
use crate::memory::{Field, Object, entry};

/// Native class identified by its main-module vtable, independent of a quest's species ID.
#[derive(Clone, Copy)]
pub struct MonsterClass(u32);
impl MonsterClass {
    pub const fn at_vtable(offset: u32) -> Self {
        Self(offset)
    }

    pub fn matches(self, actor: Monster) -> bool {
        let base = unsafe { entry(0) } as u32;
        base.checked_add(self.0) == Some(actor.0.get(Field::<u32>::at(0)))
    }
}

// The state block is addressed by a guest pointer even on a 64-bit recomp host.
const STATE_BLOCK: Field<u32> = Field::at(5160);

/// A game-owned monster on the current game thread. Copies do not retain the actor.
#[derive(Clone, Copy)]
pub struct Monster<'a>(pub(super) Object<'a>);
/// A borrowed view of the monster's live state block. Copies refer to the same engine memory.
#[derive(Clone, Copy)]
pub struct MonsterState<'a>(pub(super) Object<'a>);

impl<'a> Monster<'a> {
    /// Native time step for this actor's update.
    pub fn frame_delta(self) -> f32 {
        self.0.get(Field::at(28))
    }

    /// Borrow a monster from an engine callback; null denotes no monster.
    ///
    /// # Safety
    /// A non-null address must be an initialized MHGU 1.4 monster. The caller
    /// chooses `'a`: the actor and any accessed state block, scene or resource data must stay live
    /// and accessible for that duration, on the owning game thread. Do not
    /// retain these handles across callbacks or actor/resource destruction.
    pub unsafe fn from_raw(addr: u32) -> Option<Self> {
        unsafe { Object::at(addr) }.map(Self)
    }
    pub fn as_raw(self) -> u32 {
        self.0.addr()
    }
    /// Borrow the current state block, if the engine has attached one.
    pub fn state(self) -> Option<MonsterState<'a>> {
        unsafe { Object::at(self.0.get(STATE_BLOCK)) }.map(MonsterState)
    }
}

impl MonsterState<'_> {
    pub fn as_raw(self) -> u32 {
        self.0.addr()
    }
}

/// Proof of a live, game-thread action update. Cannot be obtained from a general Monster handle.
#[derive(Clone, Copy)]
pub struct MonsterUpdate<'a>(Monster<'a>);
impl<'a> MonsterUpdate<'a> {
    /// # Safety
    /// Only construct during an active quest inside a native monster action update (e.g. CF5F64), with the
    /// actor, model, RNG and gameplay resources initialized and live throughout 'a.
    /// Never retain this token after the callback, including across scene changes.
    pub unsafe fn from_callback(addr: u32) -> Option<Self> {
        unsafe { Monster::from_raw(addr) }.map(Self)
    }
    pub fn monster(self) -> Monster<'a> {
        self.0
    }
}
