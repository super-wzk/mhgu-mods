//! Register custom weapon identities and their controllers on GU's game thread.
use super::{
    EquipmentId, EquipmentLevel, EquipmentModel, EquippedWeapon, Weapon, WeaponEquipment, WeaponId,
    WeaponInstance, WeaponKind,
    style::{self, RegisterError, Style},
};
use crate::service;
use core::ffi::{CStr, c_char};

const NAME: &CStr = c"mhgu::WeaponRegistryV7";
const MAGIC: u32 = u32::from_le_bytes(*b"WPN7");

#[doc(hidden)]
#[repr(C)]
pub struct Service {
    pub register: unsafe extern "C" fn(
        *const Weapon,
        *const Style,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> u32,
    pub equipped: extern "C" fn() -> u32,
    pub grant: extern "C" fn(u16) -> u32,
    pub upgrade: extern "C" fn(u32) -> bool,
    pub instance: unsafe extern "C" fn(u32, *mut WeaponInstance) -> bool,
    pub register_equipment: unsafe extern "C" fn(
        *const WeaponEquipment,
        *const EquipmentLevel,
        u32,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
    ) -> u32,
    pub base: extern "C" fn(u16) -> *const WeaponEquipment,
    pub level: extern "C" fn(u16, u16) -> *const EquipmentLevel,
    pub model_path: extern "C" fn(u16, bool) -> *const c_char,
    pub register_icon: unsafe extern "C" fn(u8, *const c_char) -> u32,
}

/// # Safety
/// Install on GU's serialized initialization thread before style startup completes.
#[doc(hidden)]
pub unsafe fn publish(api: &'static Service) -> bool {
    unsafe { service::publish(NAME, MAGIC, api) }
}

/// Defer initialization until the runtime has installed its weapon and style services.
pub fn when_ready(init: extern "C" fn()) {
    style::registry::when_ready(init);
}

/// Fetch the weapon registry on the GU game thread.
pub struct WeaponRegistry(&'static Service, core::marker::PhantomData<*const ()>);

impl WeaponRegistry {
    pub fn current() -> Option<Self> {
        if !service::on_game_thread() {
            return None;
        }
        Some(Self(
            unsafe { service::find(NAME, MAGIC) }?,
            core::marker::PhantomData,
        ))
    }

    /// Registers the custom weapon and its style together. Give the style the
    /// weapon ID’s style_key() and a globally unique style ID.
    pub fn register(
        &self,
        weapon: &'static Weapon,
        style: &'static Style,
        weapon_name: &'static CStr,
        style_name: &'static CStr,
        details: &'static CStr,
    ) -> Result<(), RegisterError> {
        match unsafe {
            (self.0.register)(
                weapon,
                style,
                weapon_name.as_ptr(),
                style_name.as_ptr(),
                details.as_ptr(),
            )
        } {
            0 => Ok(()),
            2 => Err(RegisterError::Conflict),
            _ => Err(RegisterError::Invalid),
        }
    }

    /// Register one catalogue entry. The first level describes a new instance,
    /// and the remaining rows are its successive upgrades.
    pub fn register_equipment(
        &self,
        equipment: &'static WeaponEquipment,
        levels: &'static [EquipmentLevel],
        name: &'static CStr,
        details: &'static CStr,
        model: Option<EquipmentModel>,
    ) -> Result<(), RegisterError> {
        match unsafe {
            (self.0.register_equipment)(
                equipment,
                levels.as_ptr(),
                levels.len() as u32,
                name.as_ptr(),
                details.as_ptr(),
                model.map_or(core::ptr::null(), |m| m.archive.as_ptr()),
                model.map_or(core::ptr::null(), |m| m.model.as_ptr()),
            )
        } {
            0 => Ok(()),
            2 => Err(RegisterError::Conflict),
            _ => Err(RegisterError::Invalid),
        }
    }

    /// Assign a private 64×64 GU TEX to an SDK icon slot (128–255).
    /// Use its extensionless nativeNX path and reference the slot from
    /// `Weapon.icon`.
    pub fn register_icon(&self, slot: u8, texture: &'static CStr) -> Result<(), RegisterError> {
        match unsafe { (self.0.register_icon)(slot, texture.as_ptr()) } {
            0 => Ok(()),
            2 => Err(RegisterError::Conflict),
            _ => Err(RegisterError::Invalid),
        }
    }

    /// Read the registered base metadata or the stats of a particular upgrade.
    pub fn equipment(&self, id: EquipmentId) -> Option<&'static WeaponEquipment> {
        unsafe { (self.0.base)(id.get()).as_ref() }
    }
    pub fn level(&self, id: EquipmentId, upgrade: u16) -> Option<&'static EquipmentLevel> {
        unsafe { (self.0.level)(id.get(), upgrade).as_ref() }
    }

    /// Registered 3D resources for this equipment, if supplied by its plugin.
    pub fn model(&self, equipment: EquipmentId) -> Option<EquipmentModel> {
        let archive = (self.0.model_path)(equipment.get(), true);
        let model = (self.0.model_path)(equipment.get(), false);
        if archive.is_null() || model.is_null() {
            return None;
        }
        Some(EquipmentModel {
            archive: unsafe { CStr::from_ptr(archive) },
            model: unsafe { CStr::from_ptr(model) },
        })
    }

    /// Grant a new level-zero instance to the loaded character.
    pub fn grant(&self, equipment: EquipmentId) -> Option<u32> {
        let id = (self.0.grant)(equipment.get());
        (id != 0).then_some(id)
    }

    /// Increase an owned instance by one level, up to its definition's limit.
    pub fn upgrade(&self, id: u32) -> bool {
        (self.0.upgrade)(id)
    }

    pub fn instance(&self, id: u32) -> Option<WeaponInstance> {
        let mut instance = core::mem::MaybeUninit::uninit();
        unsafe { (self.0.instance)(id, instance.as_mut_ptr()).then(|| instance.assume_init()) }
    }

    pub fn equipped_instance(&self) -> Option<WeaponInstance> {
        self.instance(0)
    }

    pub fn equipped(&self) -> Option<EquippedWeapon> {
        match (self.0.equipped)() {
            raw @ 0..=14 => Some(EquippedWeapon::Native(WeaponKind::from_gu(raw as u8)?)),
            raw @ 0x10001..=0x1ffff => Some(EquippedWeapon::Custom(WeaponId::new(raw as u16)?)),
            _ => None,
        }
    }
}
