//! Native particle drawing and release dispatch.
use alloc::boxed::Box;
use core::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};
use mhgu_sdk::weapon::style::particle::{self, DrawModel, ModelRenderer, OnRelease};

struct Renderer {
    api: &'static ModelRenderer,
    next: *mut Renderer,
}
static RENDERERS: AtomicPtr<Renderer> = AtomicPtr::new(ptr::null_mut());
static mut ORIGINAL_DRAW: *const c_void = ptr::null();

unsafe extern "C" fn bind_model(api: *const ModelRenderer) {
    let Some(api) = (unsafe { api.as_ref() }) else {
        return;
    };
    // Forge serializes registrations. Render callbacks only borrow the
    // published nodes, which stay resident until the process exits.
    let next = RENDERERS.load(Ordering::Acquire);
    let renderer = Box::new(Renderer { api, next });
    RENDERERS.store(Box::into_raw(renderer), Ordering::Release);
}

unsafe fn renderer(params: u32) -> Option<&'static ModelRenderer> {
    let mut node = RENDERERS.load(Ordering::Acquire);
    while let Some(renderer) = unsafe { node.as_ref() } {
        if unsafe { (renderer.api.matches)(params) } {
            return Some(renderer.api);
        }
        node = renderer.next;
    }
    None
}

unsafe fn read<T: Copy>(address: u32, offset: u32) -> T {
    unsafe { ((address + offset) as *const T).read_unaligned() }
}

unsafe extern "C" fn draw_model(generator: u32, context: u32, gain: u32) -> u32 {
    const STATE_STACK_DEPTH_OFFSET: u32 = 0x308e4;
    const STATE_STACK_CAPACITY: u32 = 5;
    const FULL_STATE: u32 = 1;

    unsafe {
        let native: DrawModel = core::mem::transmute(ORIGINAL_DRAW);
        if read::<u8>(generator, 70) != 5 {
            return native(generator, context, gain);
        }
        let params: u32 = read(generator, 52);
        let Some(renderer) = renderer(params) else {
            return native(generator, context, gain);
        };

        let depth = read::<u32>(context, STATE_STACK_DEPTH_OFFSET);
        if depth > STATE_STACK_CAPACITY {
            return 0;
        }
        let draw = || (renderer.draw)(native, generator, params, context, gain);
        if depth == STATE_STACK_CAPACITY {
            // GU 87A7AC/87B778 copy the live bindings at +0xAC. Mirror
            // their full-state copy off-stack, leaving every outer slot
            // and the command-list cursors owned by the caller.
            const LIVE_BINDINGS_OFFSET: u32 = 0xac;
            const FIXED_STATE_BYTES: usize = 344;
            const BINDING_BYTES: usize = 8;
            const MAX_BINDINGS: u32 = 4096;
            let binding_count = read::<u32>(context, 4);
            if binding_count > MAX_BINDINGS {
                return 0;
            }
            let length = FIXED_STATE_BYTES + BINDING_BYTES * binding_count as usize;
            let live = (context + LIVE_BINDINGS_OFFSET) as *mut core::mem::MaybeUninit<u8>;
            let mut snapshot = alloc::vec::Vec::<u8>::new();
            if snapshot.try_reserve_exact(length).is_err() {
                return 0;
            }
            let saved = snapshot.spare_capacity_mut().as_mut_ptr();
            core::ptr::copy_nonoverlapping(live, saved, length);
            let result = draw();
            core::ptr::copy_nonoverlapping(saved, live, length);
            return result;
        }

        // Preserve the bindings needed by later passes, including the stage
        // fog table. GU keeps the particle's draw packet in the command list.
        let base = forge::mem::text_addr();
        let save_state: unsafe extern "C" fn(u32, u32) -> u32 =
            core::mem::transmute(base + 0x87a7ac);
        let restore_state: unsafe extern "C" fn(u32, u32) = core::mem::transmute(base + 0x87b778);
        save_state(context, FULL_STATE);
        let result = draw();
        restore_state(context, FULL_STATE);
        result
    }
}

static RELEASE_SERVICE: particle::ReleaseService = particle::ReleaseService(bind_release);

extern "C" fn bind_release(id: u32, released: OnRelease) -> bool {
    let Some(entry) = super::registered_entry(id) else {
        return false;
    };
    entry
        .particle_release
        .store(released as *mut (), Ordering::Release);
    true
}

#[forge::hook(offset = 0x32a104)]
unsafe fn release_unit(unit: u32) -> u32 {
    for entry in crate::weapon::style::registered_entries() {
        let callback = entry.particle_release.load(Ordering::Acquire);
        if !callback.is_null() {
            let released: OnRelease = unsafe { core::mem::transmute(callback) };
            released(unit);
        }
    }
    original!(unit)
}

pub(super) fn install(base: u32) {
    static MODELS: particle::ModelService = particle::ModelService(bind_model);
    if unsafe { particle::publish_model(&MODELS) } {
        unsafe {
            forge::sys::hook::forge_hook_create(
                (base + 0xa956d8) as *const _,
                draw_model as *const _,
                &raw mut ORIGINAL_DRAW,
            );
        }
    }
    if unsafe { particle::publish_release(&RELEASE_SERVICE) } {
        forge::install_hook!(base, release_unit);
    }
}
