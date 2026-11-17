//! Optional registry service. GU's own lookup and records work without a provider.
use super::{HuntCounts, Species, SpeciesId, valid_key};
use crate::memory::{Field, entry, singleton};
use core::{
    ffi::{CStr, c_char},
    marker::PhantomData,
};

// A named MT type provides a process-wide rendezvous, including on MHGURecomp,
// whose loader resolves NRO imports before other plugins may be initialized.
const NAME: &CStr = c"mhgu::MonsterRegistryV2";
const MAGIC: u32 = u32::from_le_bytes(*b"MRG2");

/// Versioned plugin ABI. No Rust strings, references or allocator ownership cross NROs.
#[doc(hidden)]
#[repr(C)]
pub struct ServiceV2 {
    pub register: unsafe extern "C" fn(u32, *const u8, usize, u32) -> u32,
    pub contains: extern "C" fn(u32) -> u32,
    pub counts: extern "C" fn(u32) -> u32,
}

#[repr(C)]
struct Published {
    dti: [u32; 8], // GU 7AEDCC initializes all eight fields in caller-owned storage.
    magic: u32,
    api: &'static ServiceV2,
}

fn descriptor() -> *const u32 {
    unsafe {
        let crc: unsafe extern "C" fn(*const c_char, i32) -> u32 =
            core::mem::transmute(entry(0x7cbdb4));
        let find: unsafe extern "C" fn(u32) -> *const u32 = core::mem::transmute(entry(0x7aef68));
        find(crc(NAME.as_ptr(), -1) & 0x7fff_ffff)
    }
}

/// Provider only. The published DTI is never unloaded.
///
/// # Safety
/// Call on the serialized Forge init thread. The API and its NRO must stay
/// loaded for the process lifetime.
#[doc(hidden)]
pub unsafe fn publish(api: &'static ServiceV2) -> bool {
    use alloc::boxed::Box;
    if !descriptor().is_null() {
        return false;
    }
    let storage = Box::leak(Box::new(Published {
        dti: [0; 8],
        magic: MAGIC,
        api,
    }));
    let init: unsafe extern "C" fn(*mut u32, *const c_char, u32, u32, u32, u32, u32) -> *mut u32 =
        unsafe { core::mem::transmute(entry(0x7aedcc)) };
    unsafe { init(storage.dti.as_mut_ptr(), NAME.as_ptr(), 0, 0, 0, 0, 0) };
    true
}

fn provider() -> Option<&'static ServiceV2> {
    let ptr = descriptor();
    if ptr.is_null() {
        return None;
    }
    unsafe {
        // A CRC collision with a different native type is not our descriptor.
        if CStr::from_ptr(ptr.add(1).read() as *const c_char) != NAME {
            return None;
        }
        let p = ptr.cast::<Published>();
        if (*p).magic != MAGIC {
            return None;
        }
        Some((*p).api)
    }
}

/// Access on the initialized game thread. Do not send across threads.
pub struct MonsterRegistry(PhantomData<*const ()>);
impl MonsterRegistry {
    /// Obtain access only on GU's startup thread; returns None before MT initialization
    /// or from another thread. The handle cannot be sent to a worker thread.
    pub fn current() -> Option<Self> {
        // GU 1.4: the main module's initializer 7AF404 constructs the process-lifetime
        // default MT allocator at 18A6190. Its constructor 7AAF00 records the startup
        // thread at +56, using this same nn::os::GetCurrentThread import.
        const STARTUP_THREAD: u32 = 0x18a6190 + 56;
        unsafe {
            let owner = (entry(STARTUP_THREAD) as *const u32).read();
            if owner == 0 {
                return None;
            }
            let current: unsafe extern "C" fn() -> u32 = core::mem::transmute(entry(0x13effd4));
            (current() == owner).then_some(Self(PhantomData))
        }
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
