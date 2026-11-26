//! Monster hurt volumes for effects that only need geometric contact.
use crate::memory::{Field, Object, entry, singleton};
use crate::weapon::style::particle::{ground_heights, transform_offset};

const ROSTER: u32 = 0x1851fe0;
const BACKGROUND: u32 = 0x21270e0;
const AREA: Field<u8> = Field::at(4180);
const MONSTER_ID: Field<u16> = Field::at(46576);

/// A monster part and an impact point in its joint's local coordinates.
/// This value can be kept across passes. Its position is resolved against the
/// current roster before any native object is accessed.
#[derive(Clone, Copy)]
pub struct Target {
    slot: u8,
    monster: u32,
    status: u32,
    id: u16,
    area: u8,
    pub part: u16,
    joint: u16,
    offset: [f32; 3],
}

/// Borrowed during a registered style's local-hunter collision pass.
#[repr(transparent)]
pub struct HurtScene<'a>(Object<'a>);

#[repr(C, align(16))]
#[derive(Default)]
struct Capsule {
    start: [f32; 4],
    end: [f32; 4],
    radius: f32,
}

#[repr(C, align(16))]
struct NativeShape {
    bone: u16,
    secondary: u16,
    kind: u16,
    scale_with_joint: u16,
    radius: f32,
    _pad: f32,
    start: [f32; 4],
    end: [f32; 4],
    owner_kind: u32,
    scale: f32,
}

fn vector([x, y, z]: [f32; 3]) -> [f32; 4] {
    [x, y, z, 0.]
}

fn xyz([x, y, z, _]: [f32; 4]) -> [f32; 3] {
    [x, y, z]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}

impl<'a> HurtScene<'a> {
    /// # Safety
    /// The registry must supply the live local quest hunter inside GU's
    /// collision update. The scene must stay on the game thread.
    #[doc(hidden)]
    pub unsafe fn from_raw(owner: u32) -> Option<Self> {
        unsafe { Object::at(owner) }.map(Self)
    }

    pub fn owner(&self) -> u32 {
        self.0.addr()
    }

    pub fn area(&self) -> u8 {
        self.0.get(AREA)
    }

    /// Delta of GU's unscaled scene clock, in 60 Hz frames.
    pub fn frames(&self) -> f32 {
        let frames =
            unsafe { singleton(0x2126764) }.map_or(0., |clock| clock.get::<f32>(Field::at(104)));
        if frames.is_finite() {
            frames.max(0.)
        } else {
            0.
        }
    }

    /// Test the moving point against the same background geometry query GU
    /// uses for stage collision. Returns the first obstruction, if any.
    pub fn terrain(&self, start: [f32; 3], end: [f32; 3]) -> Option<[f32; 3]> {
        unsafe { singleton(BACKGROUND) }?;
        let start = vector(start);
        let end = vector(end);
        let mut point = [0.; 4];
        let query: unsafe extern "C" fn(
            u32,
            *const f32,
            *const f32,
            u32,
            *mut f32,
            u32,
            u32,
            u32,
        ) -> u32 = unsafe { core::mem::transmute(entry(0x49c0c)) };
        let hit = unsafe {
            query(
                self.owner(),
                start.as_ptr(),
                end.as_ptr(),
                u32::from(self.area()),
                point.as_mut_ptr(),
                0,
                0,
                0,
            )
        };
        (hit == 1).then(|| xyz(point))
    }

    /// Highest floor or special surface at this point in the owner's area.
    pub fn surface_height(&self, position: [f32; 3]) -> Option<f32> {
        let (floor, surface) = unsafe { ground_heights(self.owner(), position) }?;
        Some(surface.map_or(floor, |surface| surface.max(floor)))
    }

    fn monster(&self, roster: Object<'a>, slot: u8) -> Option<(Object<'a>, Object<'a>)> {
        let get: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0x5ceb4)) };
        let excluded: unsafe extern "C" fn(u32) -> u32 =
            unsafe { core::mem::transmute(entry(0x7e11c)) };
        let monster = unsafe { Object::at(get(roster.addr(), u32::from(slot))) }?;
        if monster.get::<u8>(Field::at(46585)) == 0
            || monster.get::<u8>(Field::at(30290)) != 1
            || monster.get(AREA) != self.area()
            || unsafe { excluded(monster.addr()) } != 0
        {
            return None;
        }
        let status = unsafe { Object::at(monster.get(Field::at(5160))) }?;
        (status.get::<u8>(Field::at(416)) != 0 && status.get::<i32>(Field::at(880)) > 0)
            .then_some((monster, status))
    }

    /// Resolve the impact point on the moving monster, including joint rotation.
    pub fn position(&self, target: &Target) -> Option<[f32; 3]> {
        if target.area != self.area() {
            return None;
        }
        let (monster, status) = self.monster(unsafe { singleton(ROSTER) }?, target.slot)?;
        if (monster.addr(), status.addr()) != (target.monster, target.status)
            || monster.get(MONSTER_ID) != target.id
            || part_disabled(monster, target.part)
        {
            return None;
        }
        Some(transform_offset(
            &joint_matrix(monster, target.joint),
            target.offset,
        ))
    }

    /// Sweep a sphere against active monster body parts, without submitting an
    /// attack or writing to GU's attack lists. Returns the closest contact.
    pub fn sweep(&self, start: [f32; 3], end: [f32; 3], radius: f32) -> Option<Target> {
        let roster = unsafe { singleton(ROSTER) }?;
        let ray = core::array::from_fn(|i| end[i] - start[i]);
        let length_squared = dot(ray, ray);
        if !(length_squared.is_finite() && length_squared > 0. && radius.is_finite() && radius > 0.)
        {
            return None;
        }
        let sweep = Capsule {
            start: vector(start),
            end: vector(end),
            radius,
        };
        let mut nearest = None;
        let mut distance = f32::INFINITY;
        for slot in 0..10 {
            let Some((monster, status)) = self.monster(roster, slot) else {
                continue;
            };
            let rows: unsafe extern "C" fn(u32) -> u32 =
                unsafe { core::mem::transmute(entry(0x710b0)) };
            let Some(rows) = (unsafe { Object::at(rows(monster.addr())) }) else {
                continue;
            };
            // Match 16ABAC's expansion inputs. Expanding here preserves the
            // raw part index when native expansion skips a disabled shape.
            for index in 0..255 {
                let row = unsafe { rows.embedded(index * 44) };
                let bone = row.get::<u16>(Field::at(0));
                if bone == u16::MAX {
                    break;
                }
                let flags = row.get::<u16>(Field::at(10));
                let part = row.get::<u16>(Field::at(12));
                if flags & 0x8152 != 0 || part_disabled(monster, part) {
                    continue;
                }
                let Some(point) = intersect(monster, row, flags, &sweep) else {
                    continue;
                };
                let delta = core::array::from_fn(|i| point[i] - start[i]);
                let along = dot(delta, ray).clamp(0., length_squared);
                if !along.is_finite() || along >= distance {
                    continue;
                }
                distance = along;
                let matrix = joint_matrix(monster, bone);
                let delta = core::array::from_fn(|i| point[i] - matrix[12 + i]);
                let offset = core::array::from_fn(|axis| {
                    let i = 4 * axis;
                    let basis = [matrix[i], matrix[i + 1], matrix[i + 2]];
                    let scale = dot(basis, basis);
                    if scale > 0. {
                        dot(basis, delta) / scale
                    } else {
                        0.
                    }
                });
                nearest = Some(Target {
                    slot,
                    monster: monster.addr(),
                    status: status.addr(),
                    area: self.area(),
                    id: monster.get(MONSTER_ID),
                    part,
                    joint: bone,
                    offset,
                });
            }
        }
        nearest
    }
}

fn part_disabled(monster: Object<'_>, part: u16) -> bool {
    let vtable = monster.get::<*const usize>(Field::at(0));
    let check: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(vtable.add(880 / 4).read()) };
    unsafe { check(monster.addr(), u32::from(part)) != 0 }
}

fn joint_matrix(monster: Object<'_>, joint: u16) -> [f32; 16] {
    let vtable = monster.get::<*const usize>(Field::at(0));
    let get: unsafe extern "C" fn(u32, i32) -> u32 =
        unsafe { core::mem::transmute(vtable.add(208 / 4).read()) };
    let matrix = if joint < 1000 {
        unsafe { Object::at(get(monster.addr(), i32::from(joint))) }
    } else {
        None
    };
    if let Some(matrix) = matrix {
        matrix.get(Field::at(0))
    } else {
        let [x, y, z] = monster.get(Field::at(64));
        [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., x, y, z, 1.]
    }
}

fn intersect(
    monster: Object<'_>,
    row: Object<'_>,
    flags: u16,
    sweep: &Capsule,
) -> Option<[f32; 3]> {
    let input = NativeShape {
        bone: row.get(Field::at(0)),
        secondary: row.get(Field::at(2)),
        kind: row.get(Field::at(4)),
        scale_with_joint: (flags >> 12) & 1,
        radius: row.get(Field::at(16)),
        _pad: 0.,
        start: vector(row.get(Field::at(20))),
        end: vector(row.get(Field::at(32))),
        owner_kind: 1,
        scale: 1.,
    };
    let expand: unsafe extern "C" fn(u32, u32, *const NativeShape, *mut Capsule) -> u32 =
        unsafe { core::mem::transmute(entry(0x16de70)) };
    let mut volume = Capsule::default();
    let kind = unsafe {
        expand(
            u32::from(input.scale_with_joint),
            monster.addr(),
            &input,
            &mut volume,
        )
    };
    let sphere = [
        volume.start[0],
        volume.start[1],
        volume.start[2],
        volume.radius,
    ];
    let (offset, native) = match kind {
        1 => (0x167f64, sphere.as_ptr()),
        2 => (0x167b5c, volume.start.as_ptr()),
        3 => (0x1682f8, volume.start.as_ptr()),
        _ => return None,
    };
    let test: unsafe extern "C" fn(*const Capsule, *const f32, *mut f32) -> u32 =
        unsafe { core::mem::transmute(entry(offset)) };
    let mut point = [0.; 4];
    (unsafe { test(sweep, native, point.as_mut_ptr()) } != 0).then(|| xyz(point))
}
