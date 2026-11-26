use super::{Save, Styles, VERSION};
use crate::style;
use alloc::{ffi::CString, format, vec, vec::Vec};
use core::ffi::CStr;
use forge::{
    fs::{file_exists, savedata},
    log::prelude::error,
    mt::{
        fs::{MtFile, OpenMode},
        object::Object,
    },
};
use mhgu_sdk::sync::Shared;

// The schema has 15 weapons and at most 40 equipment combinations.
const LIMIT: usize = 16 * 1024;

struct Mount;
impl Mount {
    fn open() -> Result<Self, ()> {
        if savedata::mount() { Ok(Self) } else { Err(()) }
    }
}
impl Drop for Mount {
    fn drop(&mut self) {
        savedata::unmount()
    }
}

fn path(slot: u8, parity: u32) -> CString {
    CString::new(format!(
        "{}:/mhgu-sdk-{slot}-{parity}.json",
        savedata::mount_point()
    ))
    .unwrap()
}

fn read_file(path: &CStr) -> Result<Option<Vec<u8>>, ()> {
    if !file_exists(path.to_str().map_err(|_| ())?) {
        return Ok(None);
    }
    let file = MtFile::open(path, OpenMode::Read).map_err(|_| ())?;
    let size = file.size();
    let out = if size <= LIMIT {
        let mut data = vec![0; size];
        (file.read(&mut data) == size).then_some(data).ok_or(())
    } else {
        Err(())
    };
    file.close();
    file.destroy();
    out.map(Some)
}
fn load(slot: u8) -> Result<(u32, Styles), ()> {
    let _mount = Mount::open()?;
    let mut newest = None;
    let mut present = false;
    for parity in 0..2 {
        let file = match read_file(&path(slot, parity)) {
            Ok(None) => continue,
            Ok(Some(bytes)) => Save::decode(&bytes, slot, parity),
            Err(()) => None,
        };
        present = true;
        if let Some(file) = file
            && newest
                .as_ref()
                .is_none_or(|old: &Save| file.generation > old.generation)
        {
            newest = Some(file);
        }
    }
    if present {
        newest.map(|file| (file.generation, file.data)).ok_or(())
    } else {
        Ok((0, Styles::default()))
    }
}

struct Session {
    slot: u8,
    committed: Option<(u32, Styles)>,
}

static SESSION: Shared<Option<Session>> = Shared::new(None);
static PENDING: Shared<Option<(u8, Option<Save>)>> = Shared::new(None);

fn select(slot: u8) {
    PENDING.with(|pending| *pending = None);
    let committed = load(slot).ok();
    if committed.is_none() {
        error!("mhgu-sdk: character data unavailable; preserving sidecars");
    }
    style::load_character(
        committed
            .as_ref()
            .map(|(_, data)| data.clone())
            .unwrap_or_default(),
    );
    SESSION.with(|session| *session = Some(Session { slot, committed }));
}

fn create(slot: u8) {
    let data = Styles::default();
    style::load_character(data.clone());
    PENDING.with(|pending| *pending = None);
    // A newly created hunter may replace another hunter in the same GU slot.
    // Replace both SDK generations so fallback cannot restore the prior hunter.
    let committed = load(slot).ok().and_then(|(mut generation, old)| {
        if generation != 0 || old != data {
            for _ in 0..2 {
                let file = Save {
                    version: VERSION,
                    slot,
                    generation: generation.checked_add(1)?,
                    data: data.clone(),
                };
                let bytes = serde_json::to_vec(&file).ok()?;
                write_file(slot, file.generation, &bytes).ok()?;
                generation = file.generation;
            }
        }
        Some((generation, data))
    });
    if committed.is_none() {
        error!("mhgu-sdk: could not initialize the new character's data");
    }
    SESSION.with(|session| *session = Some(Session { slot, committed }));
}

fn begin(slot: u8) {
    let snapshot = style::snapshot();
    let file = SESSION.with(|session| {
        let state = session.as_ref().filter(|state| state.slot == slot)?;
        let (generation, data) = state.committed.as_ref()?;
        if *data == snapshot {
            return None;
        }
        Some(Save {
            version: VERSION,
            slot,
            generation: generation.checked_add(1)?,
            data: snapshot,
        })
    });
    PENDING.with(|pending| *pending = Some((slot, file)));
}

fn finish(success: bool) {
    let Some((slot, file)) = PENDING.with(Option::take) else {
        return;
    };
    if !success {
        return;
    }
    if SESSION.with(|session| session.is_none()) {
        // The first save of a new character can precede the normal load callback.
        select(slot);
        return;
    }
    let Some(file) = file else { return };
    let written = serde_json::to_vec(&file)
        .ok()
        .is_some_and(|bytes| write_file(slot, file.generation, &bytes).is_ok());
    if !written {
        error!("mhgu-sdk: failed to save character data");
        return;
    }
    SESSION.with(|session| {
        if let Some(state) = session.as_mut().filter(|state| state.slot == slot) {
            state.committed = Some((file.generation, file.data));
        }
    });
}

unsafe fn read<T: Copy>(p: u32, offset: u32) -> T {
    unsafe { ((p + offset) as *const T).read_unaligned() }
}

// The ordinary GU load path selects one character. Inner serializers also run for previews.
#[forge::hook(offset = 0x3e21b8)]
unsafe fn select_character(manager: u32) -> u32 {
    let before = unsafe { read::<u32>(manager, 60) };
    let out = original!(manager);
    let slot = unsafe { read::<u32>(manager, 64) };
    if before == 14 && unsafe { read::<u32>(manager, 60) } == 17 && slot < 3 {
        select(slot as u8);
    }
    out
}

// 3DFAFC chooses the slot for a newly created hunter. State 6 -> 9
// with +108 set is reached after the game's new-character write succeeds.
#[forge::hook(offset = 0x3dfafc)]
unsafe fn create_character(manager: u32) -> u32 {
    let before = unsafe { read::<u32>(manager, 60) };
    let out = original!(manager);
    if before == 6
        && unsafe { read::<u32>(manager, 60) } == 9
        && unsafe { read::<u8>(manager, 108) } == 1
    {
        let slot = unsafe { read::<u32>(manager, 64) };
        if slot < 3 {
            create(slot as u8);
        }
    }
    out
}

#[forge::hook(offset = 0x3e0980)]
unsafe fn save_slot(manager: u32, slot: u32) -> u32 {
    let out = original!(manager, slot);
    if out == 1 && slot < 3 {
        begin(slot as u8);
    }
    out
}

#[forge::hook(offset = 0x3e1308)]
unsafe fn save_progress(manager: u32) -> u32 {
    let before = unsafe { read::<u32>(manager, 60) };
    let out = original!(manager);
    if before == 21 {
        match unsafe { read::<u32>(manager, 60) } {
            24 => finish(unsafe { read::<u8>(manager, 112) } != 0),
            22 => finish(false),
            _ => {}
        }
    }
    out
}

pub(crate) fn install(base: u32) {
    forge::install_hook!(base, select_character);
    forge::install_hook!(base, create_character);
    forge::install_hook!(base, save_slot);
    forge::install_hook!(base, save_progress);
}

fn write_file(slot: u8, generation: u32, bytes: &[u8]) -> Result<(), ()> {
    let _mount = Mount::open()?;
    let path = path(slot, generation % 2);
    let file = MtFile::open(&path, OpenMode::Write).map_err(|_| ())?;
    let written = file.write(bytes);
    file.close();
    file.destroy();
    if written != bytes.len() || read_file(&path)?.as_deref() != Some(bytes) {
        return Err(());
    }
    Ok(())
}
