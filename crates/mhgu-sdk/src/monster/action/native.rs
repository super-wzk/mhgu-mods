//! Native action selection and interpreter state.
use super::ActionId;
use crate::{
    memory::Field,
    monster::{Monster, MonsterState},
};

const ACTION: Field<[u8; 2]> = Field::at(29664);
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
