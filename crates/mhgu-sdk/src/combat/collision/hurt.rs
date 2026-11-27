//! Monster hurt volumes for effects that only need geometric contact.
use crate::memory::{Field, Object, entry, singleton};
use crate::weapon::style::particle::{self, Anchor, Bank, ground_heights, transform_offset};

mod ground_hold;
pub use ground_hold::{
    GroundHold, GroundHoldService, end_ground_hold_replica, initialize_ground_hold,
    prepare_ground_topple, publish_ground_hold,
};

const ROSTER: u32 = 0x1851fe0;
const BACKGROUND: u32 = 0x21270e0;
const AREA: Field<u8> = Field::at(4180);
const MONSTER_ID: Field<u16> = Field::at(46576);
const SET_ID: Field<u16> = Field::at(438);
const REVISION: Field<u8> = Field::at(23744);
const OWNED: Field<u8> = Field::at(29728);

/// Identity of a GU monster spawn during a hunt, independent of hit part.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(C)]
pub struct MonsterSpawn {
    pub kind: u16,
    pub set_id: u16,
    pub revision: u8,
}

impl MonsterSpawn {
    fn from_live(monster: Object<'_>, status: Object<'_>) -> Self {
        Self {
            kind: monster.get(MONSTER_ID),
            set_id: status.get(SET_ID),
            revision: status.get(REVISION),
        }
    }

    /// Capture the identity of a living target in the hunter's area.
    ///
    /// # Safety
    /// GU's damage dispatcher must have supplied the live hunter and target.
    #[doc(hidden)]
    pub unsafe fn from_damage_target(owner: u32, target: u32) -> Option<Self> {
        let owner = unsafe { Object::at(owner) }?;
        let monster = unsafe { Object::at(target) }?;
        let area = owner.get(AREA);
        let status = live_status(monster, area)?;
        Some(Self::from_live(monster, status))
    }

    /// # Safety
    /// GU's damage dispatcher must have supplied the live hunter and target.
    #[doc(hidden)]
    pub unsafe fn from_owned_damage_target(hunter: u32, target: u32) -> Option<Self> {
        let monster = unsafe { Object::at(target) }?;
        if monster.get(OWNED) == 0 {
            return None;
        }
        unsafe { Self::from_damage_target(hunter, target) }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum QuestRank {
    Low,
    High,
    Arena,
    G,
}

/// A living monster in the local hunter's area during this collision pass.
pub struct Monster<'a> {
    actor: Object<'a>,
    status: Object<'a>,
    spawn: MonsterSpawn,
    area: u8,
    slot: u8,
}

impl Monster<'_> {
    pub fn spawn(&self) -> MonsterSpawn {
        self.spawn
    }

    /// Whether this console controls this monster's simulation in GU.
    pub fn locally_owned(&self) -> bool {
        self.actor.get(OWNED) != 0
    }

    /// GU room member currently controlling a replicated monster.
    pub fn network_controller(&self) -> Option<u8> {
        (self.status.get::<u8>(Field::at(424)) & 8 != 0 && self.spawn.set_id != u16::MAX)
            .then(|| self.actor.get(Field::at(29732)))
    }

    /// This client's roster slot for the current spawn.
    pub fn local_slot(&self) -> u8 {
        self.slot
    }

    /// Joints used by current, enabled body hurt volumes in native row order.
    pub fn hurt_joints(&self) -> impl Iterator<Item = u16> + '_ {
        active_rows(self.actor)
            .map(|(_, row, _)| row.get::<u16>(Field::at(0)))
            .filter(|&joint| joint < 1000)
    }

    /// Test a sphere against enabled body volumes without submitting damage.
    pub fn overlaps(&self, center: [f32; 3], radius: f32) -> bool {
        if radius <= 0. || !radius.is_finite() || !center.into_iter().all(f32::is_finite) {
            return false;
        }
        let [x, y, z] = center;
        let sphere = [x, y, z, radius];
        active_rows(self.actor).any(|(_, row, flags)| {
            let (kind, shape) = expand_shape(self.actor, row, flags);
            match kind {
                1 => {
                    let delta = core::array::from_fn(|i| center[i] - shape.start[i]);
                    let reach = radius + shape.radius;
                    dot(delta, delta) <= reach * reach
                }
                2 | 3 => {
                    let offset = if kind == 2 { 0x168198 } else { 0x168280 };
                    let query: unsafe extern "C" fn(*const f32, *const Capsule, *mut f32) -> u32 =
                        unsafe { core::mem::transmute(entry(offset)) };
                    let mut point = [0.; 4];
                    unsafe { query(sphere.as_ptr(), &shape, point.as_mut_ptr()) != 0 }
                }
                _ => false,
            }
        })
    }

    /// World translation of a bone in the monster's current skeleton.
    pub fn joint_position(&self, joint: u16) -> Option<[f32; 3]> {
        let matrix = live_joint_matrix(self.actor, joint)?;
        let position = [matrix[12], matrix[13], matrix[14]];
        position
            .iter()
            .all(|axis| axis.is_finite())
            .then_some(position)
    }

    /// Emit at a live bone with fixed world yaw. GU tracks the particle's
    /// model parent through the same attachment used by its monster effects.
    pub fn emit_joint_particle(
        &self,
        bank: &Bank,
        route: usize,
        joint: u16,
        accepted: impl FnMut(u32),
    ) {
        if self.joint_position(joint).is_some()
            && let Some(parent) = unsafe { particle::parent(self.actor.addr()) }
        {
            unsafe {
                bank.emit_tracked(
                    route,
                    Anchor::JointYaw {
                        parent,
                        joint: joint as i16,
                        offset: None,
                        yaw: 0,
                    },
                    accepted,
                )
            };
        }
    }
}

/// GU monster identity and contact relative to a hurt volume's joint.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Contact {
    pub set_id: u16,
    pub revision: u8,
    pub row: u8,
    pub part: u16,
    pub offset: [f32; 3],
}

/// A monster part and an impact point in its joint's local coordinates.
/// This value can be kept across passes. Its position is resolved against the
/// current roster before any native object is accessed.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Target {
    slot: u8,
    monster: u32,
    status: u32,
    id: u16,
    area: u8,
    joint: u16,
    contact: Contact,
}

impl Target {
    /// The GU roster slot on this console, which can differ between peers.
    pub fn local_slot(&self) -> u8 {
        self.slot
    }

    pub fn spawn(&self) -> MonsterSpawn {
        MonsterSpawn {
            kind: self.id,
            set_id: self.contact.set_id,
            revision: self.contact.revision,
        }
    }

    pub fn contact(&self) -> Contact {
        self.contact
    }

    /// Resolve this contact while the owner's action is being updated.
    ///
    /// # Safety
    /// The owner must be a live local quest hunter on the game thread.
    pub unsafe fn position_for(&self, owner: u32) -> Option<[f32; 3]> {
        HurtScene(unsafe { Object::at(owner) }?).position(self)
    }
}

#[derive(Debug)]
pub struct LostTarget;

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

fn live_status(monster: Object<'_>, area: u8) -> Option<Object<'_>> {
    let excluded: unsafe extern "C" fn(u32) -> u32 =
        unsafe { core::mem::transmute(entry(0x7e11c)) };
    if monster.get::<u8>(Field::at(46585)) == 0
        || monster.get::<u8>(Field::at(30290)) != 1
        || monster.get(AREA) != area
        || unsafe { excluded(monster.addr()) } != 0
    {
        return None;
    }
    let status = unsafe { Object::at(monster.get(Field::at(5160))) }?;
    (status.get::<u8>(Field::at(416)) != 0 && status.get::<i32>(Field::at(880)) > 0)
        .then_some(status)
}

impl<'a> HurtScene<'a> {
    /// # Safety
    /// The registry must supply the live local quest hunter on the game
    /// thread. The scene may be used only during the current GU callback.
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
        super::scene_frames()
    }

    fn quest(&self) -> Option<Object<'_>> {
        let quest = unsafe { singleton(0x188ad90) }?;
        (quest.get::<u32>(Field::at(164)) != 0).then_some(quest)
    }

    /// Whether GU currently has quest data loaded.
    pub fn has_active_quest(&self) -> bool {
        self.quest().is_some()
    }

    /// Difficulty of the active GU quest, if quest data is loaded.
    pub fn quest_rank(&self) -> Option<QuestRank> {
        let quest = self.quest()?;
        let rank: unsafe extern "C" fn(u32) -> u32 =
            unsafe { core::mem::transmute(entry(0x3a8bdc)) };
        match unsafe { rank(quest.addr()) } {
            1 => Some(QuestRank::Low),
            2 => Some(QuestRank::High),
            3 => Some(QuestRank::Arena),
            4 => Some(QuestRank::G),
            _ => None,
        }
    }

    /// Same-area living monsters in GU's roster order.
    pub fn monsters(&self) -> impl Iterator<Item = Monster<'_>> + '_ {
        let roster = unsafe { singleton(ROSTER) };
        let area = self.area();
        (0..10).filter_map(move |slot| monster_in_area(roster?, slot, area))
    }

    /// Resolve a living spawn in this area by identity, independent of its slot.
    pub fn find_spawn(&self, spawn: MonsterSpawn) -> Option<Monster<'_>> {
        self.monsters().find(|monster| monster.spawn() == spawn)
    }

    /// Test the moving point against the same background geometry query GU
    /// uses for stage collision. Returns the first obstruction, if any.
    pub fn terrain(&self, start: [f32; 3], end: [f32; 3]) -> Option<[f32; 3]> {
        terrain_between(self.0, start, end)
    }

    /// Highest floor or special surface at this point in the owner's area.
    pub fn surface_height(&self, position: [f32; 3]) -> Option<f32> {
        let (floor, surface) = unsafe { ground_heights(self.owner(), position) }?;
        Some(surface.map_or(floor, |surface| surface.max(floor)))
    }

    fn monster(&self, roster: Object<'a>, slot: u8) -> Option<(Object<'a>, Object<'a>)> {
        let monster = monster_in_area(roster, slot, self.area())?;
        Some((monster.actor, monster.status))
    }

    /// Resolve the impact point on the moving monster, including joint rotation.
    pub fn position(&self, target: &Target) -> Option<[f32; 3]> {
        if target.area != self.area() {
            return None;
        }
        let (monster, status) = self.monster(unsafe { singleton(ROSTER) }?, target.slot)?;
        let contact = target.contact;
        if (monster.addr(), status.addr()) != (target.monster, target.status)
            || monster.get(MONSTER_ID) != target.id
            || status.get(SET_ID) != contact.set_id
            || status.get(REVISION) != contact.revision
            || part_disabled(monster, contact.part)
        {
            return None;
        }
        let point = transform_offset(&joint_matrix(monster, target.joint), contact.offset);
        point.iter().all(|axis| axis.is_finite()).then_some(point)
    }

    /// Refresh a persistent part mark. An unloaded or off-area spawn has no
    /// drawable point yet. A dead or replaced spawn permanently loses its mark.
    pub fn refresh_mark(&self, target: &mut Target) -> Result<Option<[f32; 3]>, LostTarget> {
        if let Some(point) = self.position(target) {
            return Ok(Some(point));
        }
        let Some(roster) = (unsafe { singleton(ROSTER) }) else {
            return Ok(None);
        };
        let get: unsafe extern "C" fn(u32, u32) -> u32 =
            unsafe { core::mem::transmute(entry(0x5ceb4)) };
        for slot in 0..10 {
            let Some(monster) = (unsafe { Object::at(get(roster.addr(), slot)) }) else {
                continue;
            };
            let Some(status) = (unsafe { Object::at(monster.get(Field::at(5160))) }) else {
                continue;
            };
            if target.contact.set_id == u16::MAX {
                if monster.addr() != target.monster || status.addr() != target.status {
                    continue;
                }
            } else if status.get(SET_ID) != target.contact.set_id {
                continue;
            }
            if (monster.get(MONSTER_ID), status.get(REVISION))
                != (target.id, target.contact.revision)
                || status.get::<i32>(Field::at(880)) <= 0
            {
                return Err(LostTarget);
            }
            if live_status(monster, self.area()).is_none() {
                return Ok(None);
            }
            if part_disabled(monster, target.contact.part) {
                return Err(LostTarget);
            }
            let Some(rows) = hurt_rows(monster) else {
                return Ok(None);
            };
            let row = (0..=target.contact.row)
                .map(|i| unsafe { rows.embedded(44 * usize::from(i)) })
                .take_while(|row| row.get::<u16>(Field::at(0)) != u16::MAX)
                .nth(usize::from(target.contact.row))
                .ok_or(LostTarget)?;
            if row.get::<u16>(Field::at(12)) != target.contact.part
                || row.get::<u16>(Field::at(0)) != target.joint
            {
                return Err(LostTarget);
            }
            target.slot = slot as u8;
            target.area = self.area();
            target.monster = monster.addr();
            target.status = status.addr();
            return Ok(self.position(target));
        }
        Ok(None)
    }

    /// Resolve a synchronized contact in the current area. The original
    /// hurt volume must still exist on the same spawn of the monster.
    pub fn resolve_contact(&self, contact: Contact) -> Option<Target> {
        if contact.row == u8::MAX || !contact.offset.iter().all(|axis| axis.is_finite()) {
            return None;
        }
        let roster = unsafe { singleton(ROSTER) }?;
        let (slot, monster, status) = (0..10).find_map(|slot| {
            let (monster, status) = self.monster(roster, slot)?;
            (status.get(SET_ID) == contact.set_id && status.get(REVISION) == contact.revision)
                .then_some((slot, monster, status))
        })?;
        let rows = hurt_rows(monster)?;
        let row = (0..=contact.row)
            .map(|index| unsafe { rows.embedded(usize::from(index) * 44) })
            .take_while(|row| row.get::<u16>(Field::at(0)) != u16::MAX)
            .nth(usize::from(contact.row))?;
        if row.get::<u16>(Field::at(12)) != contact.part
            || row.get::<u16>(Field::at(10)) & 0x8152 != 0
            || part_disabled(monster, contact.part)
        {
            return None;
        }
        Some(Target {
            slot,
            monster: monster.addr(),
            status: status.addr(),
            area: self.area(),
            id: monster.get(MONSTER_ID),
            joint: row.get(Field::at(0)),
            contact,
        })
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
            for (index, row, flags) in active_rows(monster) {
                let bone = row.get::<u16>(Field::at(0));
                let part = row.get::<u16>(Field::at(12));
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
                    joint: bone,
                    contact: Contact {
                        set_id: status.get(SET_ID),
                        revision: status.get(REVISION),
                        row: index,
                        part,
                        offset,
                    },
                });
            }
        }
        nearest
    }
}

fn monster_in_area(roster: Object<'_>, slot: u8, area: u8) -> Option<Monster<'_>> {
    let get: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(entry(0x5ceb4)) };
    let actor = unsafe { Object::at(get(roster.addr(), u32::from(slot))) }?;
    let status = live_status(actor, area)?;
    Some(Monster {
        actor,
        status,
        spawn: MonsterSpawn::from_live(actor, status),
        area,
        slot,
    })
}

fn hurt_rows(monster: Object<'_>) -> Option<Object<'_>> {
    let rows: unsafe extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(entry(0x710b0)) };
    unsafe { Object::at(rows(monster.addr())) }
}

fn active_rows(monster: Object<'_>) -> impl Iterator<Item = (u8, Object<'_>, u16)> {
    hurt_rows(monster).into_iter().flat_map(move |rows| {
        // Preserve raw row numbers when native expansion skips a disabled part.
        (0..u8::MAX)
            .map(move |index| (index, unsafe { rows.embedded(usize::from(index) * 44) }))
            .take_while(|(_, row)| row.get::<u16>(Field::at(0)) != u16::MAX)
            .filter_map(move |(index, row)| {
                let flags = row.get::<u16>(Field::at(10));
                (flags & 0x8152 == 0 && !part_disabled(monster, row.get(Field::at(12))))
                    .then_some((index, row, flags))
            })
    })
}

/// Run GU's intersection on an eligible captive row. A marked row must win
/// even when an earlier body volume overlaps the same world position.
///
/// # Safety
/// The manager and monster must be GU's live arguments to 16C0C0.
#[doc(hidden)]
pub unsafe fn with_restrained_row(
    manager: u32,
    target: u32,
    marked: Option<u8>,
    intersect: impl FnOnce([f32; 3]) -> u32,
) -> Option<u32> {
    let monster = unsafe { Object::at(target) }?;
    let status = unsafe { Object::at(monster.get(Field::at(5160))) }?;
    let manager = unsafe { Object::at(manager) }?;
    let base: u32 = status.get(Field::at(1280));
    let capacity = 256u32.checked_sub(base)?;
    let volumes =
        |index: u8| unsafe { manager.embedded(28 + 32 * (base + u32::from(index)) as usize) };
    let (index, point) = active_rows(monster)
        .take_while(|(i, _, _)| u32::from(*i) < capacity)
        .filter(|(i, _, _)| marked.is_none_or(|mark| mark == *i))
        .find_map(|(i, _, _)| {
            let shape = volumes(i);
            let point = shape.get::<[f32; 3]>(Field::at(8));
            (matches!(shape.get::<u8>(Field::at(0)), 1..=3)
                && point.into_iter().all(f32::is_finite))
            .then_some((i, point))
        })?;
    if marked.is_none() {
        return Some(intersect(point));
    }
    let rows = hurt_rows(monster)?;
    let mut saved = [0u8; 256];
    let count = (0..u8::MAX)
        .take_while(|&i| {
            u32::from(i) < capacity
                && unsafe { rows.embedded(44 * usize::from(i)) }.get::<u16>(Field::at(0))
                    != u16::MAX
        })
        .count();
    for (i, kind) in saved[..count].iter_mut().enumerate() {
        if i != usize::from(index) {
            let volume = volumes(i as u8);
            *kind = volume.get(Field::at(0));
            volume.set(Field::at(0), 0u8);
        }
    }
    let result = intersect(point);
    for (i, kind) in saved[..count].iter().copied().enumerate() {
        if kind != 0 {
            volumes(i as u8).set(Field::at(0), kind);
        }
    }
    Some(result)
}

pub(crate) fn terrain_between(
    owner: Object<'_>,
    start: [f32; 3],
    end: [f32; 3],
) -> Option<[f32; 3]> {
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
            owner.addr(),
            start.as_ptr(),
            end.as_ptr(),
            u32::from(owner.get(AREA)),
            point.as_mut_ptr(),
            0,
            0,
            0,
        )
    };
    (hit == 1).then(|| xyz(point))
}

fn part_disabled(monster: Object<'_>, part: u16) -> bool {
    let vtable = monster.get::<*const usize>(Field::at(0));
    let check: unsafe extern "C" fn(u32, u32) -> u32 =
        unsafe { core::mem::transmute(vtable.add(880 / 4).read()) };
    unsafe { check(monster.addr(), u32::from(part)) != 0 }
}

fn joint_matrix(monster: Object<'_>, joint: u16) -> [f32; 16] {
    live_joint_matrix(monster, joint).unwrap_or_else(|| {
        let [x, y, z] = monster.get(Field::at(64));
        [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., x, y, z, 1.]
    })
}

fn live_joint_matrix(monster: Object<'_>, joint: u16) -> Option<[f32; 16]> {
    if joint >= 1000 {
        return None;
    }
    let vtable = monster.get::<*const usize>(Field::at(0));
    let get: unsafe extern "C" fn(u32, i32) -> u32 =
        unsafe { core::mem::transmute(vtable.add(208 / 4).read()) };
    let matrix = unsafe { Object::at(get(monster.addr(), i32::from(joint))) }?;
    Some(matrix.get(Field::at(0)))
}

fn expand_shape(monster: Object<'_>, row: Object<'_>, flags: u16) -> (u32, Capsule) {
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
    (kind, volume)
}

fn intersect(
    monster: Object<'_>,
    row: Object<'_>,
    flags: u16,
    sweep: &Capsule,
) -> Option<[f32; 3]> {
    let (kind, volume) = expand_shape(monster, row, flags);
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
