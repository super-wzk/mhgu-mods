use mhfz_sdk::{effects::MotionClock, sound::play_events};
use mhgu_sdk::weapon::style::action::MotionFrame;
use mhgu_sdk::weapon::style::{
    particle,
    sound::{Bank, ContactSound, contact},
};

static BANKS: [Bank; 5] = [
    Bank::new(c"sound\\se\\pl\\frontier_ms\\blade"),
    Bank::new(c"sound\\se\\pl\\frontier_ms\\common"),
    Bank::new(c"sound\\se\\pl\\frontier_ms\\weapon"),
    Bank::new(c"sound\\se\\pl\\frontier_ms\\voice_m"),
    Bank::new(c"sound\\se\\pl\\frontier_ms\\voice_f"),
];
const TRACKS: &[u8] = include_bytes!("../target/assets/frontier_ms_sound.flse");

pub(super) unsafe fn preload() {
    for bank in &BANKS {
        unsafe { bank.preload() };
    }
}

pub(super) unsafe fn sample(clock: &mut MotionClock, actor: u32, clip: u16, sample: MotionFrame) {
    let window = clock.window(clip, sample);
    if window.is_empty() {
        return;
    }
    unsafe {
        play_events(
            TRACKS,
            clip,
            |frame| window.contains(&frame),
            |bank, cue| match bank {
                3 => contact(actor, ContactSound::Footstep(cue)),
                4 if cue == 0 => contact(actor, ContactSound::Armour),
                4 if cue == 4 => contact(actor, ContactSound::WaistArmour),
                2 if cue >= 32 => {
                    let equipment = ((actor + 5060) as *const u32).read();
                    let female = equipment != 0 && ((equipment + 1240) as *const u8).read() == 1;
                    BANKS[3 + usize::from(female)].play(actor, cue);
                }
                0..=2 => BANKS[usize::from(bank)].play(actor, cue),
                _ => {}
            },
        )
    };
}

pub(super) unsafe fn splash(actor: u32, surface: u16, point: [f32; 3]) {
    // 81F99FA4 calls 81F96370 with unit strength. Class 1 has a separate sound.
    if matches!(surface, 0 | 3 | 20) {
        unsafe { BANKS[1].play_at(actor, 46, point) };
    } else if crate::magnet::scene_tick() & 7 != 0 {
        unsafe { BANKS[1].play_at(actor, 112, point) };
    }
}

pub(super) unsafe fn charge(actor: u32) {
    // 812223E6 queues we13 request 7 on reaching the impact charge threshold.
    unsafe { BANKS[0].play(actor, 7) };
}

pub(super) unsafe fn shot(actor: u32) {
    // 811659F8 plays we13 request 9 when the shot enters the scene.
    unsafe {
        if let Some(position) = particle::joint_position(actor, 12) {
            BANKS[0].play_at(actor, 9, position);
        } else {
            BANKS[0].play(actor, 9);
        }
    }
}

pub(super) unsafe fn shock(actor: u32, point: [f32; 3]) {
    // 8102D1D2 and 8102D1E2 layer the two requests at the first flash.
    unsafe {
        BANKS[0].play_at(actor, 23, point);
        BANKS[0].play_at(actor, 5, point);
    }
}

pub(super) unsafe fn at(actor: u32, cue: u8, point: [f32; 3]) {
    unsafe { BANKS[0].play_at(actor, cue, point) };
}

pub(super) unsafe fn bind_caster(actor: u32, cue: u8) {
    unsafe { BANKS[0].play(actor, cue) };
}

pub(super) unsafe fn motion_accent(actor: u32, played: &mut bool, joint: u32, offset: [f32; 3]) {
    if core::mem::replace(played, true) {
        return;
    }
    unsafe {
        if let Some(matrix) = particle::joint_matrix(actor, joint) {
            let point = particle::transform_offset(&*(matrix as *const [f32; 16]), offset);
            BANKS[0].play_at(actor, 13, point);
        } else {
            BANKS[0].play(actor, 13);
        }
    }
}

pub(super) unsafe fn slash_counter(actor: u32, played: &mut bool) {
    // 8102D310 shares 812210B2's per-action latch with ordinary motion FX;
    // the following request 12 always sounds at the wrist.
    unsafe {
        motion_accent(actor, played, 8, [30., 0., -70.]);
        if let Some(point) = particle::joint_position(actor, 8) {
            BANKS[0].play_at(actor, 12, point);
        } else {
            BANKS[0].play(actor, 12);
        }
    }
}
