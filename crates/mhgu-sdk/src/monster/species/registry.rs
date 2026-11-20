//! Optional registry service. GU's own lookup and records work without a provider.
use super::{HuntCounts, Species, SpeciesId, valid_key};
use crate::memory::{Field, entry, singleton};
use core::{ffi::CStr, marker::PhantomData};

// A named MT type provides a process-wide rendezvous, including on MHGURecomp,
// whose loader resolves NRO imports before other plugins may be initialized.
const NAME: &CStr = c"mhgu::MonsterRegistryV1";
const MAGIC: u32 = u32::from_le_bytes(*b"MRG1");

/// Versioned plugin ABI. No Rust strings, references or allocator ownership cross NROs.
#[doc(hidden)]
#[repr(C)]
pub struct ServiceV1 {
    pub register: unsafe extern "C" fn(u32, *const u8, usize, u32) -> u32,
    pub contains: extern "C" fn(u32) -> u32,
    pub counts: extern "C" fn(u32) -> u32,
}

/// # Safety
/// Publish once on the serialized Forge init thread with a process-lifetime API.
#[doc(hidden)]
pub unsafe fn publish(api: &'static ServiceV1) -> bool {
    unsafe { crate::service::publish(NAME, MAGIC, api) }
}

fn provider() -> Option<&'static ServiceV1> {
    unsafe { crate::service::find(NAME, MAGIC) }
}

/// Access on the initialized game thread. Do not send across threads.
pub struct MonsterRegistry(PhantomData<*const ()>);
impl MonsterRegistry {
    /// Obtain access only on GU's startup thread; returns None before MT initialization
    /// or from another thread. The handle cannot be sent to a worker thread.
    pub fn current() -> Option<Self> {
        crate::service::on_game_thread().then_some(Self(PhantomData))
    }

    pub fn resolve(&self, id: SpeciesId) -> Option<Species> {
        if id.is_native() {
            return Some(Species::Native(id));
        }
        ((provider()?.contains)(id.packed().into()) == 1).then_some(Species::Added(id))
    }

    /// Register an independent quest identity. The native template supplies the GU actor
    /// class and fixed layout tables, never the new species’ hunt records.
    pub fn register(
        &self,
        key: &str,
        packed: u16,
        template: SpeciesId,
    ) -> Result<Species, RegisterError> {
        let id = SpeciesId::new(packed).ok_or(RegisterError::InvalidId)?;
        if !valid_key(key) {
            return Err(RegisterError::InvalidKey);
        }
        if id.is_native() {
            return Err(RegisterError::NativeId);
        }
        if !template.is_native() || (template.packed() ^ packed) & 0x1000 != 0 {
            return Err(RegisterError::InvalidTemplate);
        }
        let api = provider().ok_or(RegisterError::Unavailable)?;
        let status = unsafe {
            (api.register)(
                id.packed().into(),
                key.as_ptr(),
                key.len(),
                template.packed().into(),
            )
        };
        match status {
            0 => Ok(Species::Added(id)),
            1 => Err(RegisterError::NativeId),
            2 => Err(RegisterError::Conflict),
            4 => Err(RegisterError::InvalidTemplate),
            _ => Err(RegisterError::InvalidKey),
        }
    }

    /// A replacement still resolves to the original GU species and player record.
    pub fn counts(&self, species: Species) -> Option<HuntCounts> {
        match species {
            Species::Native(id) => {
                let i = id.native_index()?;
                let user = unsafe { singleton(0x1897f78) }?;
                Some(HuntCounts {
                    slain: user.get(Field::at(1836 + 2 * i)),
                    captured: user.get(Field::at(2110 + 2 * i)),
                })
            }
            Species::Added(id) => {
                let value = (provider()?.counts)(id.packed().into());
                (value != u32::MAX).then_some(HuntCounts {
                    slain: value as u16,
                    captured: (value >> 16) as u16,
                })
            }
        }
    }
}

impl SpeciesId {
    /// Whether this packed quest ID appears in GU's built-in species table.
    pub fn is_native(self) -> bool {
        self.native_index().is_some()
    }

    fn native_index(self) -> Option<usize> {
        // The loader may route an added identity through a native layout. Only
        // the immutable retail table defines which IDs own native record rows.
        let rows = unsafe { &*(entry(0x1597ea4) as *const [[u32; 2]; 137]) };
        rows.iter()
            .find(|r| r[0] == u32::from(self.0))
            .map(|r| r[1] as usize)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterError {
    Unavailable,
    NativeId,
    Conflict,
    InvalidKey,
    InvalidId,
    InvalidTemplate,
}
