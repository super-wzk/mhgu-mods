use crate::{program::Gauges, read};
use mhgu_sdk::weapon::style::{action, input as pad, movement};

const AIR_WINDOW: f32 = 8.;
const X_BUTTON: u16 = 0x20;
const A_BUTTON: u16 = 0x100;
const ATTACK_BUTTONS: u16 = X_BUTTON | A_BUTTON;

#[derive(Clone, Copy)]
pub(super) enum Attack {
    X(bool),
    A,
    Chord,
    Slash(u8),
}

impl Attack {
    pub(super) unsafe fn pressed(actor: u32, moving: bool, slash_dir: Option<u8>) -> Option<Self> {
        match unsafe { read::<u16>(actor, 5112) } & ATTACK_BUTTONS {
            0 => None,
            pressed if pressed == ATTACK_BUTTONS || unsafe { pad::attack_chord_pressed(actor) } => {
                Some(slash_dir.map_or(Self::Chord, Self::Slash))
            }
            X_BUTTON => Some(Self::X(moving)),
            A_BUTTON => Some(Self::A),
            _ => None,
        }
    }

    pub(super) fn action(self, gauges: Gauges, slash: u16) -> Option<u8> {
        Some(match (self, gauges.sword) {
            (Self::X(moving), sword) => ground_x(sword, moving),
            (Self::A, false) => 1,
            (Self::A, true) => 78,
            (Self::Chord, false) => 2,
            (Self::Chord, true) => 70,
            (Self::Slash(dir), sword) if gauges.energy(slash) > 0 => {
                (if sword { 152 } else { 106 }) + dir
            }
            (Self::Slash(_), _) => return None,
        })
    }
}

pub(super) fn ground_x(sword: bool, moving: bool) -> u8 {
    match (sword, moving) {
        (false, false) => 0,
        (false, true) => 3,
        (true, false) => 5,
        (true, true) => 17,
    }
}

enum Phase {
    Airborne,
    Recovery { from_slot: u16 },
}

fn has_separate_recovery(slot: u16) -> bool {
    // 11A403C lands directly into the separate W08 60/68 motions.
    matches!(slot, 67 | 71 | 113 | 114)
}

impl Phase {
    unsafe fn read(actor: u32, (group, slot): (u16, u16)) -> Option<Self> {
        if group != action::GROUP {
            return None;
        }
        match slot {
            // GU W08 11A392C / 11A3C58 recover in the same action slot.
            56 | 57 | 63 | 65 | 108 | 109 | 111 | 112 | 115 | 116 => {
                match unsafe { read::<u8>(actor, 5) } {
                    1 => Some(Self::Airborne),
                    2 | 3 => Some(Self::Recovery { from_slot: slot }),
                    _ => None,
                }
            }
            60 | 68 => {
                let (group, from_slot) = unsafe { action::previous(actor) };
                (group == action::GROUP && has_separate_recovery(from_slot))
                    .then_some(Self::Recovery { from_slot })
            }
            _ if has_separate_recovery(slot) && unsafe { read::<u8>(actor, 5) } == 1 => {
                Some(Self::Airborne)
            }
            _ => None,
        }
    }
}

pub(super) unsafe fn recovering(actor: u32, current: (u16, u16)) -> bool {
    matches!(
        unsafe { Phase::read(actor, current) },
        Some(Phase::Recovery { .. })
    )
}

pub(super) struct Pending {
    attack: Attack,
    slot: u16,
    area: u8,
    remaining: f32,
    landed: bool,
}

impl Pending {
    pub(super) unsafe fn update(
        pending: &mut Option<Self>,
        actor: u32,
        current: (u16, u16),
        drawn: bool,
        dt: f32,
        pressed: Option<Attack>,
    ) -> Option<Attack> {
        let saved = pending.take();
        if !drawn {
            return None;
        }
        let area = unsafe { read::<u8>(actor, 4180) };
        let saved = saved.filter(|entry| entry.area == area);
        if movement::ground_idle(current) {
            let entry = saved.filter(|entry| entry.landed)?;
            return (unsafe { action::previous(actor) } == (action::GROUP, entry.slot))
                .then_some(pressed.unwrap_or(entry.attack));
        }
        let slot = current.1;
        let (landed, from_slot) = match unsafe { Phase::read(actor, current) }? {
            Phase::Airborne => (false, slot),
            Phase::Recovery { from_slot } => (true, from_slot),
        };
        let (attack, remaining) = match pressed {
            Some(attack) => (attack, AIR_WINDOW),
            None => {
                let entry = saved?;
                if (entry.slot != slot && entry.slot != from_slot) || (entry.landed && !landed) {
                    return None;
                }
                let remaining = if landed {
                    entry.remaining
                } else {
                    entry.remaining - dt
                };
                (entry.attack, remaining)
            }
        };
        *pending = (remaining > 0.).then_some(Self {
            attack,
            slot,
            area,
            remaining,
            landed,
        });
        None
    }
}
