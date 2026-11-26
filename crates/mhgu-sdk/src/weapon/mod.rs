//! Native weapon identity and optional SDK weapon extensions.

pub mod style;
pub mod trade;

/// A GU hunter weapon type, suitable for indexing native weapon tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct WeaponKind(u8);

impl WeaponKind {
    pub const COUNT: usize = 15;

    pub const fn from_gu(id: u8) -> Option<Self> {
        if (id as usize) < Self::COUNT {
            Some(Self(id))
        } else {
            None
        }
    }

    pub const fn gu_id(self) -> u8 {
        self.0
    }
}

/// Stable SDK identity of a custom weapon. Zero is reserved for native weapons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct WeaponId(u16);

impl WeaponId {
    pub const fn new(id: u16) -> Option<Self> {
        if id == 0 { None } else { Some(Self(id)) }
    }

    pub const fn style_key(self) -> u32 {
        0x10000 | self.0 as u32
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// An SDK weapon class, shared by its equipment and hunting styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Weapon {
    pub id: WeaponId,
    /// Gunner equipment when true, Blademaster otherwise.
    pub ranged: bool,
    /// Icon slot shown beside this weapon type.
    pub icon: u8,
}

/// Stable ID of a registered weapon equipment definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct EquipmentId(u16);

impl EquipmentId {
    pub const fn new(id: u16) -> Option<Self> {
        if id == 0 { None } else { Some(Self(id)) }
    }
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Resources provided by this weapon. Paths use GU resource
/// names relative to nativeNX, without file extensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EquipmentModel {
    pub archive: &'static core::ffi::CStr,
    pub model: &'static core::ffi::CStr,
}

/// One registered weapon's base data, shared by all its upgrade levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct WeaponEquipment {
    pub id: EquipmentId,
    pub weapon: WeaponId,
    /// Display rarity (1-based), applied to the weapon type's icon.
    pub rarity: u8,
    /// GU mElement and, for dual-element equipment, mElement2.
    pub elements: [Element; 2],
    /// GU mSpecialSkill, mSndMaterial and mSrqrNo identifiers.
    pub special_skill: u8,
    pub sound_material: u8,
    pub sound_request: u16,
}

/// One of the two element/status channels in a GU weapon definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct Element(u8);
impl Element {
    pub const NONE: Self = Self(0);
    pub const FIRE: Self = Self(1);
    pub const WATER: Self = Self(2);
    pub const THUNDER: Self = Self(3);
    pub const ICE: Self = Self(5);
    pub const DRAGON: Self = Self(4);
    pub const POISON: Self = Self(6);
    pub const PARALYSIS: Self = Self(7);
    pub const SLEEP: Self = Self(8);
    pub const BLAST: Self = Self(9);
    pub const fn id(self) -> u8 {
        self.0
    }
}

/// GU level-row values. Index zero describes the newly forged equipment.
/// Sharpness selects a GU kireajiData row and its native gauge level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(C)]
pub struct EquipmentLevel {
    pub attack: i16,
    pub element_values: [i16; 2],
    pub defense: u8,
    pub affinity: i8,
    pub slots: u8,
    pub sharpness_type: u8,
    pub sharpness_level: u8,
}

/// A weapon instance owned by the currently loaded hunter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct WeaponInstance {
    pub id: u32,
    pub equipment: EquipmentId,
    pub upgrade: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquippedWeapon {
    Native(WeaponKind),
    Custom(WeaponId),
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub mod registry;
