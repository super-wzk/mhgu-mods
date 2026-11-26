use super::with;
use crate::monster::records::{Totals, decode};
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

// All 8192 possible packed IDs at the longest permitted key fit in one file.
const LIMIT: usize = 1024 * 1024;

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
        "{}:/monster-registry-{slot}-{parity}.tsv",
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
fn load(slot: u8) -> Result<(u32, Totals), ()> {
    let _mount = Mount::open()?;
    let mut newest = None;
    let mut present = false;
    for parity in 0..2 {
        let data = match read_file(&path(slot, parity)) {
            Ok(None) => continue,
            Ok(Some(bytes)) => decode(&bytes),
            Err(()) => None,
        };
        present = true;
        if let Some((generation, totals)) = data
            && generation % 2 == parity
            && newest.as_ref().is_none_or(|(old, _)| generation > *old)
        {
            newest = Some((generation, totals));
        }
    }
    if present {
        newest.ok_or(())
    } else {
        Ok((0, Totals::new()))
    }
}
pub(super) fn select(raw: u32) {
    with(|s| s.pending_save_slot = None);
    let slot = u8::try_from(raw).ok().filter(|&s| s < 3);
    let loaded = slot.and_then(|s| load(s).ok());
    if loaded.is_none() {
        error!("monster-registry: character records unavailable; preserving sidecars");
    }
    with(|s| {
        s.slot = slot;
        let (n, data) = loaded.map_or((0, None), |(n, data)| (n, Some(data)));
        s.generation = n;
        s.registry.load(data);
    });
}
pub(super) fn finish(success: bool) {
    let Some(slot) = with(|s| s.pending_save_slot.take()) else {
        return;
    };
    if !success {
        return;
    }
    // A newly created hunter may save an empty slot before ever entering the
    // ordinary character-selection callback. Never copy another slot's session.
    if with(|s| s.slot.is_none()) {
        select(slot.into());
        return;
    }
    let Some((generation, bytes)) = with(|s| {
        if s.slot != Some(slot) {
            return None;
        }
        let next = s.generation.checked_add(1)?;
        Some((next, s.registry.snapshot(next)?))
    }) else {
        return;
    };
    if write_file(slot, generation, bytes.as_bytes()).is_err() {
        error!("monster-registry: failed to save character records");
        return;
    }
    with(|s| {
        if s.slot == Some(slot) && s.generation.checked_add(1) == Some(generation) {
            s.generation = generation;
            s.registry.saved();
        }
    });
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
