use crate::{combat, magnet, motion, program};
use mhfz_sdk::action::Cursor;
use mhgu_sdk::{
    sync::Shared,
    weapon::style::{Input, PASS, action, input, movement},
};

const EVADE: u8 = 0;
const X: u8 = 2;
const A: u8 = 3;
const CHORD: u8 = 4;
const CHANGE_MODE: u8 = 5;
const MAGNET_ATTACK: u8 = 9;
const SHOT: u8 = 19;

fn pressed(commands: u32, key: u8) -> bool {
    commands & (1 << key) != 0
}

unsafe fn commands(actor: u32, input: &Input) -> u32 {
    unsafe {
        // GU's resolved pad path is shared across weapon kinds. The weapon
        // command bank is produced by the retail fallback weapon's mapper.
        let buttons = input::attack_buttons_pressed(actor);
        let auxiliary =
            buttons != 0 && action::weapon_drawn(actor) && input::auxiliary_shoulder_held(actor);
        let shot = auxiliary && buttons & 0x100 != 0;
        let magnet_attack = auxiliary && !shot && buttons & 0x20 != 0;
        let chord = !shot
            && !magnet_attack
            && (buttons == 0x120
                || input::attack_chord_pressed(actor)
                || input::attack_chord_with_shoulder(actor));
        let x = !shot && !magnet_attack && !chord && buttons & 0x20 != 0;
        let a = !shot && !chord && buttons & 0x100 != 0;
        let evade = input.command(3) && movement::has_evade_stamina(actor, 150.);
        [
            (X, x),
            (A, a),
            (CHORD, chord),
            (SHOT, shot),
            (MAGNET_ATTACK, magnet_attack),
            (CHANGE_MODE, input.command(8)),
            (EVADE, evade),
        ]
        .into_iter()
        .fold(0, |bits, (id, down)| bits | (u32::from(down) << id))
    }
}

#[derive(Default)]
struct Battle {
    mode: program::Mode,
    cursor: Option<Cursor>,
    aim: Option<[f32; 3]>,
}

impl Battle {
    // Returns true while the same state has another clip to process.
    unsafe fn play_step(&mut self, actor: u32) -> bool {
        let cursor = self.cursor.as_mut().unwrap();
        let Some(step) = program::step(cursor.state, cursor.time.step) else {
            unsafe { self.finish(actor) };
            return false;
        };
        if step.op == 2 {
            // FZ 81815D60 treats operand 1 as a state for opcode 2.
            unsafe { self.branch(actor, step.clip) };
            return false;
        }
        if !unsafe { motion::play(actor, step.clip, step.start, step.blend) } {
            unsafe { self.finish(actor) };
            return false;
        }
        cursor.time.cycles = 0;
        cursor.time.frame = f32::from(step.start);
        cursor.time.blend = f32::from(step.blend / 2);
        unsafe { Self::events(actor, cursor, &mut self.aim) };
        true
    }

    unsafe fn events(actor: u32, cursor: &mut Cursor, aim: &mut Option<[f32; 3]>) {
        while let Some((event, value, late)) = cursor.next_event() {
            match event {
                // FZ 81225C5C writes the drawn byte used by 8121DA12.
                2 => unsafe { action::set_weapon_drawn(actor, value != 0) },
                // FZ 81225C60 spawns a single family-120 contact.
                3 => unsafe { combat::strike(actor, value, cursor.time.frame, late) },
                // FZ 81225FC6 snapshots the camera at event 17.
                17 if matches!(cursor.state, 82 | 84) => *aim = Some(unsafe { magnet::aim(actor) }),
                4 if value == 1 => unsafe {
                    let direction = aim.take().unwrap_or_else(|| magnet::aim(actor));
                    magnet::launch(actor, direction);
                },
                _ => {}
            }
        }
    }

    unsafe fn start(&mut self, actor: u32, id: u8) -> bool {
        unsafe {
            action::start_weapon_state(actor);
            let cost = program::cost(id);
            if cost != 0 && !magnet::spend(actor, cost) {
                self.finish(actor);
                return true;
            }
            self.mode = self.mode.for_state(id);
            self.aim = None;
            motion::set_mode(actor, self.mode);
            self.cursor = Some(program::cursor(id));
            self.play_step(actor);
        }
        true
    }

    unsafe fn finish(&mut self, actor: u32) {
        self.cursor = None;
        unsafe { action::finish_weapon_state(actor) };
    }

    unsafe fn branch(&mut self, actor: u32, target: u16) -> bool {
        if matches!(target, 202 | 203) {
            if unsafe { action::transition_to(actor, 1, 6) } {
                self.cursor = None;
                return true;
            }
            return false;
        }
        let Ok(id) = u8::try_from(target) else {
            return false;
        };
        if !program::ACTIONS.contains(id) {
            return false;
        }
        let cost = program::cost(id);
        if cost != 0 && !(unsafe { movement::has_stamina(actor) } && magnet::can_spend(actor, cost))
        {
            return false;
        }
        if unsafe { action::accept_custom_start(actor, id) } {
            return unsafe { self.start(actor, id) };
        }
        false
    }

    unsafe fn advance(&mut self, actor: u32, state: u8, commands: u32) -> bool {
        let Some(cursor) = self.cursor.as_mut().filter(|cursor| cursor.state == state) else {
            return unsafe { self.start(actor, state) };
        };
        let Some(step) = program::step(state, cursor.time.step) else {
            unsafe { self.finish(actor) };
            return true;
        };
        if step.op == 2 {
            return unsafe { self.branch(actor, step.clip) };
        }
        let sample = unsafe { action::MotionFrame::sample(actor) };
        let wrapped = cursor.time.observe(sample.frame, sample.blend);
        unsafe { Self::events(actor, cursor, &mut self.aim) };
        // FZ 8121F57C advances 81815D60 before evaluating the current
        // step's branch windows through 81815F30.
        if program::step_complete(step, cursor.time, sample, wrapped) {
            cursor.time.step += 1;
            if !unsafe { self.play_step(actor) } {
                return true;
            }
        }
        let Some(cursor) = self.cursor.as_mut() else {
            return true;
        };
        if cursor.art_window(|condition| condition == 0)
            && unsafe { action::activate_hunter_art(actor) }
        {
            self.cursor = None;
            return true;
        }
        // FZ 824F0DF4 conditions 1 through 5 check stamina and magnetic segments.
        if let Some(next) = cursor.branch(commands, |condition| match condition {
            0 => true,
            1..=5 => {
                (unsafe { movement::has_stamina(actor) })
                    && magnet::can_spend(actor, condition as u8)
            }
            _ => false,
        }) {
            unsafe { self.branch(actor, next) };
        }
        true
    }

    unsafe fn tick(&mut self, actor: u32, input: &Input) -> bool {
        let current = unsafe { action::current(actor) };
        let commands = unsafe { commands(actor, input) };
        if let Some(state) = action::custom_id(current.0, current.1) {
            return unsafe { self.advance(actor, state, commands) };
        }
        self.cursor = None;
        let drawn = unsafe { action::weapon_drawn(actor) };
        if !drawn {
            self.mode = program::Mode::Slash;
            motion::set_mode(actor, self.mode);
        }
        if !(movement::ground_idle(current) || !drawn && movement::sheathed_movement(current)) {
            return false;
        }
        if unsafe { action::activate_hunter_art(actor) } {
            return true;
        }
        let Some(id) = [SHOT, MAGNET_ATTACK, CHORD, X, A, CHANGE_MODE]
            .into_iter()
            .find_map(|key| {
                if !pressed(commands, key) {
                    return None;
                }
                self.mode.opener(key, drawn)
            })
        else {
            return false;
        };
        if drawn {
            unsafe { self.branch(actor, u16::from(id)) }
        } else if unsafe { action::enter_draw_attack(actor, id) } {
            unsafe { self.start(actor, id) }
        } else {
            false
        }
    }
}

static LOCAL: Shared<Battle> = Shared::new(Battle {
    mode: program::Mode::Slash,
    cursor: None,
    aim: None,
});

pub unsafe extern "C" fn dispatch(actor: u32, input: *const Input) -> u32 {
    LOCAL.with(|battle| {
        let Some(input) = (unsafe { input.as_ref() }) else {
            *battle = Battle::default();
            magnet::reset();
            motion::set_mode(0, program::Mode::Slash);
            return PASS;
        };
        if unsafe { battle.tick(actor, input) } {
            0
        } else {
            PASS
        }
    })
}
