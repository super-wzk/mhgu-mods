//! Health, part breaks and flinch counters.
use super::HitzoneRow;
use crate::{
    memory::{Field, Object, entry},
    monster::{Monster, MonsterState},
};

const HP: Field<i32> = Field::at(880);
const MAX_HP: Field<i32> = Field::at(884);
const BREAK_FLAGS: Field<u16> = Field::at(948);
const FLINCH_FLAGS: Field<u8> = Field::at(23755);

impl Monster<'_> {
    /// Select a native damage row for this instance. Both DTT tables have eight
    /// slots; a missing table or invalid index leaves the current row alone.
    pub fn select_hitzone(self, slot: u8, row: HitzoneRow) -> bool {
        if slot >= 8 || self.state().is_none() {
            return false;
        }
        let Some(tune) = (unsafe { Object::at(self.0.get(Field::at(30188))) }) else {
            return false;
        };
        unsafe {
            match row {
                HitzoneRow::Default if tune.get(Field::<u32>::at(112)) != 0 => {
                    let bind: unsafe extern "C" fn(u32, u32) -> u32 =
                        core::mem::transmute(entry(0xbab88));
                    bind(self.as_raw(), slot.into()) != 0
                }
                HitzoneRow::Alternate(i) if i < 8 => {
                    // BABB8 handles an absent alternate table without altering the slot.
                    let bind: unsafe extern "C" fn(u32, u32, u32) -> u32 =
                        core::mem::transmute(entry(0xbabb8));
                    bind(self.as_raw(), slot.into(), i.into()) != 0
                }
                _ => false,
            }
        }
    }
}

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
