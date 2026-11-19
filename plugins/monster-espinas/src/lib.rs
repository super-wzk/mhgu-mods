//! Frontier Espinas on GU 1.4. Register the species before installing its hooks.
#![no_std]
extern crate alloc;
use alloc::collections::BTreeMap;
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};
use mhgu_sdk::monster::{Monster, MonsterClass, MonsterUpdate, species::SpeciesId};

struct Actors<T> {
    locked: AtomicBool,
    value: UnsafeCell<BTreeMap<u32, T>>,
}
unsafe impl<T: Send> Sync for Actors<T> {}
impl<T> Actors<T> {
    const fn new() -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(BTreeMap::new()),
        }
    }
    fn remove(&self, actor: u32) {
        self.with(|map| {
            map.remove(&actor);
        });
    }
    fn with<R>(&self, f: impl FnOnce(&mut BTreeMap<u32, T>) -> R) -> R {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        let r = f(unsafe { &mut *self.value.get() });
        self.locked.store(false, Ordering::Release);
        r
    }
}
unsafe fn u32at(p: u32, off: u32) -> u32 {
    unsafe { ((p + off) as *const u32).read_unaligned() }
}
unsafe fn f32at(p: u32, off: u32) -> f32 {
    unsafe { f32::from_bits(u32at(p, off)) }
}
unsafe fn u16at(p: u32, off: u32) -> u16 {
    unsafe { ((p + off) as *const u16).read_unaligned() }
}
unsafe fn byte(p: u32, off: u32) -> u8 {
    unsafe { *((p + off) as *const u8) }
}
mod air;
mod ground;
mod hunt;
mod icon;
mod materials;
mod melee;
mod motion;
mod package;
mod physiology;
mod quest;
mod reaction;
mod resources;

const SPECIES: SpeciesId = SpeciesId::new(107).unwrap();
// The GU factory allocates a flying wyvern with this layout. Instance
// identity and asset resolution never use the layout's species ID.
const FACTORY_LAYOUT: SpeciesId = SpeciesId::new(1).unwrap();
const ACTOR_VTABLE_OFFSET: u32 = 0x179acf8;
const ACTOR_CLASS: MonsterClass = MonsterClass::at_vtable(ACTOR_VTABLE_OFFSET);
fn is_espinas(monster: Monster) -> bool {
    monster.species() == Some(SPECIES) && ACTOR_CLASS.matches(monster)
}

#[forge::hook(offset = 0xcf5f64)]
unsafe fn tick(actor: u32, flags: u32) -> u32 {
    let outcome = unsafe { MonsterUpdate::from_callback(actor) }
        .map_or(hunt::ActionOutcome::UseOriginal, hunt::update_action);
    match outcome {
        hunt::ActionOutcome::UseOriginal => original!(actor, flags),
        hunt::ActionOutcome::Handled => 0,
    }
}

#[forge::hook(offset = 0x6ed40)]
unsafe fn release(actor: u32) -> u32 {
    hunt::forget(actor);
    physiology::forget(actor);
    reaction::forget(actor);
    melee::forget(actor);
    motion::forget(actor);
    original!(actor)
}
// Forge 1.5 generates a safe extern handler that dereferences its ABI argument.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
mod entry {
    use super::*;
    use mhgu_sdk::monster::species::registry::MonsterRegistry;

    #[forge::entry]
    fn main() {
        // Forge init runs on the game thread. Frontier's 0x50 is Glavenus in GU.
        let Some(registry) = MonsterRegistry::current() else {
            return;
        };
        if let Err(err) = registry.register("frontier:espinas", SPECIES.packed(), FACTORY_LAYOUT) {
            forge::log::prelude::warn!("Espinas disabled: {err:?}");
            return;
        }
        let base = forge::mem::text_addr();
        resources::install(base);
        quest::install(base);
        icon::install(base);
        materials::install(base);
        use super::physiology::{add_anger, mode, reset_anger};
        use super::reaction::damage_program;
        motion::install(base);
        forge::install_hook!(base, tick);
        melee::install(base);
        forge::install_hook!(base, release);
        forge::install_hook!(base, reset_anger);
        forge::install_hook!(base, damage_program);
        forge::install_hook!(base, add_anger);
        forge::install_hook!(base, mode);
        forge::log::prelude::info!("Espinas: native hunter targeting active");
    }
}
