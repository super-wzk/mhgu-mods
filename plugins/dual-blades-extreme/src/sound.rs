use mhfz_sdk::sound::Clock;
use mhgu_sdk::style::sound::Bank;

static BANKS: [Bank; 3] = [
    Bank::new(c"sound\\se\\pl\\frontier_db\\blade"),
    Bank::new(c"sound\\se\\pl\\frontier_db\\common"),
    Bank::new(c"sound\\se\\pl\\frontier_db\\weapon"),
];
const TRACKS: &[u8] = include_bytes!("../target/assets/frontier_db_sound.fdse");

pub(super) unsafe fn preload() {
    for bank in &BANKS {
        unsafe { bank.preload() };
    }
}

pub(super) unsafe fn sharpen(actor: u32) {
    unsafe { BANKS[0].play(actor, 1) };
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
