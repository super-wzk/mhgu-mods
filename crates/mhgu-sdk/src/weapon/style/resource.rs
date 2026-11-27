//! Retained GU resources whose preparation can run on the native loader.
use crate::{
    memory::{Field, entry, singleton},
    sync::Shared,
};
use core::ffi::{CStr, c_char};
use core::sync::atomic::{AtomicU32, Ordering};

const SYNC: u32 = 1;
const ASYNC: u32 = 2;
const READY: u32 = 1;
const FAILED_OR_CANCELED: u32 = 0x50;

#[repr(C)]
pub(super) struct CachedResource {
    kind: u32,
    path: &'static CStr,
    pointer: Shared<u32>,
}

impl CachedResource {
    pub(super) const fn new(kind: u32, path: &'static CStr) -> Self {
        Self {
            kind,
            path,
            pointer: Shared::new(0),
        }
    }

    /// Queue a load or retry a failed request at the next hunt's preparation.
    pub(super) unsafe fn preload(&self) {
        let failed = self.pointer.with(|pointer| {
            if unsafe { flags(*pointer) } & FAILED_OR_CANCELED != 0 {
                core::mem::take(pointer)
            } else {
                0
            }
        });
        if failed != 0 {
            unsafe { release(failed) };
        }
        unsafe { self.request(ASYNC) };
    }

    /// Keep lazy loading for callers that had no hunt preparation. A queued
    /// resource is safe to consume only once the native worker marks it ready.
    pub(super) unsafe fn get(&self) -> Option<u32> {
        unsafe { self.request(SYNC) }
    }

    /// The HUD can poll continuously while the native texture loads.
    pub(super) unsafe fn get_async(&self) -> Option<u32> {
        unsafe { self.request(ASYNC) }
    }

    unsafe fn request(&self, mode: u32) -> Option<u32> {
        if let Some(cached) = self
            .pointer
            .with(|pointer| (*pointer != 0).then(|| unsafe { ready(*pointer) }))
        {
            return cached;
        }
        let manager = unsafe { singleton(0x2126a64) }?;
        let vtable: *const usize = manager.get(Field::at(0));
        let load: unsafe extern "C" fn(u32, usize, *const c_char, u32) -> u32 =
            unsafe { core::mem::transmute(vtable.add(12).read()) };
        // BCAD18 dispatches flag 2 to BCB6BC (native Resource Loader queue).
        // Even a synchronous cache lookup can return a pending resource.
        let resource = unsafe { load(manager.addr(), entry(self.kind), self.path.as_ptr(), mode) };
        if resource == 0 {
            return None;
        }
        let (kept, ready) = self.pointer.with(|pointer| {
            let kept = *pointer == 0;
            if kept {
                *pointer = resource;
            }
            (kept, unsafe { ready(*pointer) })
        });
        if !kept {
            unsafe { release(resource) };
        }
        ready
    }
}

unsafe fn flags(resource: u32) -> u32 {
    if resource == 0 {
        return 0;
    }
    // BC74E0 publishes this aligned status word after parse and finalization.
    unsafe { AtomicU32::from_ptr((resource + 80) as *mut u32) }.load(Ordering::Acquire)
}

unsafe fn ready(resource: u32) -> Option<u32> {
    (unsafe { flags(resource) } & (READY | FAILED_OR_CANCELED) == READY).then_some(resource)
}

unsafe fn release(resource: u32) {
    let release: unsafe extern "C" fn(u32) = unsafe { core::mem::transmute(entry(0x887660)) };
    unsafe { release(resource) };
}
