//! Deliver authored ground reactions inside GU's monster damage pass.
use alloc::vec::Vec;
use mhgu_sdk::{
    combat::collision::hurt::{MonsterSpawn, prepare_ground_topple},
    service,
    sync::Shared,
};

struct PendingTopple {
    hunter: u32,
    style: u32,
    target: u32,
    spawn: MonsterSpawn,
}

static PENDING: Shared<Vec<PendingTopple>> = Shared::new(Vec::new());

pub(super) fn queue(hunter: u32, style: u32, target: u32, spawn: MonsterSpawn) {
    if service::on_game_thread() {
        PENDING.with(|pending| {
            pending.retain(|entry| entry.target != target);
            pending.push(PendingTopple {
                hunter,
                style,
                target,
                spawn,
            });
        });
    }
}

pub(super) fn clear(hunter: u32) {
    PENDING.with(|pending| pending.retain(|entry| entry.hunter != hunter));
}

fn take(target: u32) -> Option<PendingTopple> {
    PENDING.with(|pending| {
        let position = pending.iter().position(|entry| entry.target == target)?;
        Some(pending.swap_remove(position))
    })
}

#[forge::hook(offset = 0x9d67c)]
unsafe fn clear_damage(actor: u32) -> u32 {
    // GU clears a monster's accumulated damage before the next collision
    // batch. A reaction that was bypassed expires with those same hits.
    take(actor);
    original!(actor)
}

#[forge::hook(offset = 0x9a828)]
unsafe fn react(actor: u32) -> u32 {
    if let Some(pending) = take(actor)
        && service::on_game_thread()
    {
        let hunter = pending.hunter;
        let entry = unsafe {
            if crate::weapon::local_hunter() == hunter {
                crate::weapon::style::local_entry(hunter)
            } else {
                crate::weapon::style::multiplayer::peer_entry(hunter)
            }
        };
        if entry.is_some_and(|entry| entry.style.id == pending.style)
            && unsafe { MonsterSpawn::from_owned_damage_target(hunter, actor) }
                == Some(pending.spawn)
        {
            unsafe { prepare_ground_topple(actor) };
        }
    }
    original!(actor)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, clear_damage);
    forge::install_hook!(base, react);
}
