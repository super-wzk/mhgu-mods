use crate::{program, sound};
use mhfz_sdk::{action::Cursor, combat::attacker_hitstop};
use mhgu_sdk::{
    combat::collision::CachedHitShape,
    weapon::style::action::{
        self,
        melee::{DamageKind, MonsterReaction, Strike, StrikePower},
    },
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

static BIND_FINISHER: CachedHitShape =
    CachedHitShape::new(c"player\\hit\\frontier_ms\\bind_finisher");

pub unsafe fn preload() {
    for shape in &MELEE {
        unsafe { shape.preload() };
    }
    unsafe { BIND_FINISHER.preload() };
}

pub unsafe fn events(
    actor: u32,
    cursor: &mut Cursor,
    bonus: u8,
    mut on_event: impl FnMut(u16, u16, f32),
) {
    while let Some((event, value, late)) = cursor.next_motion_event() {
        match event {
            // 81225C3E switches the bind opener to airborne mode mid-clip.
            1 if value == 3 && program::is_bind_drop(cursor.state) => {
                unsafe { action::start_airborne_weapon_state(actor) };
                on_event(event, value, late);
            }
            // FZ 81225C5C writes the drawn byte used by 8121DA12.
            2 => unsafe { action::set_weapon_drawn(actor, value != 0) },
            // FZ 81225C60 spawns a single family-120 contact.
            3 => unsafe { strike(actor, value, bonus) },
            4 if value == 1 => {
                unsafe { sound::shot(actor) };
                on_event(event, value, late);
            }
            // FZ 81225DBA writes the action's base motion rate as a percentage.
            13 => unsafe { action::set_motion_rate(actor, f32::from(value) / 100.) },
            _ => on_event(event, value, late),
        }
    }
}

unsafe fn strike(actor: u32, id: u16, bonus: u8) {
    let Some(row) = program::melee_row(id) else {
        return;
    };
    let selector = u16::from_le_bytes([row[16], row[17]]);
    let Some(shape) = usize::from(selector)
        .checked_sub(1)
        .and_then(|index| MELEE.get(index))
    else {
        return;
    };
    // FZ 81225C60 arms deflection for each family-120 melee event.
    unsafe { submit_strike(actor, row, shape, bonus, true) };
}

pub unsafe fn bind_finisher(actor: u32, bonus: u8) -> bool {
    // Events 43/44 spawn a family-121 effect. Its delay is relative to the
    // effect's birth, and it does not arm the family-120 deflection window.
    unsafe {
        submit_strike(
            actor,
            program::bind_finisher_row(),
            &BIND_FINISHER,
            bonus,
            false,
        )
    }
}

unsafe fn submit_strike(
    actor: u32,
    row: &[u8; 40],
    shape: &CachedHitShape,
    bonus: u8,
    deflectable: bool,
) -> bool {
    let half = |at: usize| u16::from_le_bytes([row[at], row[at + 1]]);
    let damage = match row[15] {
        1 => DamageKind::Impact,
        2 => DamageKind::Slash,
        _ => return false,
    };
    let Some(shape) = (unsafe { shape.load() }) else {
        return false;
    };
    let (delay, duration) = mhfz_sdk::combat::contact_window(half(0), half(2));
    Strike {
        delay,
        duration,
        power: StrikePower {
            motion: half(4),
            bonus,
        },
        damage,
        stun: row[33],
        hitstop: attacker_hitstop(row[29]),
        deflectable,
        reaction: if half(6) == 236 {
            MonsterReaction::Topple
        } else {
            MonsterReaction::Native
        },
    }
    .submit(actor, &shape)
}
