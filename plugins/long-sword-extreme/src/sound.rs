use mhfz_sdk::sound::Clock;
use mhgu_sdk::weapon::style::sound::Bank;

static BANKS: [Bank; 3] = [
    Bank::new(c"sound\\se\\pl\\frontier_ls\\blade"),
    Bank::new(c"sound\\se\\pl\\frontier_ls\\common"),
    Bank::new(c"sound\\se\\pl\\frontier_ls\\weapon"),
];
const TRACKS: &[u8] = include_bytes!("../target/assets/frontier_ls_sound.flse");

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

pub(super) unsafe fn blade(actor: u32, cue: u8) {
    unsafe { BANKS[0].play(actor, cue) }
}
pub(super) unsafe fn blade_at(actor: u32, cue: u8, position: [f32; 3]) {
    unsafe { BANKS[0].play_at(actor, cue, position) }
}

pub(super) unsafe fn common(actor: u32, cue: u8) {
    unsafe { BANKS[1].play(actor, cue) }
}

pub(super) unsafe fn weapon(actor: u32, cue: u8) {
    unsafe { BANKS[2].play(actor, cue) }
}

pub(super) unsafe fn release(actor: u32, tier: u8, position: [f32; 3]) {
    unsafe {
        BANKS[0].play_at(actor, 10 + tier, position);
    }
}
