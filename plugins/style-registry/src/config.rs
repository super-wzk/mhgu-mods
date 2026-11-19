//! Forge savedata INI, using the same tokenizer as Forge's config module.
const SECTION: &str = "Weapons";
pub const WEAPONS: usize = 15;
type Selection = [u16; WEAPONS];

fn parse(bytes: &[u8]) -> Selection {
    let Ok(s) = core::str::from_utf8(bytes) else {
        return Selection::default();
    };
    let mut weapons = false;
    let mut selection = Selection::default();
    // Forge 1.5's Reader unwraps a missing section for a leading bare key.
    // Treat that input as inactive instead of panicking in the game.
    for item in ini_core::Parser::new(s.trim_start_matches('\u{feff}')).auto_trim(true) {
        match item {
            ini_core::Item::Section(name) => weapons = name == SECTION,
            ini_core::Item::Property(key, raw) if weapons => {
                if let Some(slot) = decimal::<usize>(key).and_then(|i| selection.get_mut(i)) {
                    *slot = raw.and_then(decimal).unwrap_or(0);
                }
            }
            ini_core::Item::Error(_) => return Selection::default(),
            _ => (),
        }
    }
    selection
}

fn decimal<T: core::str::FromStr>(s: &str) -> Option<T> {
    s.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| s.parse().ok())
        .flatten()
}

#[cfg(target_arch = "arm")]
fn mounted<T: Default>(f: impl FnOnce(&str) -> T) -> T {
    use forge::fs::savedata;
    if !savedata::mount() {
        return T::default();
    }
    struct Mount;
    impl Drop for Mount {
        fn drop(&mut self) {
            savedata::unmount();
        }
    }
    let _mount = Mount;
    f(&alloc::format!(
        "{}:/style_registry.ini",
        savedata::mount_point()
    ))
}

#[cfg(target_arch = "arm")]
pub fn save(selection: Selection) -> bool {
    use core::fmt::Write;
    mounted(|path| {
        use forge::mt::{
            fs::{MtFile, OpenMode},
            object::Object,
        };
        let Ok(path) = alloc::ffi::CString::new(path) else {
            return false;
        };
        let mut text = alloc::format!("[{SECTION}]\n");
        for (weapon, id) in selection.into_iter().enumerate().filter(|&(_, id)| id != 0) {
            let _ = writeln!(text, "{weapon} = {id}");
        }
        let Ok(file) = MtFile::open(&path, OpenMode::Write) else {
            return false;
        };
        let ok = file.write(text.as_bytes()) == text.len();
        file.close();
        file.destroy();
        ok
    })
}

#[cfg(target_arch = "arm")]
pub fn load() -> Selection {
    mounted(|path| {
        use alloc::ffi::CString;
        use forge::{
            fs::file_exists,
            mt::{
                fs::{MtFile, OpenMode},
                object::Object,
            },
        };
        if !file_exists(path) {
            return Selection::default();
        }
        let Ok(path) = CString::new(path) else {
            return Selection::default();
        };
        let Ok(file) = MtFile::open(&path, OpenMode::Read) else {
            return Selection::default();
        };
        let mut buf = [0; 4096];
        let size = file.size();
        let value = if size != 0 && size <= buf.len() && file.read(&mut buf[..size]) == size {
            parse(&buf[..size])
        } else {
            Selection::default()
        };
        file.close();
        file.destroy();
        value
    })
}

#[cfg(test)]
mod tests {
    use super::{Selection, parse};
    #[test]
    fn forge_ini() {
        let mut expected = Selection::default();
        expected[0] = 1;
        expected[11] = 2;
        assert_eq!(
            parse(b"\xef\xbb\xbf[Weapons]\r\n0 = 1\r\n11 = 2\r\n14 = 65536\r\n99 = 3"),
            expected
        );
        for invalid in [
            b"style=1".as_slice(),
            b"[General]\nstyle=1",
            b"[Other]\nstyle=1",
            b"[Weapons]\n0=65536",
            b"[Weapons]\n11=-1",
            b"[Weapons]\n0=true",
            b"[Weapons\n0=1",
            b"\xff",
            b"1",
        ] {
            assert_eq!(parse(invalid), Selection::default());
        }
    }
}
