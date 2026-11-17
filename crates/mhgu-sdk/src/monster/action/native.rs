//! Native action selection and interpreter state.
use super::ActionId;
use crate::{
    memory::Field,
    monster::{Monster, MonsterState, MonsterUpdate},
};

const ACTION: Field<[u8; 2]> = Field::at(29664);
// AD7B4 measures elapsed time for the unchanged GU action group/slot.
const ACTION_ELAPSED: Field<f32> = Field::at(5112);
const ACTION_STAGE: Field<u8> = Field::at(417);

impl Monster<'_> {
    /// Native action group and slot, independent of the current animation.
    pub fn action(self) -> ActionId {
        let [group, slot] = self.0.get(ACTION);
        ActionId { group, slot }
    }
}

impl MonsterState<'_> {
    pub fn action_stage(self) -> u8 {
        self.0.get(ACTION_STAGE)
    }
    pub fn mark_action_started(self) {
        self.0.set(ACTION_STAGE, 1);
    }
}

impl MonsterUpdate<'_> {
    /// Restart GU's stuck-action timer after a scripted action advances within
    /// the same native group/slot (a new phase or a completed intentional loop).
    pub fn note_action_progress(self) {
        self.monster().0.set(ACTION_ELAPSED, 0.);
    }
}
