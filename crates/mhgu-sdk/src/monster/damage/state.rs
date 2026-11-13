//! Health, part breaks and flinch counters.
use crate::{memory::Field, monster::MonsterState};

const HP: Field<i32> = Field::at(880);
const MAX_HP: Field<i32> = Field::at(884);
const BREAK_FLAGS: Field<u16> = Field::at(948);
const FLINCH_FLAGS: Field<u8> = Field::at(23755);

impl MonsterState<'_> {
    pub fn hp(self) -> i32 {
        self.0.get(HP)
    }
    pub fn max_hp(self) -> i32 {
        self.0.get(MAX_HP)
    }
    pub fn tail_severed(self) -> bool {
        self.0.get(BREAK_FLAGS) & 1 != 0
    }
    pub fn flinch_part_and_count(self) -> (u8, u8) {
        let part = (self.0.get(FLINCH_FLAGS) >> 3) & 7;
        (part, self.0.get(Field::at(956 + 12 * usize::from(part))))
    }
}
