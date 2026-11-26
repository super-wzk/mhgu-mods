use mhfz_sdk::sound::Clock;
use mhgu_sdk::weapon::style::{particle, sound::Bank};

static BANKS: [Bank; 2] = [
    Bank::new(c"sound\\se\\pl\\frontier_ms\\blade"),
    Bank::new(c"sound\\se\\pl\\frontier_ms\\common"),
];
const TRACKS: &[u8] = include_bytes!("../target/assets/frontier_ms_sound.flse");

pub(super) unsafe fn preload() {
    for bank in &BANKS {
        unsafe { bank.preload() };
    }
}

pub(super) unsafe fn sample(clock: &mut Clock, actor: u32, clip: u16, frame: f32, start: bool) {
    unsafe {
        clock.update(
            actor,
            &BANKS,
            TRACKS,
            Some(clip),
            frame,
            start.then_some(frame),
        )
    };
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

pub(super) unsafe fn slash_counter(actor: u32) {
    // 8122119A layers request 13 before 8102D32E plays request 12.
    // Frontier node 14 is GU logical joint 8.
    unsafe {
        if let Some(matrix) = particle::joint_matrix(actor, 8) {
            let matrix = &*(matrix as *const [f32; 16]);
            let accent = particle::transform_offset(matrix, [30., 0., -70.]);
            BANKS[0].play_at(actor, 13, accent);
            BANKS[0].play_at(actor, 12, [matrix[12], matrix[13], matrix[14]]);
        } else {
            BANKS[0].play(actor, 13);
            BANKS[0].play(actor, 12);
        }
    }
}
