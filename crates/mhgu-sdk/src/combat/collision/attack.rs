//! Read-only monster attack shapes expanded by GU for the current collision pass.
use crate::memory::{Field, Object, entry};

/// The incoming attacks in the local quest hunter's collision pass.
#[repr(C)]
pub struct AttackScene<'a> {
    manager: Object<'a>,
    hunter: Object<'a>,
}

impl AttackScene<'_> {
    /// # Safety
    /// Call with the live local quest hunter and manager after GU 16ABAC has
    /// expanded the attack list, before GU 16B15C starts settling hits.
    #[doc(hidden)]
    pub unsafe fn from_raw(manager: u32, hunter: u32) -> Option<Self> {
        Some(Self {
            manager: unsafe { Object::at(manager) }?,
            hunter: unsafe { Object::at(hunter) }?,
        })
    }

    pub fn owner(&self) -> u32 {
        self.hunter.addr()
    }

    /// Delta of GU's unscaled scene clock, in 60 Hz frames.
    pub fn frames(&self) -> f32 {
        super::scene_frames()
    }

    /// Test a world-space sphere against active monster strikes and projectiles
    /// targeting this hunter. This query leaves native hit settlement untouched.
    pub fn overlaps_monster_attack(&self, center: [f32; 3], radius: f32) -> bool {
        if radius <= 0. || !radius.is_finite() || !center.into_iter().all(f32::is_finite) {
            return false;
        }
        let [x, y, z] = center;
        let sphere = [x, y, z, radius];
        let area = self.hunter.get::<u8>(Field::at(4180));
        let mut next = self.manager.get(Field::at(8224));
        while let Some(hit) = unsafe { Object::at(next) } {
            next = hit.get(Field::at(44));
            // Match GU 16B15C's activity, area, hunter mask and target filter.
            // Owner type 1 is a monster, including the root owner of projectiles.
            let owner = hit.get::<u8>(Field::at(16));
            if (owner == 1 || owner >= 4 && hit.get::<u8>(Field::at(110)) == 1)
                && hit.get::<u8>(Field::at(5)) != 0
                && hit.get::<f32>(Field::at(92)) <= 0.
                && hit.get::<u8>(Field::at(7)) == area
                && hit.get::<u8>(Field::at(8)) & 1 != 0
                && (hit.get::<u8>(Field::at(50)) & 0x80 == 0
                    || hit.get::<u32>(Field::at(140)) == self.owner())
                && !matches!(hit.get::<u8>(Field::at(58)), 8 | 9 | 0x34 | 0x45)
                && self.overlaps_hit(hit, &sphere)
            {
                return true;
            }
        }
        false
    }

    fn overlaps_hit(&self, hit: Object<'_>, sphere: &[f32; 4]) -> bool {
        let Some(resource) = (unsafe { Object::at(hit.get(Field::at(132))) }) else {
            return false;
        };
        let Some(vtable) = (unsafe { Object::at(resource.get(Field::at(0))) }) else {
            return false;
        };
        let get_group: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(vtable.get::<u32>(Field::at(68))) };
        let group = unsafe { get_group(resource.addr(), hit.get(Field::at(136))) };
        let Some(group) = (unsafe { Object::at(group) }) else {
            return false;
        };
        let Some(rows) = (unsafe { Object::at(group.get(Field::at(8))) }) else {
            return false;
        };
        let used = self.manager.get::<usize>(Field::at(10288)).min(64);
        let first = hit.get::<usize>(Field::at(28)).min(used);
        let count = group.get::<usize>(Field::at(4)).min(used - first);
        (0..count).any(|index| {
            rows.get::<u32>(Field::at(4 * index)) != 0 && self.overlaps_shape(first + index, sphere)
        })
    }

    fn overlaps_shape(&self, index: usize, sphere: &[f32; 4]) -> bool {
        // Same pool and stride as GU 16BB38 and 1706A8.
        let shape = unsafe { self.manager.embedded(8240 + 32 * index) };
        let kind = shape.get::<u8>(Field::at(0));
        if !(1..=3).contains(&kind) {
            return false;
        }
        let radius = shape.get::<f32>(Field::at(4));
        let [x, y, z] = shape.get::<[f32; 3]>(Field::at(8));
        if kind == 1 {
            let [dx, dy, dz] = [x - sphere[0], y - sphere[1], z - sphere[2]];
            let reach = radius + sphere[3];
            return dx * dx + dy * dy + dz * dz <= reach * reach;
        }
        let offset = match kind {
            2 => 0x168198, // sphere and capsule
            3 => 0x168280, // sphere and flat-ended cylinder
            _ => return false,
        };
        let [ex, ey, ez] = shape.get::<[f32; 3]>(Field::at(20));
        let volume = [x, y, z, 0., ex, ey, ez, 0., radius];
        let mut point = [0.; 4];
        let test: unsafe extern "C" fn(*const f32, *const f32, *mut f32) -> u32 =
            unsafe { core::mem::transmute(entry(offset)) };
        unsafe { test(sphere.as_ptr(), volume.as_ptr(), point.as_mut_ptr()) != 0 }
    }
}
