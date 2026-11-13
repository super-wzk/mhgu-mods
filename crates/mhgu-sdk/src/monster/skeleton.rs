//! Monster joint transforms.
use super::MonsterUpdate;
use crate::memory::{Field, entry};

impl MonsterUpdate<'_> {
    /// Transform a model-joint offset to world coordinates. GU uses the root for unmapped joints.
    pub fn joint_world_position(self, joint: u8, local: [f32; 3]) -> Option<[f32; 3]> {
        let actor = self.monster();
        if actor.0.get(Field::<u32>::at(1176)) == 0 || actor.0.get(Field::<u32>::at(1172)) == 0 {
            return None;
        }
        let lookup: unsafe extern "C" fn(u32, i32) -> *const [[f32; 4]; 4] =
            unsafe { core::mem::transmute(entry(0xc1628)) };
        let m = unsafe { lookup(actor.as_raw(), joint.into()).read_unaligned() };
        Some(core::array::from_fn(|i| {
            m[3][i] + m[0][i] * local[0] + m[1][i] * local[1] + m[2][i] * local[2]
        }))
    }
}
