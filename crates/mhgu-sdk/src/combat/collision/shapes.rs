//! Groups in a live HTS resource, shared by contacts of any owner.
use crate::memory::{Field, Object, entry, singleton};
use core::ffi::{CStr, c_char};

/// Owns one native reference to a loaded rHitShape. GU retains its own reference
/// when the resource is submitted with an attack.
pub struct HitShape(Object<'static>);

impl HitShape {
    /// # Safety
    /// Call on the game thread while the resource manager is active.
    pub unsafe fn load(path: &CStr) -> Option<Self> {
        unsafe {
            let manager = singleton(0x2126a64)?;
            let vtable = Object::at(manager.get(Field::at(0)))?;
            let load: unsafe extern "C" fn(u32, u32, *const c_char, u32) -> u32 =
                core::mem::transmute(vtable.get::<u32>(Field::at(48)));
            let shape = Self(Object::at(load(
                manager.addr(),
                entry(0x1857810) as u32,
                path.as_ptr(),
                1,
            ))?);
            (shape.0.get::<u32>(Field::at(80)) & 1 != 0).then_some(shape)
        }
    }

    pub fn as_raw(&self) -> u32 {
        self.0.addr()
    }

    /// Return the first group’s native row pointers if its size matches.
    /// The caller can borrow this address while the resource is retained.
    pub fn rows_in_first_group(&self, count: u32) -> Option<u32> {
        unsafe {
            let vtable = Object::at(self.0.get(Field::at(0)))?;
            let get: unsafe extern "C" fn(u32, u32) -> u32 =
                core::mem::transmute(vtable.get::<u32>(Field::at(68)));
            let group = Object::at(get(self.as_raw(), 0))?;
            let rows = group.get(Field::at(8));
            (group.get::<u32>(Field::at(4)) == count && rows != 0).then_some(rows)
        }
    }
}

impl Drop for HitShape {
    fn drop(&mut self) {
        unsafe {
            let release: unsafe extern "C" fn(u32) = core::mem::transmute(entry(0x887660));
            release(self.as_raw());
        }
    }
}

/// A group resolved against one live HTS resource; two equal indices may refer to different groups.
#[derive(Clone, Copy)]
pub struct ShapeGroup<'a> {
    pub(crate) resource: Object<'a>,
    pub(crate) index: u8,
}

#[derive(Clone, Copy)]
pub struct ShapeGroups<'a> {
    resource: Object<'a>,
    slots: Object<'a>,
    count: u32,
}
impl<'a> ShapeGroups<'a> {
    pub(crate) fn read(resource: Object<'a>) -> Option<Self> {
        let header = unsafe { Object::at(resource.get(Field::at(100))) }?;
        Some(ShapeGroups {
            resource,
            slots: unsafe { Object::at(resource.get(Field::at(104))) }?,
            count: header.get(Field::at(8)),
        })
    }

    /// GU's 16A4D8 / 16A4E4 accept loaded groups (even empty ones), but never null slots.
    pub fn get(self, index: u8) -> Option<ShapeGroup<'a>> {
        if u32::from(index) >= self.count
            || self.slots.get(Field::<u32>::at(4 * usize::from(index))) == 0
        {
            return None;
        }
        Some(ShapeGroup {
            resource: self.resource,
            index,
        })
    }

    pub(crate) fn belongs_to(self, resource: Object) -> bool {
        self.resource.addr() == resource.addr()
    }
}
