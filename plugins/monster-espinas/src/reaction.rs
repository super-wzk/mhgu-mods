//! Retain GU's native damage action interpreter (A134C) and replace only
//! the motion programs that need species-specific slots.
use super::{Actors, package::Directory};
use espinas_behavior::Request;
use mhgu_sdk::monster::{
    Monster,
    action::ActionId,
    damage::{DamageMotionEnd, DamageProgram, DamageStep},
};

const GROUND_FLINCH: ActionId = ActionId { group: 10, slot: 7 };

const CAPTURE_IN_TRAP: ActionId = ActionId {
    group: 11,
    slot: 17,
};

// 11:17 otherwise shares the terrestrial death program. FZ 6:4 uses
// motion 97 when already trapped; GU command 7 after playback still runs.
#[repr(C)]
struct CaptureCommand(u32, Option<&'static [u32; 6]>);
static TRAPPED_CAPTURE: [CaptureCommand; 3] = [
    CaptureCommand(0, Some(&[0x314, 8f32.to_bits(), 0, 0, 1, 0])),
    CaptureCommand(7, None),
    CaptureCommand(255, None),
];

const MAX_STEPS: usize = 4; // three motions and one timer
static REACTIONS: Actors<DamageProgram<MAX_STEPS>> = Actors::new();
pub(super) fn forget(actor: u32) {
    REACTIONS.remove(actor);
}
#[forge::hook(offset = 0xa134c)]
pub unsafe fn damage_program(actor: u32, program: u32, mode: u32) -> u32 {
    let replacement = unsafe { Monster::from_raw(actor) }.and_then(|monster| {
        // Native damage/death programs correspond to FZ groups 4–8.
        if matches!(monster.action().group, 10 | 11) && crate::is_espinas(monster) {
            monster.set_base_motion_rate(1.);
        }
        replacement_program(monster)
    });
    original!(actor, replacement.unwrap_or(program), mode)
}

fn replacement_program(monster: Monster) -> Option<u32> {
    match monster.action() {
        CAPTURE_IN_TRAP if super::is_espinas(monster) => {
            return Some(TRAPPED_CAPTURE.as_ptr() as u32);
        }
        GROUND_FLINCH => (),
        _ => return None,
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
