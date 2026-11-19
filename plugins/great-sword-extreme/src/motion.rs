use core::sync::atomic::{AtomicU32, Ordering};
use mhgu_sdk::style::PASS;

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

// Hold one owning reference per resource. The native binder 957400 takes its
// own reference and the GU actor destructor releases its sixteen entries.
const PATHS: [[&core::ffi::CStr; 3]; 2] = [
    [
        c"player\\mot\\frontier_gs\\m_0",
        c"player\\mot\\frontier_gs\\m_2",
        c"player\\mot\\frontier_gs\\m_4",
    ],
    [
        c"player\\mot\\frontier_gs\\f_0",
        c"player\\mot\\frontier_gs\\f_2",
        c"player\\mot\\frontier_gs\\f_4",
    ],
];
static BANKS: [[AtomicU32; 3]; 2] = [const { [const { AtomicU32::new(0) }; 3] }; 2];
// The game asks for roll motion 28 after it has changed the hunter's action
// (and sometimes their facing). Sample the stick at the attack's cancel,
// and retain that choice for every lookup belonging to that single roll.
static ROLL: AtomicU32 = AtomicU32::new(0);
pub(super) fn remember_roll(p: u32, side: u32) {
    ROLL.store(p | side, Ordering::Release);
}
pub(super) fn forget_roll(p: u32) {
    let old = ROLL.load(Ordering::Acquire);
    if old & !3 == p {
        let _ = ROLL.compare_exchange(old, 0, Ordering::AcqRel, Ordering::Relaxed);
    }
}
pub(super) unsafe fn roll_side(p: u32) -> u32 {
    unsafe {
        if call!(base() + 0x27e608, p, 0) == 0 {
            return 0;
        }
        // GU 27F678: world yaw = camera + stick - 0x4000. 27E638
        // compares against the *target* yaw (+5280); use the body facing at
        // the cancel (+4076) and shift the zero to stick-forward. Clip 1012
        // travels along +X, 1013 along -X.
        let origin = 0x4000u16
            .wrapping_add(at::<u16>(p, 4076))
            .wrapping_sub(at::<u16>(p, 5280));
        let edge = |n| u32::from(origin.wrapping_add(n));
        if call!(base() + 0x27e638, p, 0, edge(9102), edge(32768), 1) != 0 {
            1
        } else if call!(base() + 0x27e638, p, 0, edge(32769), edge(56434), 1) != 0 {
            2
        } else {
            0
        }
    }
}

/// FZ 811D5188 applies aim when a buffered release/counter branch executes.
/// GU resolves the stick dead zone and camera-relative world direction.
pub(super) unsafe fn aim(p: u32, branch: u16) {
    unsafe {
        if !matches!(branch, 85..=89) || call!(base() + 0x27e608, p, 0) == 0 {
            return;
        }
        let facing = at::<u16>(p, 4076);
        let target = call!(base() + 0x27f5d0, p) as u16;
        let delta = target.wrapping_sub(facing) as i16;
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

unsafe fn resource(sex: usize, group: usize, slot: u32) -> Option<u32> {
    unsafe {
        let cache = &BANKS[sex][group];
        let cached = cache.load(Ordering::Acquire);
        if cached != 0 {
            return (call!(base() + 0xb49e54, cached, slot) != 0).then_some(cached);
        }
        let mgr: u32 = at(base(), 0x2126a64);
        if mgr == 0 {
            return None;
        }
        let path = PATHS[sex][group];
        let vtable: u32 = at(mgr, 0);
        let r = call!(
            at::<u32>(vtable, 48),
            mgr,
            base() + 0x2125210,
            path.as_ptr() as u32,
            1
        );
        if r == 0 {
            return None;
        }
        if at::<u32>(r, 80) & 1 == 0 || call!(base() + 0xb49e54, r, slot) == 0 {
            call!(base() + 0x887660, r);
            return None;
        }
        Some(
            match cache.compare_exchange(0, r, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => r,
                Err(winner) => {
                    call!(base() + 0x887660, r);
                    winner
                }
            },
        )
    }
}
unsafe fn bank(player: u32, resource: u32) -> Option<u32> {
    unsafe {
        let mut empty = None;
        for i in (8..16).rev() {
            let v = at::<u32>(player, 3844 + 4 * i);
            if v == resource {
                return Some(i);
            }
            if v == 0 && empty.is_none() {
                empty = Some(i)
            }
        }
        let i = empty?;
        call!(base() + 0x957400, player, resource, i);
        Some(i)
    }
}
pub(super) unsafe extern "C" fn encode(player: u32, request: u32) -> u32 {
    unsafe {
        if (0x10000..0x20000).contains(&request) {
            return physical(player, request as u16).unwrap_or(PASS);
        }
        // Only replace the GS's drawn ground roll (GU 2CF8A4 -> 2ACFA4).
        // Let its controller spend stamina and set the invulnerability clock.
        if request == 28 && at::<u16>(player, 9478) == 1 && at::<u16>(player, 9482) == 6 {
            let saved = ROLL.load(Ordering::Acquire);
            let side = if saved & !3 == player {
                saved & 3
            } else if at::<u8>(player, 10057) & 1 != 0 {
                roll_side(player)
            } else {
                0
            };
            return match side {
                1 => physical(player, 1012),
                2 => physical(player, 1013),
                _ => None,
            }
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
        let group = match source / 100 {
            10 => 0,
            12 => 1,
            14 => 2,
            _ => return None,
        };
        let eq: u32 = at(player, 5060);
        if eq == 0 {
            return None;
        }
        let sex = at::<u8>(eq, 1240);
        let slot = u32::from(source % 100);
        if sex < 2
            && let Some(resource) = resource(usize::from(sex), group, slot)
            && let Some(i) = bank(player, resource)
        {
            return Some(i * 256 + slot);
        }
        None
    }
}
pub(super) unsafe extern "C" fn decode(player: u32, id: u32) -> u32 {
    unsafe {
        if id <= 0xfff && (id >> 8) >= 8 && at::<u32>(player, 0) == base() + 0x1749a88 {
            let r = at::<u32>(player, 3844 + 4 * (id >> 8));
            if r != 0 {
                for &(public, source) in MOTIONS {
                    let group = usize::from((source - 1000) / 200);
                    if u32::from(source % 100) == id & 255
                        && BANKS
                            .iter()
                            .any(|banks| banks[group].load(Ordering::Acquire) == r)
                    {
                        return public;
                    }
                }
                // Runtime-only clips have no stock GU alias. Give each one a
                // stable identity for the native attack volume's motion check.
                for group in 0..3 {
                    if BANKS
                        .iter()
                        .any(|sex| sex[group].load(Ordering::Acquire) == r)
                    {
                        return 31000 + group as u32 * 200 + (id & 255);
                    }
                }
            }
        }
        PASS
    }
}

pub(super) unsafe fn source_clip(p: u32, packed: u16) -> Option<u16> {
    let slot = packed >> 8;
    if !(8..16).contains(&slot) {
        return None;
    }
    let res: u32 = unsafe { at(p, 3844 + 4 * u32::from(slot)) };
    if res == 0 {
        return None;
    }
    (0..3)
        .find(|&i| {
            BANKS
                .iter()
                .any(|sex| sex[i].load(Ordering::Acquire) == res)
        })
        .map(|i| 1000 + 200 * i as u16 + (packed & 255))
}

#[forge::hook(offset = 0x308834)]
unsafe fn equipment_grip(gear: u32, player: u32, weapon: u32) -> u32 {
    unsafe {
        if weapon == 0 && crate::battle::local(player) && at::<f32>(player, 1268) > 0. {
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

pub(super) fn install() {
    forge::install_hook!(base(), equipment_grip);
}

pub(super) unsafe fn play(player: u32, source: u16, start: u16, blend: u16) -> Option<u16> {
    unsafe {
        mhgu_sdk::style::action::play_motion(
            player,
            0x10000 | u32::from(source),
            f32::from(start),
            f32::from(blend / 2),
        )
    }
}
