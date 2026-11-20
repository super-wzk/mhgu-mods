use alloc::boxed::Box;
use core::{
    ffi::{CStr, c_char},
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
};
use mhfz_sdk::trail::{self, Renderer, Sample, Snapshot};
use mhgu_sdk::style::particle::{self, OnRelease};

// Entries are registered during Forge init and stay valid on the render thread.
struct Strip {
    path: &'static CStr,
    snapshot: Snapshot,
    renderer: Renderer,
    next: *mut Strip,
}
static STRIPS: AtomicPtr<Strip> = AtomicPtr::new(ptr::null_mut());
static SERVICE: trail::Service = trail::Service(bind);
const MODEL_PATH_OFFSET: u32 = 80;

unsafe extern "C" fn bind(path: *const c_char, snapshot: Snapshot) {
    if path.is_null() {
        return;
    }
    let path = unsafe { CStr::from_ptr(path) };
    if path.to_bytes_with_nul().len() > 64 {
        return;
    }
    let next = STRIPS.load(Ordering::Acquire);
    STRIPS.store(
        Box::into_raw(Box::new(Strip {
            path,
            snapshot,
            renderer: Renderer::new(),
            next,
        })),
        Ordering::Release,
    );
}

unsafe fn render_strip(generator: u32, params: u32, draw: impl FnOnce() -> u32) -> u32 {
    let model_path = (params + MODEL_PATH_OFFSET) as *const u8;
    let mut cur = STRIPS.load(Ordering::Acquire);
    while let Some(strip) = unsafe { cur.as_ref() } {
        let path = strip.path.to_bytes_with_nul();
        if unsafe { core::slice::from_raw_parts(model_path, path.len()) } == path {
            let mut sample = Sample::default();
            let version = unsafe { (strip.snapshot)(&mut sample) };
            return if version == 0 {
                0
            } else {
                unsafe { strip.renderer.draw(generator, version, &sample, draw) }
            };
        }
        cur = strip.next;
    }
    draw()
}

unsafe fn read<T: Copy>(address: u32, offset: u32) -> T {
    unsafe { ((address + offset) as *const T).read_unaligned() }
}

#[forge::hook(offset = 0xa956d8)]
unsafe fn draw_model(generator: u32, context: u32, gain: u32) -> u32 {
    const STATE_STACK_DEPTH_OFFSET: u32 = 0x308e4;
    const STATE_STACK_CAPACITY: u32 = 5;
    const FULL_STATE: u32 = 1;

    unsafe {
        if read::<u8>(generator, 70) != 5 {
            return original!(generator, context, gain);
        }
        let params: u32 = read(generator, 52);
        if params == 0
            || !matches!(
                &read::<[u8; 19]>(params, MODEL_PATH_OFFSET),
                b"effect\\frontier_gs\\" | b"effect\\frontier_db\\" | b"effect\\frontier_ls\\"
            )
        {
            return original!(generator, context, gain);
        }

        let depth = read::<u32>(context, STATE_STACK_DEPTH_OFFSET);
        if depth > STATE_STACK_CAPACITY {
            return 0;
        }
        let draw = || render_strip(generator, params, || original!(generator, context, gain));
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
        .store(released as u32, Ordering::Release);
    true
}

#[forge::hook(offset = 0x32a104)]
unsafe fn release_unit(unit: u32) -> u32 {
    for entry in crate::runtime::registered_entries() {
        let callback = entry.particle_release.load(Ordering::Acquire);
        if callback != 0 {
            let released: OnRelease = unsafe { core::mem::transmute(callback) };
            released(unit);
        }
    }
    original!(unit)
}

pub(super) fn install(base: u32) {
    unsafe {
        trail::publish(&SERVICE);
    }
    if unsafe { particle::publish_release(&RELEASE_SERVICE) } {
        forge::install_hook!(base, release_unit);
    }
    forge::install_hook!(base, draw_model);
}
