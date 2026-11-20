//! Versioned services published in the process-wide MT type table.
use crate::memory::entry;
use core::ffi::{CStr, c_char};

#[repr(C)]
struct Published<T: 'static> {
    dti: [u32; 8], // GU 7AEDCC initializes all eight fields in caller-owned storage.
    magic: u32,
    api: &'static T,
}

fn descriptor(name: &CStr) -> *const u32 {
    unsafe {
        let crc: unsafe extern "C" fn(*const c_char, i32) -> u32 =
            core::mem::transmute(entry(0x7cbdb4));
        let find: unsafe extern "C" fn(u32) -> *const u32 = core::mem::transmute(entry(0x7aef68));
        find(crc(name.as_ptr(), -1) & 0x7fff_ffff)
    }
}

/// Provider only. The published DTI is never unloaded.
///
/// # Safety
/// Call on the serialized Forge init thread. The API and its NRO must stay
/// loaded for the process lifetime.
#[doc(hidden)]
pub unsafe fn publish<T>(name: &'static CStr, magic: u32, api: &'static T) -> bool {
    use alloc::boxed::Box;
    if !descriptor(name).is_null() {
        return false;
    }
    let storage = Box::leak(Box::new(Published {
        dti: [0; 8],
        magic,
        api,
    }));
    let init: unsafe extern "C" fn(*mut u32, *const c_char, u32, u32, u32, u32, u32) -> *mut u32 =
        unsafe { core::mem::transmute(entry(0x7aedcc)) };
    unsafe { init(storage.dti.as_mut_ptr(), name.as_ptr(), 0, 0, 0, 0, 0) };
    true
}

/// # Safety
/// The name and magic must identify the published API type `T`. Call only
/// on the serialized game thread while the engine's type table is live.
#[doc(hidden)]
pub unsafe fn find<T>(name: &CStr, magic: u32) -> Option<&'static T> {
    let ptr = descriptor(name);
    if ptr.is_null() {
        return None;
    }
    unsafe {
        // A CRC collision with a different native type is not our descriptor.
        if CStr::from_ptr(ptr.add(1).read() as *const c_char) != name {
            return None;
        }
        let p = ptr.cast::<Published<T>>();
        if (*p).magic != magic {
            return None;
        }
        Some((*p).api)
    }
}

/// MT service lookup is only performed on the initialized GU startup thread.
#[doc(hidden)]
pub fn on_game_thread() -> bool {
    // GU 1.4: the main module's initializer 7AF404 constructs the process-lifetime
    // default MT allocator at 18A6190. Its constructor 7AAF00 records the startup
    // thread at +56, using this same nn::os::GetCurrentThread import.
    const STARTUP_THREAD: u32 = 0x18a6190 + 56;
    unsafe {
        let owner = (entry(STARTUP_THREAD) as *const u32).read();
        if owner == 0 {
            return false;
        }
        let current: unsafe extern "C" fn() -> u32 = core::mem::transmute(entry(0x13effd4));
        current() == owner
    }
}
