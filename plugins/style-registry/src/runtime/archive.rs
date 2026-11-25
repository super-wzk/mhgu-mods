//! Mount private resource archives before a game's path-based resource lookup.
use core::{
    ffi::{CStr, c_char},
    sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed},
};

const MISSING: u32 = u32::MAX;
const COMMON: &CStr = c"arc\\frontier\\common";
const WEAPONS: &[(&[u8], &CStr)] = &[
    (b"frontier_gs", c"arc\\frontier\\gs"),
    (b"frontier_db", c"arc\\frontier\\db"),
    (b"frontier_ls", c"arc\\frontier\\ls"),
    (b"frontier_sa", c"arc\\frontier\\sa"),
    (b"frontier_sns", c"arc\\frontier\\sns"),
    (b"frontier_gl", c"arc\\frontier\\gl"),
];
static COMMON_ROOT: AtomicU32 = AtomicU32::new(0);
static WEAPON_ROOTS: [AtomicU32; WEAPONS.len()] = [const { AtomicU32::new(0) }; WEAPONS.len()];
static LOADING: AtomicBool = AtomicBool::new(false);

fn mount(
    load: impl FnOnce(u32, usize, *const c_char, u32) -> u32,
    manager: u32,
    path: &CStr,
    root: &AtomicU32,
) -> bool {
    match root.load(Relaxed) {
        0 => {}
        MISSING => return false,
        _ => return true,
    }
    // Hold the ARC reference so its members remain searchable by their path keys.
    let resource = load(
        manager,
        (forge::mem::text_addr() + 0x2124df0) as usize,
        path.as_ptr(),
        1,
    );
    root.store(if resource == 0 { MISSING } else { resource }, Relaxed);
    resource != 0
}

#[forge::hook(offset = 0xbcad18)]
unsafe fn load_resource(manager: u32, kind: usize, path: *const c_char, flags: u32) -> u32 {
    unsafe {
        if !path.is_null() && !LOADING.load(Relaxed) {
            let path_bytes = CStr::from_ptr(path).to_bytes();
            let mut parts = path_bytes.split(|&byte| byte == b'\\').rev();
            parts.next();
            if let Some(index) =
                parts.find_map(|part| WEAPONS.iter().position(|&(namespace, _)| part == namespace))
                && WEAPON_ROOTS[index].load(Relaxed) == 0
                && COMMON_ROOT.load(Relaxed) != MISSING
                && mhgu_sdk::service::on_game_thread()
            {
                LOADING.store(true, Relaxed);
                let load = |manager, kind, name, mode| original!(manager, kind, name, mode);
                if mount(load, manager, COMMON, &COMMON_ROOT) {
                    mount(load, manager, WEAPONS[index].1, &WEAPON_ROOTS[index]);
                }
                LOADING.store(false, Relaxed);
            }
        }
        original!(manager, kind, path, flags)
    }
}

#[forge::hook(offset = 0xb39f58)]
unsafe fn read_archive(resource: u32, stream: u32) -> u32 {
    unsafe {
        let name = CStr::from_ptr((resource + 8) as *const c_char);
        if name == COMMON || WEAPONS.iter().any(|&(_, path)| name == path) {
            // rArchive::load normally starts concurrent child requests.
            // Our directory is ordered by dependency, so use its sequential path.
            let flags = (resource + 77) as *mut u8;
            let original_flags = flags.read();
            flags.write(original_flags | 2);
            let result = original!(resource, stream);
            flags.write((flags.read() & !2) | (original_flags & 2));
            result
        } else {
            original!(resource, stream)
        }
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, read_archive);
    forge::install_hook!(base, load_resource);
}
