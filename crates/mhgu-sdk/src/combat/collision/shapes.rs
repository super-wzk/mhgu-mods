//! Groups in a live HTS resource, shared by contacts of any owner.
use crate::memory::{Field, Object};

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
