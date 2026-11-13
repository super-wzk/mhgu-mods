//! Borrowed main attack rows (HDE) and their shape groups (HTS).
use crate::{
    combat::collision::{ShapeGroup, ShapeGroups},
    memory::{Field, Object},
    monster::Monster,
};

const RESOURCE_HEADER: Field<u32> = Field::at(100);
const RESOURCE_ENTRIES: Field<u32> = Field::at(104);
const ENTRY_COUNT: Field<u32> = Field::at(8);
const ROW_GROUP: Field<u8> = Field::at(25);

#[derive(Clone, Copy)]
pub struct AttackTables<'a> {
    owner: Monster<'a>,
    rows: u32,
    count: u32,
    shapes: Object<'a>,
}

impl<'a> Monster<'a> {
    /// Loaded main HDE attack table and its HTS shape resource (not the secondary banks).
    pub fn attack_tables(self) -> Option<AttackTables<'a>> {
        unsafe {
            let hde = Object::at(self.0.get(Field::at(30164)))?;
            let shapes = Object::at(self.0.get(Field::at(30172)))?;
            let header = Object::at(hde.get(RESOURCE_HEADER))?;
            let rows = Object::at(hde.get(RESOURCE_ENTRIES))?.addr();
            let count = header.get(ENTRY_COUNT);
            Some(AttackTables {
                owner: self,
                rows,
                count,
                shapes,
            })
        }
    }
}

impl<'a> AttackTables<'a> {
    pub fn row_count(self) -> u32 {
        self.count
    }
    pub fn row(self, index: u32) -> Option<AttackRow<'a>> {
        if index >= self.count {
            return None;
        }
        let addr = self.rows.checked_add(index.checked_mul(32)?)?;
        Some(AttackRow {
            data: unsafe { Object::at(addr) }?,
            owner: self.owner,
            shapes: self.shapes,
        })
    }
    /// Borrow the loaded HTS group table for the current collision pass.
    pub fn shape_groups(self) -> Option<ShapeGroups<'a>> {
        ShapeGroups::read(self.shapes)
    }
    pub fn shape_group(self, index: u8) -> Option<ShapeGroup<'a>> {
        self.shape_groups()?.get(index)
    }
}

#[derive(Clone, Copy)]
pub struct AttackRow<'a> {
    data: Object<'a>,
    pub(super) owner: Monster<'a>,
    shapes: Object<'a>,
}
impl<'a> AttackRow<'a> {
    /// Address of the 32-byte row, for plugins storing their own metadata in HDE.
    pub fn as_raw(self) -> u32 {
        self.data.addr()
    }
    pub fn start(self) -> u16 {
        self.data.get(Field::at(0))
    }
    pub fn duration(self) -> u16 {
        self.data.get(Field::at(2))
    }
    pub fn shape_group(self) -> Option<ShapeGroup<'a>> {
        ShapeGroups::read(self.shapes)?.get(self.data.get(ROW_GROUP))
    }
    /// Look up the row in an already borrowed HTS table; reject a different resource.
    pub fn shape_group_in(self, groups: ShapeGroups<'a>) -> Option<ShapeGroup<'a>> {
        if !groups.belongs_to(self.shapes) {
            return None;
        }
        groups.get(self.data.get(ROW_GROUP))
    }
}
