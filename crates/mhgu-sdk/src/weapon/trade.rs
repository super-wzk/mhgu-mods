//! Prices and recipes for the GU weapon shop and smithy.

/// One native GU item ID and its required quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Material {
    pub item: u16,
    pub quantity: u16,
}

impl Material {
    pub const EMPTY: Self = Self {
        item: 0,
        quantity: 0,
    };
}

/// A zenny price and up to four distinct materials. Fill unused slots with
/// `Material::EMPTY`. GU takes materials from the pouch first, then the item box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Recipe {
    pub zenny: u32,
    pub materials: [Material; 4],
}

#[cfg(all(target_arch = "arm", target_endian = "little"))]
pub use native::*;

#[cfg(all(target_arch = "arm", target_endian = "little"))]
mod native {
    use super::Recipe;
    use crate::{
        service,
        weapon::{EquipmentId, style::RegisterError},
    };
    use core::ffi::CStr;

    const NAME: &CStr = c"mhgu::WeaponTradeV2";
    const MAGIC: u32 = u32::from_le_bytes(*b"WTR2");

    #[doc(hidden)]
    #[repr(C)]
    pub struct Service {
        pub purchase: extern "C" fn(u16, u32) -> u32,
        pub forge: extern "C" fn(u16, Recipe) -> u32,
        pub upgrade: extern "C" fn(u16, u16, Recipe) -> u32,
    }

    /// # Safety
    /// Install on the GU initialization thread before weapon registration.
    #[doc(hidden)]
    pub unsafe fn publish(api: &'static Service) -> bool {
        unsafe { service::publish(NAME, MAGIC, api) }
    }

    fn register(call: impl FnOnce(&Service) -> u32) -> Result<(), RegisterError> {
        if !service::on_game_thread() {
            return Err(RegisterError::Invalid);
        }
        let api = unsafe { service::find(NAME, MAGIC) }.ok_or(RegisterError::Invalid)?;
        match call(api) {
            0 => Ok(()),
            2 => Err(RegisterError::Conflict),
            _ => Err(RegisterError::Invalid),
        }
    }

    /// Sell a new level-zero instance at the weapon shop. Register its weapon first.
    pub fn register_purchase(equipment: EquipmentId, zenny: u32) -> Result<(), RegisterError> {
        register(|api| (api.purchase)(equipment.get(), zenny))
    }

    /// Make a new level-zero instance at the smithy.
    pub fn register_recipe(equipment: EquipmentId, recipe: Recipe) -> Result<(), RegisterError> {
        register(|api| (api.forge)(equipment.get(), recipe))
    }

    /// Improve an instance from `level` to `level + 1` at the smithy.
    pub fn register_upgrade(
        equipment: EquipmentId,
        level: u16,
        recipe: Recipe,
    ) -> Result<(), RegisterError> {
        register(|api| (api.upgrade)(equipment.get(), level, recipe))
    }
}
