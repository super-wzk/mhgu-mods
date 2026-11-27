//! Resolve the physical w13 chords before GU turns the same edges into common actions.
use crate::battle;
use mhfz_sdk::action::Time;
use mhgu_sdk::{
    sync::Shared,
    weapon::style::{
        action,
        input::{self, Buttons, PadButton},
        movement,
    },
};

const X: u16 = PadButton::X as u16;
const A: u16 = PadButton::A as u16;
const B: u16 = PadButton::B as u16;
const Y: u16 = PadButton::Y as u16;
const R: u16 = PadButton::R as u16;
const ZL: u16 = PadButton::Zl as u16;
const ZR: u16 = PadButton::Zr as u16;
pub(super) const SPECIAL: u16 = 0x1000;
const FACES: u16 = X | A | B | Y;
// 27CA28 gives the following two 30 Hz polls to the rest of a combination.
const GRACE: f32 = 4.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Ground(bool),
    Roll,
    Weapon(u8),
}

fn scope(actor: u32) -> Option<Scope> {
    let current = unsafe { action::current(actor) };
    let drawn = unsafe { action::weapon_drawn(actor) };
    if let Some(id) = action::custom_id(current.0, current.1) {
        Some(Scope::Weapon(id))
    } else if drawn && current == (1, 6) {
        Some(Scope::Roll)
    } else if movement::ground_idle(current) || !drawn && movement::sheathed_movement(current) {
        Some(Scope::Ground(drawn))
    } else {
        None
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Stroke {
    pub held: u16,
    pub pressed: u16,
    pub time: Option<Time>,
    pub strength: i16,
}

struct Pending {
    stroke: Stroke,
    remaining: f32,
}

#[derive(Default)]
struct Controls {
    owner: u32,
    scope: Option<Scope>,
    pending: Option<Pending>,
    ready: Stroke,
    passthrough: u16,
}

static CONTROLS: Shared<Controls> = Shared::new(Controls {
    owner: 0,
    scope: None,
    pending: None,
    ready: Stroke {
        held: 0,
        pressed: 0,
        time: None,
        strength: 0,
    },
    passthrough: 0,
});

impl Controls {
    fn poll(
        &mut self,
        buttons: &mut Buttons,
        drawn: bool,
        special: u16,
        time: Option<Time>,
        strength: i16,
        elapsed: f32,
    ) {
        let held = buttons.held;
        let face_mask = if drawn {
            FACES | SPECIAL | special
        } else {
            X | A
        };
        let face = buttons.pressed & face_mask;
        let changed = buttons.pressed & (R | ZL);
        self.ready = Stroke {
            held,
            pressed: buttons.pressed & ZR,
            time: None,
            strength,
        };

        let mut pass = self.passthrough & !face;
        if let Some(p) = &mut self.pending {
            p.remaining -= elapsed;
        }
        if face != 0 {
            let p = self.pending.get_or_insert(Pending {
                stroke: Stroke {
                    held,
                    pressed: 0,
                    time,
                    strength,
                },
                remaining: GRACE,
            });
            p.stroke.pressed |= face;
            if face & special != 0 {
                p.stroke.pressed |= SPECIAL;
            }
            p.stroke.held |= held & (face_mask | R | ZL);
            // A completed chord belongs to the pose where its last key arrived.
            p.stroke.time = time;
            p.stroke.strength = strength;
        } else if let Some(p) = &mut self.pending {
            let newly_modified = changed & !p.stroke.held;
            if newly_modified != 0 {
                p.stroke.held |= newly_modified;
                p.stroke.time = time;
                p.stroke.strength = strength;
            }
        }
        // GU may see several dispatches for one poll. Publish one press and
        // retain physical holds for aiming and charging on later dispatches.
        let mut released_to_gu = 0;
        if let Some(p) = &self.pending {
            let keys = p.stroke.pressed;
            let mods = p.stroke.held & (R | ZL);
            let complete = drawn && mods & ZL != 0 && keys & (X | A | B) != 0
                || mods & R != 0
                    && (p.stroke.held & (X | A) == X | A
                        || drawn && keys & (B | Y | SPECIAL | special) != 0);
            if complete || p.remaining <= 0. || held & keys == 0 {
                self.ready = p.stroke;
                self.ready.pressed |= buttons.pressed & ZR;
                self.pending = None;
                if matches!(self.scope, Some(Scope::Ground(_))) {
                    released_to_gu = if !drawn && keys & X == 0 {
                        keys & A
                    } else if drawn && mods == 0 && keys & (X | A | SPECIAL | special) == 0 {
                        keys & (B | Y)
                    } else {
                        0
                    };
                }
                pass |= released_to_gu;
            }
        }
        // Protect the common command resolver, weapon mapper and action
        // tables. +5116/+5124 remain the player's physical state.
        let shoulders = if drawn { R | ZL | ZR } else { 0 };
        let mask = face_mask & !pass | shoulders;
        buttons.held = (held & !mask) | released_to_gu;
        buttons.pressed = (buttons.pressed & !mask) | released_to_gu;
        buttons.previous_pressed &= !mask;
        buttons.released &= !mask;
        self.passthrough = pass & held;
    }
}

pub(super) unsafe extern "C" fn poll(actor: u32, buttons: &mut Buttons) {
    let scope = scope(actor);
    let raw_edges = unsafe {
        ((actor + 5116) as *const u32).read()
            | ((actor + 5120) as *const u32).read()
            | ((actor + 5124) as *const u32).read()
    };
    let suppressed =
        buttons.held | buttons.pressed | buttons.released == 0 && raw_edges as u16 != 0;
    let drawn = unsafe { action::weapon_drawn(actor) };
    let item_selector = !suppressed
        && scope.is_some()
        && drawn
        && unsafe { input::reserve_shoulder_gestures(actor, R | ZL | ZR, raw_edges as u16) };
    let forced =
        item_selector || suppressed || unsafe { ((actor + 9493) as *const u8).read() & 4 != 0 };
    let time = if let Some(Scope::Weapon(id)) = scope {
        battle::press_time(actor, id)
    } else {
        None
    };
    let strength = unsafe { ((actor + 5136) as *const i16).read() };
    let elapsed = unsafe { ((actor + 28) as *const f32).read() }.clamp(0., GRACE);
    let special = unsafe { input::special_attack_mask() };
    CONTROLS.with(|state| {
        if forced || state.owner != actor || state.scope != scope {
            *state = Controls {
                owner: actor,
                scope,
                ..Default::default()
            };
        }
        if forced || scope.is_none() {
            return;
        }
        state.poll(buttons, drawn, special, time, strength, elapsed);
    });
}

pub(super) fn pending(actor: u32) -> Option<Stroke> {
    CONTROLS.with(|s| {
        s.pending
            .as_ref()
            .filter(|_| s.owner == actor)
            .map(|p| p.stroke)
    })
}

pub(super) fn take(actor: u32) -> Option<Stroke> {
    CONTROLS.with(|state| {
        if state.owner != actor || state.scope.is_none() {
            return None;
        }
        if state.scope != scope(actor) {
            return Some(Stroke::default());
        }
        let ready = state.ready;
        state.ready.pressed = 0;
        state.ready.time = None;
        Some(ready)
    })
}

pub(super) fn reset(actor: u32) {
    CONTROLS.with(|s| {
        if s.owner == actor {
            *s = Controls::default()
        }
    });
}
