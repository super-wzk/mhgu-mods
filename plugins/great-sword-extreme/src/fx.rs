//! Event requests for converted Frontier EFL resources. GU owns emission,
//! particle curves, attachment, rendering and destruction.
use core::sync::atomic::{AtomicU32, Ordering};
mod charge;
mod depth;
pub(super) use charge::ChargeEffect;

const ROUTES: &[u8] = include_bytes!("../target/assets/effects/routes.bin");
const CHARGE_ROUTE: usize = 9;
static PEL: AtomicU32 = AtomicU32::new(0);
static CHARGE_UNITS: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];

unsafe fn field<T: Copy>(p: u32, off: u32) -> T {
    unsafe { ((p + off) as *const T).read_unaligned() }
}
unsafe fn put<T>(p: u32, off: u32, v: T) {
    unsafe { ((p + off) as *mut T).write_unaligned(v) }
}
macro_rules! gu {
    ($a:expr $(,$x:expr)* $(,)?)=>{{
        let f:unsafe extern "C" fn($(gu!(@u $x)),*)->u32=core::mem::transmute($a as usize);f($($x),*)
    }};
    (@u $x:expr)=>{u32};
}
unsafe fn resources(base: u32) -> u32 {
    unsafe {
        let cached = PEL.load(Ordering::Acquire);
        if cached != 0 {
            return cached;
        }
        let mgr = field::<u32>(base, 0x2126a64);
        if mgr == 0 {
            return 0;
        }
        let res = gu!(
            field::<u32>(field(mgr, 0), 48),
            mgr,
            base + 0x1889fdc,
            c"effect\\frontier_gs\\particles".as_ptr() as u32,
            1
        );
        if res == 0 {
            return 0;
        }
        if field::<u32>(res, 80) & 1 == 0
            || PEL
                .compare_exchange(0, res, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            gu!(base + 0x887660, res);
        }
        PEL.load(Ordering::Acquire)
    }
}
unsafe fn anchor(p: u32, joint: Option<u8>) -> [f32; 3] {
    unsafe {
        if let Some(j) = joint
            && field::<u32>(p, 1172) != 0
            && field::<u32>(p, 1176) != 0
        {
            let m = gu!(forge::mem::text_addr() + 0xc1628, p, u32::from(j));
            if m != 0 {
                return field(m, 48);
            }
        }
        field(p, 64)
    }
}
enum Attachment {
    Burst(Option<u8>),
    World([f32; 3], u16),
    Follow(u8),
}
unsafe fn route(p: u32, route: usize, attachment: Attachment) {
    unsafe {
        let Some(&[first_lo, first_hi, count_lo, count_hi]) = ROUTES.as_chunks::<4>().0.get(route)
        else {
            return;
        };
        let first = u16::from_le_bytes([first_lo, first_hi]);
        let count = u16::from_le_bytes([count_lo, count_hi]);
        let base = forge::mem::text_addr();
        let manager = field::<u32>(base, 0x188a0d4);
        if manager == 0 || field::<u32>(base, 0x2126f48) == 0 {
            return;
        }
        let pel = resources(base);
        if pel == 0 {
            return;
        }
        let handle = gu!(base + 0x3268d8, manager, pel, 0);
        if handle == 0 || field::<u32>(handle, 4) == u32::MAX {
            return;
        }
        let world = match attachment {
            Attachment::Burst(joint) => Some((anchor(p, joint), field(p, 4076))),
            Attachment::World(position, yaw) => Some((position, yaw)),
            Attachment::Follow(_) => None,
        };
        for n in 0..count {
            let unit = gu!(base + 0x3288c4, 416, 16);
            if unit == 0 {
                continue;
            }
            gu!(base + 0x328900, unit);
            let param = unit + 128;
            if let Some((position, yaw)) = world {
                // Burst effects stay at their emission position, like FZ.
                put(param, 4, 0xau32);
                put(param, 12, 1u32);
                put(param, 70, 3u8);
                put(param, 176, position);
                put(
                    param,
                    32,
                    [0., f32::from(yaw) * 360. / 65536., 0.],
                );
            } else if let Attachment::Follow(joint) = attachment {
                put(param, 4, 0x28u32);
                put(param, 12, 2u32);
                put(param, 66, i16::from(joint));
                put(param, 70, 0u8);
                put(param, 192, gu!(field::<u32>(field(p, 0), 304), p));
            }
            if gu!(
                base + 0x3267d0,
                manager,
                unit,
                field::<u32>(handle, 4),
                1,
                u32::from(first + n),
                0
            ) == 0
            {
                gu!(field::<u32>(field(unit, 0), 4), unit);
            } else if route == CHARGE_ROUTE
                && let Some(slot) = CHARGE_UNITS.get(usize::from(n))
            {
                slot.store(unit, Ordering::Release);
            }
        }
        gu!(base + 0x326c78, manager, handle, 0);
    }
}
pub(super) unsafe fn emit(p: u32, id: u16, joint: Option<u8>) {
    // Bank 4006 occupies routes 0 through 6, in selector order.
    if id <= 6 {
        unsafe { route(p, usize::from(id), Attachment::Burst(joint)) }
    }
}
pub(super) unsafe fn radiance(p: u32, position: [f32; 3], yaw: u16) {
    unsafe { route(p, 7, Attachment::World(position, yaw)) }
}
pub(super) unsafe fn radiance_ready(p: u32) {
    // Frontier binds 4022:21 to hunter node 18, retargeted to GU joint 12.
    unsafe { route(p, 8, Attachment::Follow(12)) }
}
fn stop_charge() {
    let base = forge::mem::text_addr();
    for slot in &CHARGE_UNITS {
        let unit = slot.swap(0, Ordering::AcqRel);
        if unit != 0 {
            unsafe {
                // End existing EFL children and cancel pending emission. GU's
                // proof destructor lets already emitted particles finish.
                gu!(base + 0x32a400, unit, 1);
            }
        }
    }
}
// Resource release is shared by normal completion, scene teardown and both
// destructors. Forget owned handles before GU can recycle their allocation.
#[forge::hook(offset = 0x32a104)]
unsafe fn release(unit: u32) -> u32 {
    for slot in &crate::fx::CHARGE_UNITS {
        let _ = slot.compare_exchange(unit, 0, Ordering::AcqRel, Ordering::Acquire);
    }
    original!(unit)
}


pub(super) fn install() {
    let base = forge::mem::text_addr();
    forge::install_hook!(base, release);
    depth::install();
}
