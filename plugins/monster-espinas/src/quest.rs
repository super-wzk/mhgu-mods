//! Advertise the Espinas hunt only after the plugin registered its species.
use super::{SPECIES, u32at};

const HUB_3_NORMAL: u32 = 10308;
const QUEST: u32 = 10336;

// GU 39BB30 decodes rQuestGroup to a native new[] of trivially copyable,
// twelve-byte cQuestGroup objects. Its destructor uses the two native cookie
// words and allocator 37A628. GU persists quest flags by row position in three
// 256-byte bitsets. Append only; moving any retail row would reinterpret old saves.
#[forge::hook(offset = 0x39bb30)]
unsafe fn load_group(group: u32, stream: u32) -> u32 {
    let loaded = original!(group, stream);
    if loaded == 0 {
        return loaded;
    }
    unsafe {
        let n = u32at(group, 100) as usize;
        let old = u32at(group, 108) as *mut [u32; 3];
        if n == 0 || n >= 256 * 8 || old.is_null() {
            return loaded;
        }
        let rows = core::slice::from_raw_parts(old, n);
        let Some(source) = rows.iter().position(|r| r[1] == HUB_3_NORMAL) else {
            return loaded;
        };
        if rows.iter().any(|r| r[1] == QUEST) {
            return loaded;
        }
        let cookie = old.cast::<u32>().sub(2);
        if cookie.read() != 12 || cookie.add(1).read() != n as u32 {
            return loaded;
        }
        let base = forge::mem::text_addr();
        let alloc: unsafe extern "C" fn(usize, u32) -> *mut u32 =
            core::mem::transmute(base + 0x37a5ec);
        let free: unsafe extern "C" fn(*mut u32) = core::mem::transmute(base + 0x37a628);
        let next = alloc(8 + 12 * (n + 1), 16);
        if next.is_null() {
            return loaded;
        }
        next.write(12);
        next.add(1).write(n as u32 + 1);
        let dst = next.add(2).cast::<[u32; 3]>();
        core::ptr::copy_nonoverlapping(old, dst, n);
        dst.add(n).write([rows[source][0], QUEST, rows[source][2]]);
        ((group + 108) as *mut u32).write(dst as u32);
        ((group + 100) as *mut u32).write(n as u32 + 1);
        free(cookie);
    }
    loaded
}

// The counter calls GU's quest-unlock script by ID. Its retail script has
// no entry for the added hunt. Use an ordinary Hub 3★ quest's availability.
#[forge::hook(offset = 0x3d7e90)]
unsafe fn unlock(script: u32, result: u32, quest: u32) -> u32 {
    original!(
        script,
        result,
        if quest == QUEST { HUB_3_NORMAL } else { quest }
    )
}

// 55BDA4 is the packed species -> GUI name index adapter, separate from the
// native class/size/record index. Only this private token reaches the GMD hook.
const NAME_INDEX: u32 = 0x4000_0000 | SPECIES.packed() as u32;
#[forge::hook(offset = 0x55bda4)]
unsafe fn name_index(table: u32, raw: u32) -> u32 {
    if raw == u32::from(SPECIES.packed()) {
        NAME_INDEX
    } else {
        original!(table, raw)
    }
}
#[forge::hook(offset = 0xb84114)]
unsafe fn message(gmd: u32, index: u32) -> u32 {
    if index != NAME_INDEX {
        return unsafe { crate::materials::text(gmd, index) }
            .map_or_else(|| original!(gmd, index), |text| text.as_ptr() as u32);
    }
    let name = match unsafe { u32at(gmd, 104) } {
        0 => c"エスピナス",
        7 => c"棘龍",
        8 => c"棘龙",
        _ => c"Espinas",
    };
    name.as_ptr() as u32
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, load_group);
    forge::install_hook!(base, unlock);
    forge::install_hook!(base, name_index);
    forge::install_hook!(base, message);
}
