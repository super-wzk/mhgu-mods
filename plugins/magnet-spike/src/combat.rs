use crate::program;
use mhfz_sdk::combat::{HitWindow, attacker_hitstop};
use mhgu_sdk::{
    combat::collision::CachedHitShape,
    weapon::style::action::melee::{DamageKind, Strike},
};

static MELEE: [CachedHitShape; 8] = [
    CachedHitShape::new(c"player\\hit\\frontier_ms\\melee_1"),
    CachedHitShape::new(c"player\\hit\\frontier_ms\\melee_2"),
    CachedHitShape::new(c"player\\hit\\frontier_ms\\melee_3"),
    CachedHitShape::new(c"player\\hit\\frontier_ms\\melee_4"),
    CachedHitShape::new(c"player\\hit\\frontier_ms\\melee_5"),
    CachedHitShape::new(c"player\\hit\\frontier_ms\\melee_6"),
    CachedHitShape::new(c"player\\hit\\frontier_ms\\melee_7"),
    CachedHitShape::new(c"player\\hit\\frontier_ms\\melee_8"),
];

pub unsafe fn preload() {
    for shape in &MELEE {
        unsafe { shape.preload() };
    }
}

pub unsafe fn strike(actor: u32, id: u16, frame: f32, late: f32) {
    let Some(row) = program::melee_row(id) else {
        return;
    };
    let half = |at: usize| u16::from_le_bytes([row[at], row[at + 1]]);
    let Some(window) = HitWindow::new(half(0), half(2), frame, late) else {
        return;
    };
    let damage = match row[15] {
        1 => DamageKind::Impact,
        2 => DamageKind::Slash,
        _ => return,
    };
    let Some(shape) = usize::from(half(16))
        .checked_sub(1)
        .and_then(|index| MELEE.get(index))
        .and_then(|shape| unsafe { shape.load() })
    else {
        return;
    };
    Strike {
        delay: window.delay,
        duration: window.duration,
        power: half(4),
        damage,
        stun: row[33],
        hitstop: attacker_hitstop(row[29]),
    }
    .submit(actor, &shape);
}
