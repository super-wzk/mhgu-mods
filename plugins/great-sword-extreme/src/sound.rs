//! Bank paths and cues specific to Frontier's great sword.
use mhfz_sdk::sound::Clock;
use mhgu_sdk::style::sound::Bank;

static BANKS: [Bank; 3] = [
    Bank::new(c"sound\\se\\pl\\frontier_gs\\blade"),
    Bank::new(c"sound\\se\\pl\\frontier_gs\\common"),
    Bank::new(c"sound\\se\\pl\\frontier_gs\\iron"),
];
const TRACKS: &[u8] = include_bytes!("../target/assets/frontier_gs_sound.fgse");

pub(super) unsafe fn play(actor: u32, common: bool, id: u8) {
    unsafe { BANKS[usize::from(common)].play(actor, id) };
}

pub(super) unsafe fn charge(actor: u32, cue: u8) {
    unsafe { BANKS[2].play(actor, cue) };
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
