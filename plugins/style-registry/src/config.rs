//! Forge savedata INI, using the same tokenizer as Forge's config module.
const SECTION: &str = "General";
const KEY: &str = "style";

fn parse(bytes: &[u8]) -> u16 {
    let Ok(s) = core::str::from_utf8(bytes) else {
        return 0;
    };
    let mut general = false;
    let mut value = 0;
    // Forge 1.5's Reader unwraps a missing section for a leading bare key.
    // Treat that input as inactive instead of panicking in the game.
    for item in ini_core::Parser::new(s.trim_start_matches('\u{feff}')).auto_trim(true) {
        match item {
            ini_core::Item::Section(name) => general = name == SECTION,
            ini_core::Item::Property(KEY, raw) if general => {
                value = raw
                    .filter(|v| v.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
            ini_core::Item::Error(_) => return 0,
            _ => (),
        }
    }
    value
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
pub fn save(id: u16) -> bool {
    mounted(|path| write(path, id))
}

#[cfg(target_arch = "arm")]
fn write(path: &str, id: u16) -> bool {
    use forge::mt::{
        fs::{MtFile, OpenMode},
        object::Object,
    };
    let Ok(path) = alloc::ffi::CString::new(path) else {
        return false;
    };
    let Ok(file) = MtFile::open(&path, OpenMode::Write) else {
        return false;
    };
    let text = alloc::format!("[{SECTION}]\n{KEY} = {id}\n");
    let ok = file.write(text.as_bytes()) == text.len();
    file.close();
    file.destroy();
    ok
}

#[cfg(target_arch = "arm")]
pub fn load() -> u16 {
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
            write(path, 0);
            return 0;
        }
        let Ok(path) = CString::new(path) else {
            return 0;
        };
        let Ok(file) = MtFile::open(&path, OpenMode::Read) else {
            return 0;
        };
        let mut buf = [0; 4096];
        let size = file.size();
        let value = if size != 0 && size <= buf.len() && file.read(&mut buf[..size]) == size {
            parse(&buf[..size])
        } else {
            0
        };
        file.close();
        file.destroy();
        value
    })
}

#[cfg(test)]
mod tests {
    use super::parse;
    #[test]
    fn forge_ini() {
        assert_eq!(parse(b"; preference\r\n[General]\r\nstyle = 1\r\n"), 1);
        assert_eq!(parse(b"\xef\xbb\xbf[General]\nstyle=65535"), 65535);
        for invalid in [
            b"style=1".as_slice(),
            b"[Other]\nstyle=1",
            b"[General]\nstyle=65536",
            b"[General]\nstyle=-1",
            b"[General]\nstyle=true",
            b"[General\nstyle=1",
            b"\xff",
            b"1",
        ] {
            assert_eq!(parse(invalid), 0);
        }
    }
}
