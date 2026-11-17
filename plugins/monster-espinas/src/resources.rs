//! GU 1.4's em001 actor binder uses numbered resources from a read-only table.
//! Scope its private lookups to the actor being bound, including nested loads.
use super::{Actors, SPECIES, u16at, u32at};
use core::sync::atomic::{AtomicUsize, Ordering};

pub(super) const FIREBALL: u32 = hash(br"shell\em\em107_00_shell00\em107_00_00");

const fn hash(path: &[u8]) -> u32 {
    let mut h = u32::MAX;
    let mut i = 0;
    while i < path.len() {
        h ^= path[i] as u32;
        let mut j = 0;
        while j < 8 {
            h = (h >> 1) ^ (0xedb88320 & 0u32.wrapping_sub(h & 1));
            j += 1;
        }
        i += 1;
    }
    h
}
macro_rules! path {
    ($p:literal) => {
        hash($p.as_bytes())
    };
}
// Only private resources from the template's binding row at 1732A68.
// Sound bank dependencies resolve by path inside the private request resource.
fn private_hash(number: u32) -> Option<u32> {
    Some(match number {
        30665 | 30667 => path!(r"enemy\em107\em107_00\mod\em107_00"),
        30666 => path!(r"enemy\em107\em107_00\mod\em107_00_tail"),
        30668 => path!(r"enemy\em107\em107_00\mot\em107_00_0"),
        30669 => path!(r"enemy\em107\em107_00\mot\em107_00_1"),
        30670 => path!(r"enemy\em107\em107_00\mot\em107_00_2"),
        30671 => path!(r"enemy\em107\em107_00\mot\em107_00_3"),
        30672 => path!(r"enemy\em107\em107_00\mot\em107_00_4"),
        30673 => path!(r"enemy\em107\em107_00\mot\em107_00_cam"),
        30674 => path!(r"sound\call\em\em107"),
        30675 => path!(r"sound\se\em\em107_00\snd_em107_00"),
        30676 => path!(r"effect\pel\em\em107_00c"),
        30677 => path!(r"effect\pel\em\em107_00u"),
        30678 => path!(r"effect\psl\enemy\em107\em107_00\mot\em107_00_0"),
        30679 => path!(r"effect\psl\enemy\em107\em107_00\mot\em107_00_1"),
        30680 => path!(r"effect\psl\enemy\em107\em107_00\mot\em107_00_2"),
        30681 => path!(r"effect\psl\enemy\em107\em107_00\mot\em107_00_3"),
        30682 => path!(r"effect\psl\enemy\em107\em107_00\mot\em107_00_4"),
        30683 => path!(r"sound\pts\em\em107_00c"),
        30684 => path!(r"sound\pts\em\em107_00u"),
        30685 => path!(r"arc\shell\em\em107_00_shell"),
        30686 => path!(r"enemy\hit_data\em107_00_attackdata"),
        30687 => path!(r"enemy\hit_data\em107_00_hitsize"),
        30688 => path!(r"enemy\body_data\em107_00_bodydata"),
        30689 => path!(r"enemy\cam_body_data\em107_00_cambodydata"),
        30690 => path!(r"enemy\dt_base\em107_00_dtbase"),
        30691 => path!(r"enemy\dt_tune\em107_00_dttune"),
        30692 => path!(r"enemy\dt_base\em107_00_dtbparts"),
        30693 => path!(r"enemy\cmd_tbl\em107_00_cmdtbl"),
        30694 => path!(r"enemy\dou_mou_ka\em107_00_doumouka"),
        30695 => path!(r"enemy\dou_mou_ka\em107_00_doumouka_G"),
        30696 => path!(r"enemy\area_link\em107_00"),
        30697 => path!(r"enemy\area_eat\em107_00"),
        30698 => path!(r"enemy\map_info\em107_00"),
        30699 => path!(r"enemy\area_select\em107_00"),
        30700 => path!(r"enemy\area_patrol\em107_00"),
        30701 => path!(r"enemy\em_option\mot\em107_00_option"),
        34516 => path!(r"enemy\hagi\hagi_s0_em107_00"),
        34517 => path!(r"enemy\hagi\hagi_s1_em107_00"),
        34518 => path!(r"enemy\hagi\hagi_s2_em107_00"),
        34519 => path!(r"enemy\hagi\hagi_s3_em107_00"),
        34520 => path!(r"enemy\hagi\hagi_s4_em107_00"),
        _ => return None,
    })
}
static BINDING: Actors<()> = Actors::new();
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
fn thread() -> u32 {
    let current: unsafe extern "C" fn() -> u32 =
        unsafe { core::mem::transmute(forge::mem::text_addr() + 0x13effd4) };
    unsafe { current() }
}

#[forge::hook(offset = 0x4a858)]
unsafe fn bind(loader: u32) -> u32 {
    let custom = unsafe { u16at(loader, 4) } == SPECIES.packed();
    if !custom && ACTIVE.load(Ordering::Acquire) == 0 {
        return original!(loader);
    }
    let key = thread();
    let previous = BINDING.with(|map| map.remove(&key).is_some());
    if custom {
        BINDING.with(|map| map.insert(key, ()));
        ACTIVE.fetch_add(1, Ordering::AcqRel);
    }
    let result = original!(loader);
    BINDING.with(|map| {
        if previous {
            map.insert(key, ());
        } else {
            map.remove(&key);
        }
    });
    if custom {
        ACTIVE.fetch_sub(1, Ordering::AcqRel);
    }
    result
}

/// Same owning lookup and ABI as 152960, using a private ARC key.
unsafe fn find(hash: u32, kind: u32) -> u32 {
    let base = forge::mem::text_addr();
    unsafe {
        let manager = u32at(base, 0x2126a64);
        if manager == 0 {
            return 0;
        }
        let method = u32at(u32at(manager, 0), 56);
        let lookup: unsafe extern "C" fn(u32, u32, u32, u32) -> u32 = core::mem::transmute(method);
        lookup(manager, method, hash, kind)
    }
}
#[forge::hook(offset = 0x152960)]
unsafe fn numbered(manager: u32, number: u32) -> u32 {
    if ACTIVE.load(Ordering::Acquire) != 0
        && let Some(hash) = private_hash(number)
    {
        let thread = thread();
        if BINDING.with(|map| map.contains_key(&thread)) {
            let kind = unsafe { u32at(forge::mem::text_addr(), 0x15a0ce4 + 8 * number) };
            return unsafe { find(hash, kind) };
        }
    }
    original!(manager, number)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, bind);
    forge::install_hook!(base, numbered);
}
