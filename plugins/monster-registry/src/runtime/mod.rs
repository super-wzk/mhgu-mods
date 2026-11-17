use crate::records::Registry;
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use mhgu_sdk::monster::species::{
    SpeciesId,
    registry::{self, ServiceV2},
    valid_key,
};

mod native;
mod objectives;
mod save;

struct State {
    registry: Registry,
    slot: Option<u8>,
    pending_save_slot: Option<u8>,
    generation: u32,
}
struct Shared(AtomicBool, UnsafeCell<State>);
// All access goes through with(); no native call or file I/O while holding this lock.
unsafe impl Sync for Shared {}
static STATE: Shared = Shared(
    AtomicBool::new(false),
    UnsafeCell::new(State {
        registry: Registry::new(),
        slot: None,
        pending_save_slot: None,
        generation: 0,
    }),
);
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    while STATE
        .0
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop()
    }
    let value = f(unsafe { &mut *STATE.1.get() });
    STATE.0.store(false, Ordering::Release);
    value
}

fn species(raw: u32) -> Option<SpeciesId> {
    SpeciesId::new(u16::try_from(raw).ok()?)
}
unsafe extern "C" fn register(raw: u32, key: *const u8, len: usize, template: u32) -> u32 {
    let Some(id) = species(raw) else { return 3 };
    if id.is_native() {
        return 1;
    }
    let Some(template) =
        species(template).filter(|t| t.is_native() && (t.packed() ^ id.packed()) & 0x1000 == 0)
    else {
        return 4;
    };
    if key.is_null() || len == 0 || len > 96 {
        return 3;
    }
    let Ok(key) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(key, len) }) else {
        return 3;
    };
    if !valid_key(key) {
        return 3;
    }
    if with(|s| s.registry.register(id, key, template)) {
        0
    } else {
        2
    }
}
extern "C" fn contains(raw: u32) -> u32 {
    species(raw).is_some_and(|id| with(|s| s.registry.contains(id))) as u32
}
extern "C" fn counts(raw: u32) -> u32 {
    species(raw)
        .and_then(|id| with(|s| s.registry.counts(id)))
        .map_or(u32::MAX, |c| {
            u32::from(c.slain) | (u32::from(c.captured) << 16)
        })
}
static API: ServiceV2 = ServiceV2 {
    register,
    contains,
    counts,
};

// Only called for the exact live GU 1.4 object passed by the installed hook.
unsafe fn read<T: Copy>(object: u32, offset: usize) -> T {
    unsafe {
        (object as *const u8)
            .add(offset)
            .cast::<T>()
            .read_unaligned()
    }
}
unsafe fn write<T>(object: u32, offset: usize, v: T) {
    unsafe {
        (object as *mut u8)
            .add(offset)
            .cast::<T>()
            .write_unaligned(v)
    }
}

#[forge::hook(offset = 0x39f2b0)]
unsafe fn reset(quest: u32) -> u32 {
    with(|s| s.registry.begin_quest());
    original!(quest)
}
#[forge::hook(offset = 0x3a75c8)]
unsafe fn hunt(quest: u32, raw: u32, kind: u32, size: u32, special: u32) -> u32 {
    let Some(id) = species(raw) else { return 0 };
    if id.is_native() {
        return original!(quest, raw, kind, size, special);
    }
    // Never let an unknown key reach GU's unchecked 8 * lookup(key) write.
    let captured = kind == 1;
    if unsafe { read::<u8>(quest, 6007) } != 0 || !with(|s| s.registry.hunt(id, captured)) {
        return 0;
    }
    unsafe { objectives::advance(quest, id, captured) as u32 }
}
#[forge::hook(offset = 0x3a3930)]
unsafe fn hunted(quest: u32, raw: u32) -> u32 {
    let Some(id) = species(raw) else { return 0 };
    if id.is_native() {
        original!(quest, raw)
    } else {
        with(|s| s.registry.has_hunted(id)) as u32
    }
}
#[forge::hook(offset = 0x38d990)]
unsafe fn settle(results: u32) -> u32 {
    let base = forge::mem::text_addr();
    let quest = unsafe { read::<u32>(base, 0x188ad90) };
    // Same two exclusions as the native quest-to-player transfer.
    let eligible =
        quest != 0 && unsafe { read::<u8>(quest, 80) != 6 && read::<u8>(quest, 185) & 8 == 0 };
    let out = original!(results);
    if eligible {
        with(|s| s.registry.settle());
    }
    out
}

// On the ordinary load path GU reads exactly one selected slot, via the save
// manager +64, then closes its buffer. Do not use an inner deserializer: previews and
// three-slot bulk conversions call that same function for other characters.
#[forge::hook(offset = 0x3e21b8)]
unsafe fn select(manager: u32) -> u32 {
    let before = unsafe { read::<u32>(manager, 60) };
    let result = original!(manager);
    if before == 14 && unsafe { read::<u32>(manager, 60) } == 17 {
        save::select(unsafe { read(manager, 64) })
    }
    result
}

#[forge::hook(offset = 0x3e0980)]
unsafe fn save_slot(manager: u32, slot: u32) -> u32 {
    let result = original!(manager, slot);
    if result == 1 && slot < 3 {
        with(|s| s.pending_save_slot = Some(slot as u8));
    }
    result
}

#[forge::hook(offset = 0x3e1308)]
unsafe fn save_progress(manager: u32) -> u32 {
    let before = unsafe { read::<u32>(manager, 60) };
    let out = original!(manager);
    if before == 21 {
        match unsafe { read::<u32>(manager, 60) } {
            24 if unsafe { read::<u8>(manager, 112) } != 0 => save::finish(true),
            22 => save::finish(false),
            _ => {}
        }
    }
    out
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[forge::entry]
fn start() {
    if !unsafe { registry::publish(&API) } {
        return;
    }
    let base = forge::mem::text_addr();
    native::install(base);
    forge::install_hook!(base, reset);
    forge::install_hook!(base, hunt);
    forge::install_hook!(base, hunted);
    forge::install_hook!(base, settle);
    forge::install_hook!(base, select);
    forge::install_hook!(base, save_slot);
    forge::install_hook!(base, save_progress);
}
