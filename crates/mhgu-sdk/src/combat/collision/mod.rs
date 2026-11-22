//! GU collision contacts, shape submission and storage, independent of the owner type.

use crate::memory::{Field, Object, entry, singleton};
use alloc::boxed::Box;
use core::cell::UnsafeCell;

const CONTACT_ENABLED: Field<u8> = Field::at(5);
const CONTACT_OWNER: Field<u32> = Field::at(12);
const CONTACT_DELAY: Field<f32> = Field::at(92);
const CONTACT_DURATION: Field<f32> = Field::at(96);
const CONTACT_SHAPES: Field<u32> = Field::at(132);
const CONTACT_GROUP: Field<u32> = Field::at(136);
const CONTACT_FLAGS: Field<u32> = Field::at(48);

mod shapes;
pub use shapes::{CachedHitShape, HitShape, ShapeGroup, ShapeGroups};

#[derive(Clone, Copy)]
pub struct Contact<'a>(pub(crate) Object<'a>);
impl<'a> Contact<'a> {
    pub(crate) fn add_flags(self, flags: u32) {
        self.0.set(CONTACT_FLAGS, self.0.get(CONTACT_FLAGS) | flags);
    }

    /// # Safety
    /// The callback must supply a live GU contact on its owning thread for the duration of 'a.
    pub unsafe fn from_raw(addr: u32) -> Option<Self> {
        unsafe { Object::at(addr) }.map(Self)
    }
    pub fn as_raw(self) -> u32 {
        self.0.addr()
    }
    /// Owner identity only: not all contacts belong to monsters.
    pub fn owner_address(self) -> u32 {
        self.0.get(CONTACT_OWNER)
    }
    pub fn enabled(self) -> bool {
        self.0.get(CONTACT_ENABLED) != 0
    }
    pub fn disable(self) {
        self.0.set(CONTACT_ENABLED, 0u8);
    }
    pub fn delay(self) -> f32 {
        self.0.get(CONTACT_DELAY)
    }
    pub fn duration(self) -> f32 {
        self.0.get(CONTACT_DURATION)
    }
    /// Borrow this contact's retained HTS table, regardless of the owner's class.
    pub fn shape_groups(self) -> Option<ShapeGroups<'a>> {
        let resource = unsafe { Object::at(self.0.get(CONTACT_SHAPES)) }?;
        ShapeGroups::read(resource)
    }
    /// A contact retains its own HTS reference. Refuse handles from a different resource.
    pub fn set_shape_group(self, group: ShapeGroup) -> bool {
        if self.0.get(CONTACT_SHAPES) != group.resource.addr() {
            return false;
        }
        self.0.set(CONTACT_GROUP, u32::from(group.index));
        true
    }

    pub fn set_window(self, delay: f32, duration: f32) {
        self.0.set(CONTACT_DELAY, delay);
        self.0.set(CONTACT_DURATION, duration);
    }

    /// Initialize a player-owned world contact and apply the equipped weapon's modifiers.
    /// GU 16919C uses the global clock when the owner-action/owner-motion flags are clear.
    ///
    /// # Safety
    /// Run on the game thread with a live quest hunter, an HDP row, and a loaded HTS.
    pub unsafe fn prepare_world_attack(self, actor: u32, row: &[u32; 14], shape: u32) {
        unsafe {
            let init: unsafe extern "C" fn(u32, u32, u32, *const u32, u32, u32, u32, u32) =
                core::mem::transmute(entry(0x168960));
            let adjust: unsafe extern "C" fn(u32, u32, *const u32, u32) =
                core::mem::transmute(entry(0x2b76cc));
            init(self.as_raw(), 0, actor, row.as_ptr(), shape, 0, 15, 1);
            let mut flags = 0x0a00_0428;
            let mut modifiers = crate::style::action::Attack::WEAPON_MODIFIERS;
            if crate::style::hunter::equipped_weapon(actor) == Some(1) {
                // Match the SnS weapon bit from 2B72D4 and the draw check in 2B7ED8.
                flags |= 0x800;
                if *((actor + 10057) as *const u8) & 8 != 0 {
                    modifiers |= 0x10;
                }
            }
            self.0.set(CONTACT_FLAGS, flags);
            adjust(actor, self.as_raw(), row.as_ptr(), modifiers);
        }
    }

    /// Add this contact to GU's intrusive attack list once for the current pass.
    ///
    /// # Safety
    /// The manager must be the live argument to GU 16ABAC on the game thread.
    pub unsafe fn enqueue(self, manager: u32) {
        unsafe {
            let mut next = ((manager + 8224) as *const u32).read();
            while next != 0 {
                if next == self.as_raw() {
                    return;
                }
                next = ((next + 44) as *const u32).read();
            }
            let add: unsafe extern "C" fn(u32) = core::mem::transmute(entry(0x1696e4));
            add(self.as_raw());
        }
    }

    /// Submit a sphere at a fixed world position instead of expanding a bone-relative shape.
    ///
    /// # Safety
    /// Call in the native shape expansion pass for this contact.
    pub unsafe fn submit_world_sphere(self, center: [f32; 3], radius: f32) -> bool {
        let Some(manager) = (unsafe { self.begin_world_shapes() }) else {
            return false;
        };
        unsafe { Self::submit_world_shape(manager, 1, center, [0.; 3], radius) };
        true
    }

    /// Submit a capsule whose two endpoints are fixed in world space.
    ///
    /// # Safety
    /// Call in the native shape expansion pass for this contact.
    pub unsafe fn submit_world_capsule(self, ends: [[f32; 3]; 2], radius: f32) -> bool {
        (unsafe { self.submit_world_capsules(&[ends], radius) }) != 0
    }

    /// Submit a contiguous group of world-space capsules with a single contact
    /// serial. The retained HTS group must contain the same number of shapes.
    ///
    /// # Safety
    /// Call once in the native shape expansion pass for this contact.
    pub unsafe fn submit_world_capsules(self, ends: &[[[f32; 3]; 2]], radius: f32) -> u32 {
        let Some(manager) = (unsafe { self.begin_world_shapes() }) else {
            return 0;
        };
        for &[start, end] in ends {
            unsafe { Self::submit_world_shape(manager, 2, start, end, radius) };
        }
        ends.len() as u32
    }

    unsafe fn begin_world_shapes(self) -> Option<Object<'a>> {
        let manager = unsafe { singleton(0x1857830) }?;
        self.0
            .set(Field::at(28), manager.get(Field::<u32>::at(10288)));
        Some(manager)
    }

    unsafe fn submit_world_shape(
        manager: Object<'_>,
        kind: u32,
        start: [f32; 3],
        end: [f32; 3],
        radius: f32,
    ) {
        let mut shape = [0f32; 9];
        shape[..3].copy_from_slice(&start);
        shape[4..7].copy_from_slice(&end);
        shape[8] = radius;
        unsafe {
            let submit: unsafe extern "C" fn(u32, u32, *const f32) -> u32 =
                core::mem::transmute(entry(0x1706a8));
            submit(manager.addr(), kind, shape.as_ptr());
        }
    }
}

// Native storage is 184 bytes; keep each address aligned independently of its native size.
#[repr(C, align(16))]
struct Storage(UnsafeCell<[u32; 46]>);

/// A fixed allocation of native contacts. Moving the pool never moves its contacts.
/// GU retains submitted addresses; see the lifetime contract on new().
pub struct ContactPool(Box<[Storage]>);

impl ContactPool {
    /// # Safety
    /// Create and drop the pool on the owning game thread. Keep it alive until GU no longer
    /// holds any of its addresses (normally the owner release callback).
    pub unsafe fn new(count: usize) -> Self {
        let mut slots: Box<[Storage]> = (0..count)
            .map(|_| Storage(UnsafeCell::new([0; 46])))
            .collect();
        let init: unsafe extern "C" fn(*mut Storage) -> u32 =
            unsafe { core::mem::transmute(entry(0x1687a8)) };
        for s in &mut slots {
            unsafe {
                init(s);
            }
        }
        Self(slots)
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Contact<'_>> {
        self.0.iter().map(|s| Contact(Object::from_cell(&s.0)))
    }
}

impl Drop for ContactPool {
    fn drop(&mut self) {
        let destroy: unsafe extern "C" fn(*mut Storage) -> u32 =
            unsafe { core::mem::transmute(entry(0x1688c8)) };
        for slot in &mut self.0 {
            unsafe {
                destroy(slot);
            }
        }
    }
}
