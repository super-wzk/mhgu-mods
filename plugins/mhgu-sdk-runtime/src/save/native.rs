use super::{CHARACTER, CharacterData, Save, VERSION};
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
use serde::{Deserialize, Serialize};

// Includes three usage counters per registered u16 weapon type.
const LIMIT: usize = 2 * 1024 * 1024;

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

fn journal_path(slot: u8) -> CString {
    CString::new(format!(
        "{}:/mhgu-sdk-{slot}-trade.json",
        savedata::mount_point()
    ))
    .unwrap()
}

#[derive(Deserialize, Serialize)]
struct Journal<T> {
    native: u64,
    save: T,
}

fn read_journal(slot: u8) -> Option<Journal<Save>> {
    let bytes = read_file(&journal_path(slot)).ok()??;
    let journal: Journal<Save> = serde_json::from_slice(&bytes).ok()?;
    journal.save.valid(slot).then_some(journal)
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
fn load(slot: u8, manager: Option<u32>) -> Result<(u32, CharacterData, bool), ()> {
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
    if let Some(journal) = read_journal(slot)
        && newest
            .as_ref()
            .is_none_or(|file| journal.save.generation > file.generation)
        && manager.and_then(|manager| unsafe { fingerprint(manager, slot) }) == Some(journal.native)
    {
        return Ok((journal.save.generation, journal.save.data, true));
    }
    if present {
        newest
            .map(|file| (file.generation, file.data, false))
            .ok_or(())
    } else {
        Ok((0, CharacterData::default(), false))
    }
}

struct Session {
    slot: u8,
    committed: Option<(u32, CharacterData)>,
    traded: bool,
    recovered: bool,
}

static SESSION: Shared<Option<Session>> = Shared::new(None);
static PENDING: Shared<Option<(u8, Option<Save>)>> = Shared::new(None);
static STAGING: Shared<Option<u32>> = Shared::new(None);

pub(crate) fn mark_trade() {
    SESSION.with(|session| {
        if let Some(state) = session {
            state.traded = true;
        }
    });
}

pub(crate) fn character_loaded() -> bool {
    SESSION.with(|session| {
        session
            .as_ref()
            .is_some_and(|state| state.committed.is_some())
    })
}

fn select(slot: u8, manager: Option<u32>) {
    PENDING.with(|pending| *pending = None);
    let loaded = load(slot, manager).ok();
    let recovered = loaded.as_ref().is_some_and(|(_, _, recovered)| *recovered);
    let committed = loaded.map(|(generation, data, _)| (generation, data));
    if committed.is_none() {
        error!("mhgu-sdk: character data unavailable; preserving sidecars");
    }
    CHARACTER.with(|data| {
        *data = committed
            .as_ref()
            .map(|(_, data)| data.clone())
            .unwrap_or_default();
    });
    SESSION.with(|session| {
        *session = Some(Session {
            slot,
            committed,
            traded: false,
            recovered,
        })
    });
}

fn create(slot: u8) {
    let data = CharacterData::default();
    CHARACTER.with(|current| *current = data.clone());
    PENDING.with(|pending| *pending = None);
    // A newly created hunter may replace another hunter in the same GU slot.
    // Replace both SDK generations so fallback cannot restore the prior hunter.
    let old_journal = Mount::open().ok().and_then(|_mount| read_journal(slot));
    let committed = load(slot, None).ok().and_then(|(mut generation, old, _)| {
        if let Some(journal) = old_journal {
            generation = generation.max(journal.save.generation);
        }
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
    SESSION.with(|session| {
        *session = Some(Session {
            slot,
            committed,
            traded: false,
            recovered: false,
        })
    });
}

fn persist(file: &Save) -> bool {
    serde_json::to_vec(file)
        .ok()
        .is_some_and(|bytes| write_file(file.slot, file.generation, &bytes).is_ok())
}

// Keep the previous journal until its generation is in A/B. Otherwise a
// second save could overwrite the recovery copy before either save is durable.
fn seal_recovered(slot: u8) -> bool {
    SESSION.with(|session| {
        let Some(state) = session
            .as_mut()
            .filter(|state| state.slot == slot && state.recovered)
        else {
            return true;
        };
        let (generation, data) = state.committed.as_ref().unwrap();
        if !persist(&Save {
            version: VERSION,
            slot,
            generation: *generation,
            data: data.clone(),
        }) {
            return false;
        }
        state.recovered = false;
        true
    })
}

fn begin(slot: u8) {
    let snapshot = CHARACTER.with(|current| current.clone());
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
        select(slot, None);
        return;
    }
    let Some(file) = file else { return };
    let written = persist(&file);
    if !written {
        error!("mhgu-sdk: failed to save character data");
    }
    SESSION.with(|session| {
        if let Some(state) = session.as_mut().filter(|state| state.slot == slot)
            && (written || state.traded)
        {
            state.committed = Some((file.generation, file.data));
            state.recovered = !written;
            state.traded = false;
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
        select(slot as u8, Some(manager));
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
    if slot >= 3 {
        return original!(manager, slot);
    }
    let slot_id = slot as u8;
    if !seal_recovered(slot_id) {
        // Same native error path as a write that failed to start.
        unsafe { ((manager + 28) as *mut u32).write(8) };
        return 0;
    }
    begin(slot_id);
    let traded = SESSION.with(|session| {
        session
            .as_ref()
            .is_some_and(|state| state.slot == slot_id && state.traded)
    });
    let result = if traded {
        STAGING.with(|staging| *staging = Some(manager));
        let result = original!(manager, slot);
        STAGING.with(|staging| *staging = None);
        result
    } else {
        original!(manager, slot)
    };
    if result != 1 {
        PENDING.with(|pending| *pending = None);
    }
    result
}

// 3E0EA4 serializes the 632-byte hunter header, item box, and pouch first.
// Bind the journal to this prefix of the native slot after serialization and
// before GU starts writing "system". The load hook sees the same slot bytes.
unsafe fn fingerprint(manager: u32, slot: u8) -> Option<u64> {
    const SAVE_SIZE: u32 = 0x4eb898;
    const PREFIX: u32 = 0x3000;
    let buffer = unsafe { read::<u32>(manager, 40) };
    if buffer == 0 {
        return None;
    }
    let start = unsafe { read::<u32>(buffer, 16 + 4 * u32::from(slot)) };
    if !(28..=SAVE_SIZE - PREFIX).contains(&start) {
        return None;
    }
    let bytes =
        unsafe { core::slice::from_raw_parts((buffer + start) as *const u8, PREFIX as usize) };
    Some(bytes.iter().fold(0xcbf29ce484222325u64, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    }))
}

#[forge::hook(offset = 0x8775f4)]
unsafe fn start_native_write(io: u32) -> u32 {
    if let Some(manager) = STAGING.with(|staging| *staging) {
        let staged = PENDING.with(|pending| {
            let Some((slot, Some(file))) = pending.as_ref() else {
                return false;
            };
            let Some(native) = (unsafe { fingerprint(manager, *slot) }) else {
                return false;
            };
            let Ok(bytes) = serde_json::to_vec(&Journal { native, save: file }) else {
                return false;
            };
            write_path(&journal_path(*slot), &bytes).is_ok()
        });
        if !staged {
            error!("mhgu-sdk: could not stage expansion weapon transactions");
            return 8;
        }
    }
    original!(io)
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
    forge::install_hook!(base, start_native_write);
}

fn write_file(slot: u8, generation: u32, bytes: &[u8]) -> Result<(), ()> {
    write_path(&path(slot, generation % 2), bytes)
}

fn write_path(path: &CStr, bytes: &[u8]) -> Result<(), ()> {
    let _mount = Mount::open()?;
    let file = MtFile::open(path, OpenMode::Write).map_err(|_| ())?;
    let written = file.write(bytes);
    file.close();
    file.destroy();
    if written != bytes.len() || read_file(path)?.as_deref() != Some(bytes) {
        return Err(());
    }
    Ok(())
}
