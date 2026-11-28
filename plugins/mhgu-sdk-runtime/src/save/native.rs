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

fn path(slot: u8, suffix: &str) -> CString {
    CString::new(format!(
        "{}:/mhgu-sdk-{slot}{suffix}",
        savedata::mount_point()
    ))
    .unwrap()
}

// GU 1.4 imports nn::fs::DeleteFile and RenameFile. Both return zero on success.
fn remove(path: &CStr) -> Result<(), ()> {
    let delete: unsafe extern "C" fn(*const core::ffi::c_char) -> u32 =
        unsafe { core::mem::transmute(forge::mem::text_addr() + 0x13f0424) };
    if !file_exists(path.to_str().unwrap()) || unsafe { delete(path.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(())
    }
}

fn rename(from: &CStr, to: &CStr) -> Result<(), ()> {
    let rename_file: unsafe extern "C" fn(
        *const core::ffi::c_char,
        *const core::ffi::c_char,
    ) -> u32 = unsafe { core::mem::transmute(forge::mem::text_addr() + 0x13f0430) };
    (unsafe { rename_file(from.as_ptr(), to.as_ptr()) } == 0)
        .then_some(())
        .ok_or(())
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

// If replacement stopped after removing the old name, the verified new file
// still has its temporary name. Never overwrite that sole copy on a retry.
fn finish_replace(slot: u8, main: &CStr, temp: &CStr) -> Result<(), ()> {
    if !file_exists(main.to_str().unwrap())
        && let Some(bytes) = read_file(temp)?
    {
        if Save::decode(&bytes, slot).is_some() {
            rename(temp, main)?;
        } else {
            remove(temp)?;
        }
    }
    Ok(())
}

fn empty(slot: u8) -> Save {
    Save {
        version: VERSION,
        slot,
        data: CharacterData::default(),
    }
}

fn load(slot: u8) -> Result<Save, ()> {
    let _mount = Mount::open()?;
    let main = path(slot, ".json");
    let temp = path(slot, ".tmp");
    finish_replace(slot, &main, &temp)?;
    if let Some(bytes) = read_file(&main)? {
        let save = Save::decode(&bytes, slot).ok_or(())?;
        let _ = remove(&temp);
        return Ok(save);
    }

    Ok(empty(slot))
}

// The title screen reads all three slots before it selects a character.
pub(crate) fn preview_weapon(slot: u8) -> Option<(super::OwnedWeapon, [u16; 3])> {
    if slot >= 3 { return None; }
    let _mount = Mount::open().ok()?;
    let bytes = read_file(&path(slot, ".json")).ok()??;
    let inventory = Save::decode(&bytes, slot)?.data.inventory;
    let id = inventory.equipped?;
    Some((*inventory.instances.get(&id)?, inventory.decorations.get(&id).copied().unwrap_or_default()))
}

static SESSION: Shared<Option<Save>> = Shared::new(None);
static PENDING: Shared<Option<(u8, Option<Save>)>> = Shared::new(None);

pub(crate) fn character_loaded() -> bool {
    SESSION.with(|session| session.is_some())
}

fn use_character(save: Result<Save, ()>) {
    let save = save.ok();
    if save.is_none() {
        error!("mhgu-sdk: character data unavailable");
    }
    PENDING.with(|pending| *pending = None);
    CHARACTER.with(|current| *current = save.as_ref().map(|s| s.data.clone()).unwrap_or_default());
    SESSION.with(|session| *session = save);
}

fn persist(file: &Save) -> Result<(), ()> {
    let bytes = serde_json::to_vec(file).map_err(|_| ())?;
    if bytes.len() > LIMIT {
        return Err(());
    }
    let _mount = Mount::open()?;
    let main = path(file.slot, ".json");
    let temp = path(file.slot, ".tmp");
    finish_replace(file.slot, &main, &temp)?;
    let staged = MtFile::open(&temp, OpenMode::Write).map_err(|_| ())?;
    let written = staged.write(&bytes);
    staged.close();
    staged.destroy();
    if written != bytes.len() || read_file(&temp)?.as_deref() != Some(&bytes) {
        return Err(());
    }
    remove(&main)?;
    rename(&temp, &main)?;
    Ok(())
}

fn create(slot: u8) {
    let file = empty(slot);
    use_character(persist(&file).map(|()| file));
}

fn begin(slot: u8) {
    let file = SESSION.with(|session| {
        let previous = session.as_ref().filter(|s| s.slot == slot)?;
        CHARACTER.with(|current| {
            (current != &previous.data).then(|| Save {
                version: VERSION,
                slot,
                data: current.clone(),
            })
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
    if let Some(file) = file {
        if persist(&file).is_ok() {
            SESSION.with(|session| *session = Some(file));
        } else {
            error!("mhgu-sdk: failed to save character data");
        }
    } else if !character_loaded() {
        use_character(load(slot));
    }
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
        use_character(load(slot as u8));
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
    begin(slot as u8);
    let result = original!(manager, slot);
    if result != 1 {
        PENDING.with(|pending| *pending = None);
    }
    result
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
