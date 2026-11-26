use mhfz_sdk::sound::Clock;
use mhgu_sdk::weapon::style::sound::Bank;

static BANKS: [Bank; 3] = [
    Bank::new(c"sound\\se\\pl\\frontier_gl\\blade"),
    Bank::new(c"sound\\se\\pl\\frontier_gl\\common"),
    Bank::new(c"sound\\se\\pl\\frontier_gl\\weapon"),
];
const TRACKS: &[u8] = include_bytes!("../target/assets/frontier_gl_sound.flse");

pub(super) unsafe fn ignition(actor: u32) {
    // FZ 81FC9E0A: effect 41/0 uses common cue 101 for player weapon 9.
    unsafe { BANKS[1].play(actor, 101) };
}

pub(super) unsafe fn first_chamber(actor: u32) {
    // FZ 812031BA requests the weapon's chamber sound on insertion.
    unsafe { BANKS[2].play(actor, 12) };
}

pub(super) unsafe fn full_burst(actor: u32) {
    // FZ 81203B46 plays once per simultaneous group or sequential shot.
    unsafe { BANKS[2].play(actor, 13) };
}

pub(super) unsafe fn preload() {
    for bank in &BANKS {
        unsafe { bank.preload() };
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
