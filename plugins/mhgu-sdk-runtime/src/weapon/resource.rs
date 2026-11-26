//! Retain resources referenced by SDK overlays.
use alloc::collections::BTreeMap;
use core::{ffi::CStr, sync::atomic::Ordering};
use mhgu_sdk::sync::Shared;

static RESOURCES: Shared<BTreeMap<(u32, &'static CStr), u32>> = Shared::new(BTreeMap::new());

unsafe fn read(addr: u32) -> u32 {
    unsafe { (addr as *const u32).read_unaligned() }
}

pub(super) fn load(kind: u32, path: &'static CStr) -> Option<u32> {
    let base = forge::mem::text_addr();
    let cached = RESOURCES.with(|r| r.get(&(kind, path)).copied());
    let ptr = match cached {
        Some(ptr) => ptr,
        None => unsafe {
            let manager = read(base + 0x2126a64);
            if manager == 0 {
                return None;
            }
            let load: unsafe extern "C" fn(u32, u32, *const core::ffi::c_char, u32) -> u32 =
                core::mem::transmute(read(read(manager) + 48));
            let ptr = load(manager, base + kind, path.as_ptr(), 1);
            if ptr == 0 {
                return None;
            }
            // Keep the resource alive while native objects can use it.
            RESOURCES.with(|r| r.insert((kind, path), ptr));
            ptr
        },
    };
    let flags = unsafe { core::sync::atomic::AtomicU32::from_ptr((ptr + 80) as *mut u32) }
        .load(Ordering::Acquire);
    (flags & 0x51 == 1).then_some(ptr)
}
