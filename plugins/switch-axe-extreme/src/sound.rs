use mhfz_sdk::sound::Clock;
use mhgu_sdk::style::sound::Bank;

static BANKS: [Bank; 3] = [
    Bank::new(c"sound\\se\\pl\\frontier_sa\\blade"),
    Bank::new(c"sound\\se\\pl\\frontier_sa\\common"),
    Bank::new(c"sound\\se\\pl\\frontier_sa\\weapon"),
];
const TRACKS: &[u8] = include_bytes!("../target/assets/frontier_sa_sound.flse");

pub(super) unsafe fn preload_draw() {
    // Clip 1412 first plays the weapon bank at frame 4 and the blade bank at 6.
    unsafe {
        BANKS[2].preload();
        BANKS[0].preload();
    }
}

pub(super) unsafe fn update(
    clock: &mut Clock,
    actor: u32,
    clip: Option<u16>,
    frame: f32,
    first: Option<f32>,
) {
    unsafe { clock.update(actor, &BANKS, TRACKS, clip, frame, first) };
}

pub unsafe fn sheathe(actor: u32) {
    unsafe {
        BANKS[0].play(actor, 2);
    }
}

pub(super) unsafe fn effect(actor: u32, bank: usize, cue: u8) {
    unsafe { BANKS[bank].play(actor, cue) };
}
