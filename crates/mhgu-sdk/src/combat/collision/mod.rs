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
pub use shapes::{ShapeGroup, ShapeGroups};

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

    /// Submit a sphere at a fixed world position instead of expanding a bone-relative shape.
    ///
    /// # Safety
    /// Call in the native shape expansion pass for this contact.
    pub unsafe fn submit_world_sphere(self, center: [f32; 3], radius: f32) -> bool {
        unsafe { self.submit_world_shape(1, center, [0.; 3], radius) }
    }

    /// Submit a capsule whose two endpoints are fixed in world space.
    ///
    /// # Safety
    /// Call in the native shape expansion pass for this contact.
    pub unsafe fn submit_world_capsule(self, ends: [[f32; 3]; 2], radius: f32) -> bool {
        unsafe { self.submit_world_shape(2, ends[0], ends[1], radius) }
    }

    unsafe fn submit_world_shape(
        self,
        kind: u32,
        start: [f32; 3],
        end: [f32; 3],
        radius: f32,
    ) -> bool {
        unsafe {
            let Some(manager) = singleton(0x1857830) else {
                return false;
            };
            let mut shape = [0f32; 9];
            shape[..3].copy_from_slice(&start);
            shape[4..7].copy_from_slice(&end);
            shape[8] = radius;
            self.0
                .set(Field::at(28), manager.get(Field::<u32>::at(10288)));
            let submit: unsafe extern "C" fn(u32, u32, *const f32) -> u32 =
                core::mem::transmute(entry(0x1706a8));
            submit(manager.addr(), kind, shape.as_ptr());
            true
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
