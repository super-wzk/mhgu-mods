use mhfz_sdk::motion::{DrawnRoll, MotionBanks, sprint_sheathe};
use mhgu_sdk::weapon::style::{PASS, movement::command_stick_yaw};

fn base() -> u32 {
    forge::mem::text_addr()
}
unsafe fn at<T: Copy>(p: u32, offset: u32) -> T {
    unsafe { ((p + offset) as *const T).read_unaligned() }
}
macro_rules! call {
    ($addr:expr, $($x:expr),* $(,)?) => {{
        let f:unsafe extern "C" fn($(call!(@arg $x)),*)->u32=core::mem::transmute(($addr) as usize);
        f($($x),*)
    }};
    (@arg $x:expr)=>{u32};
}

// FZ common action 202, weapon row in 825901F4.
const SPRINT_SHEATHE: u16 = 1032;
pub(super) static SHEATHE_MOTIONS: mhgu_sdk::weapon::style::remote::Sheathe =
    mhfz_sdk::motion::sheathe_motions(&[crate::battle::DASH], SPRINT_SHEATHE);

static BANKS: MotionBanks = MotionBanks::new([
    c"player\\mot\\frontier_gs\\0",
    c"player\\mot\\frontier_gs\\2",
    c"player\\mot\\frontier_gs\\4",
]);

pub(super) unsafe extern "C" fn preload(_actor: u32) {
    unsafe {
        BANKS.preload_for_hunt(1410);
        crate::combat::preload();
        crate::sound::preload();
        crate::fx::preload();
    }
}

pub(super) static ROLL: DrawnRoll = DrawnRoll::new(1012, 1013);

/// FZ 811D5188 applies aim when a buffered release/counter branch executes.
/// GU resolves the stick dead zone and camera-relative world direction.
pub(super) unsafe fn aim(p: u32, branch: u16) {
    unsafe {
        if !matches!(branch, 85..=89) || call!(base() + 0x27e608, p, 0) == 0 {
            return;
        }
        let facing = at::<u16>(p, 4076);
        let target = call!(base() + 0x27f5d0, p) as u16;
        let delta = command_stick_yaw(p).unwrap_or(0).wrapping_neg() as i16;
        let yaw = match branch {
            87 | 88 => target,
            // The uppercut uses the left/right sectors (8172C3D4).
            86 if !(0x2000..=0x6000).contains(&delta.unsigned_abs()) => return,
            _ if delta == 0 || delta == i16::MIN => return,
            _ => facing.wrapping_add(if delta > 0 { 0x1000 } else { 0xf000 }),
        };
        ((p + 4076) as *mut u32).write(u32::from(yaw));
        ((p + 5280) as *mut u32).write(u32::from(yaw));
    }
}

// Only shared locomotion/guard actions use GU public motion IDs. Weapon moves
// are selected by the style's own action program and play their source directly.
const MOTIONS: &[(u32, u16)] = &[
    (5001, 1001), // drawn idle
    (5002, 1002), // draw with retail equipment sockets
    (5003, 1003), // sheathe
    (5004, 1004), // drawn walk
    (5005, 1473), // raised guard, w00 state 65
    (5012, 1012), // drawn lateral evades
    (5013, 1013),
];

pub(super) unsafe extern "C" fn encode(player: u32, request: u32) -> u32 {
    unsafe {
        if (0x10000..0x20000).contains(&request) {
            return physical(player, request as u16).unwrap_or(PASS);
        }
        if sprint_sheathe(player, request, &SHEATHE_MOTIONS) {
            return physical(player, SPRINT_SHEATHE).unwrap_or(PASS);
        }
        // Only replace the GS's drawn ground roll (GU 2CF8A4 -> 2ACFA4).
        // Let its controller spend stamina and set the invulnerability clock.
        if request == 28 {
            return ROLL
                .clip(player, || {
                    (at::<u8>(player, 10057) & 1 != 0)
                        .then(|| command_stick_yaw(player))
                        .flatten()
                })
                .and_then(|clip| physical(player, clip))
                .unwrap_or(PASS);
        }
        let Some(&(_, source)) = MOTIONS.iter().find(|&&(public, _)| public == request) else {
            return PASS;
        };
        physical(player, source).unwrap_or(PASS)
    }
}
pub(super) unsafe fn physical(player: u32, source: u16) -> Option<u32> {
    unsafe {
        if at::<u32>(player, 0) != base() + 0x1749a88 {
            return None;
        }
        BANKS.encode(player, source).map(u32::from)
    }
}

pub(super) unsafe extern "C" fn decode(player: u32, id: u32) -> u32 {
    let Some(source) = u16::try_from(id).ok().and_then(|packed| unsafe {
        (at::<u32>(player, 0) == base() + 0x1749a88)
            .then(|| source_clip(player, packed))
            .flatten()
    }) else {
        return PASS;
    };
    if source == SPRINT_SHEATHE {
        return 5003;
    }
    MOTIONS
        .iter()
        .find(|&&(_, clip)| clip == source)
        .map_or(31000 + u32::from(source - 1000), |&(public, _)| public)
}

pub(super) unsafe fn source_clip(actor: u32, packed: u16) -> Option<u16> {
    unsafe { BANKS.decode(actor, packed) }
}

#[forge::hook(offset = 0x308834)]
unsafe fn equipment_grip(gear: u32, player: u32, weapon: u32) -> u32 {
    unsafe {
        if player != 0
            && weapon == 0
            && at::<f32>(player, 1268) > 0.
            && (crate::battle::local(player) || crate::battle::peer_action(player))
        {
            // FZ 819070E2 keeps these grips until its interpolation clock
            // (+516) finishes. GU's matching clock is +1268; LMT events take
            // over afterwards, including draws started partway into a clip.
            match source_clip(player, at(player, 1204)) {
                Some(1410 | 1447 | 1448) => return 2,
                Some(1432 | 1434..=1436) => return 1,
                _ => {}
            }
        }
    }
    original!(gear, player, weapon)
}

// GU's great-sword model controller chooses channels 9..12 from the stock
// charge predicate (virtual +1684) and tier (+2024). Custom actions use
// Frontier's charge clock. Keep the native model-specific visibility rules.
#[forge::hook(offset = 0x30a8b0)]
unsafe fn charge_channels(
    part: u32,
    kind: u32,
    actor: u32,
    drawn: *mut u8,
    folded: *mut u8,
    idle: *mut u8,
    first: *mut u8,
    second: *mut u8,
    third: *mut u8,
) -> u32 {
    let result = original!(part, kind, actor, drawn, folded, idle, first, second, third);
    unsafe {
        if *drawn == 1
            && *idle == 1
            && let Some(level @ 1..=4) = crate::battle::model_charge(actor)
        {
            *idle = 0;
            // The native display has three charged channels.
            match level {
                1 => *first = 1,
                2 => *second = 1,
                _ => *third = 1,
            }
        }
    }
    result
}

pub(super) fn install() {
    forge::install_hook!(base(), equipment_grip);
    forge::install_hook!(base(), charge_channels);
}

pub(super) unsafe fn play(player: u32, source: u16, start: u16, blend: u16) -> Option<u16> {
    unsafe { mhfz_sdk::motion::play_step(player, 0x10000 | u32::from(source), start, blend) }
}
