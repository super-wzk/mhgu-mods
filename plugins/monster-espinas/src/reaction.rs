//! Retain GU's native damage action interpreter (A134C) and replace only
//! the motion program it receives for ordinary ground flinches.
use super::*;
use espinas_behavior::Request;
use mhgu_sdk::monster::{
    action::ActionId,
    damage::{DamageMotionEnd, DamageProgram, DamageStep},
};

const GROUND_FLINCH: ActionId = ActionId { group: 10, slot: 7 };

const MAX_STEPS: usize = 4; // three motions and one timer
static REACTIONS: Actors<DamageProgram<MAX_STEPS>> = Actors::new();
pub(super) fn forget(actor: u32) {
    REACTIONS.with(|r| {
        r.remove(&actor);
    });
}
#[forge::hook(offset = 0xa134c)]
pub unsafe fn flinch(actor: u32, program: u32, mode: u32) -> u32 {
    let replacement = unsafe { Monster::from_raw(actor) }.and_then(flinch_program);
    original!(actor, replacement.unwrap_or(program), mode)
}

fn flinch_program(monster: Monster) -> Option<u32> {
    if monster.action() != GROUND_FLINCH {
        return None;
    }
    let directory = Directory::read(monster)?;
    let state = monster.state()?;
    let actor = monster.as_raw();
    REACTIONS.with(|programs| {
        let program = if state.action_stage() == 0 {
            let (part, count) = state.flinch_part_and_count();
            let program = programs.entry(actor).or_default();
            replace_flinch(program, &directory, part, count)?;
            program
        } else {
            programs.get(&actor)?
        };
        Some(program.as_ptr() as u32)
    })
}

fn replace_flinch(
    program: &mut DamageProgram<MAX_STEPS>,
    directory: &Directory,
    part: u8,
    count: u8,
) -> Option<()> {
    use DamageMotionEnd::{MotionFinished, TimerExpired};
    let play = |clip, until| {
        let entry = unsafe { directory.lookup(Request(249, clip)) }?;
        let (motion, blend) = unsafe { Directory::playback(entry) };
        Some(DamageStep::Play {
            motion,
            blend: f32::from(blend),
            until,
        })
    };
    let replaced = if matches!(part, 3 | 4) && count > 1 {
        let (first, hold, last) = if part == 3 {
            (69, 70, 71)
        } else {
            (74, 75, 72)
        };
        program.replace(&[
            play(first, MotionFinished)?,
            DamageStep::SetTimer(240),
            play(hold, TimerExpired)?,
            play(last, MotionFinished)?,
        ])
    } else {
        let clip = match part {
            0 | 7 => 60,
            1 | 2 => 64,
            3 | 4 => 66,
            _ => 63,
        };
        program.replace(&[play(clip, MotionFinished)?])
    };
    replaced.then_some(())
}
