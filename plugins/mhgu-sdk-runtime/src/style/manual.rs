//! GU 1.4 Hunter's Notes style rows and pause manual reader.
use super::{Entry, entries, registered_entries, registered_entry};
use alloc::{boxed::Box, ffi::CString, vec::Vec};
use core::{
    ffi::CStr,
    sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed},
};
use mhgu_sdk::style::manual;

pub(super) struct Page {
    body: CString,
    heading: CString,
    rows: Vec<(CString, CString)>,
}

impl Page {
    fn parse(block: &str) -> Option<Self> {
        let mut text = alloc::string::String::from(block);
        for (key, icon) in [
            ("{X}", "PAD_X"),
            ("{A}", "PAD_A"),
            ("{B}", "PAD_B"),
            ("{R}", "PAD_R"),
            ("{Y}", "PAD_Y"),
            ("{摇杆}", "ANALOG"),
        ] {
            if text.contains(key) {
                text = text.replace(
                    key,
                    &alloc::format!("<SIZE 28><COL E6E6E6FF><ICON {icon}></COL></SIZE>"),
                );
            }
        }
        let mut lines = text.lines();
        let heading = CString::new(lines.next()?).ok()?;
        let mut rows = Vec::new();
        if text.contains('\t') {
            for line in lines {
                let (input, action) = line.split_once('\t')?;
                if input.is_empty() || action.is_empty() || action.contains('\t') || rows.len() == 9
                {
                    return None;
                }
                rows.push((CString::new(input).ok()?, CString::new(action).ok()?));
            }
        }
        let body = CString::new(text.replace('\t', "　").replace('\n', "\r\n")).ok()?;
        Some(Self {
            body,
            heading,
            rows,
        })
    }
}

unsafe extern "C" fn bind(id: u32, data: *const u8, len: usize) -> bool {
    let Some(entry) = registered_entry(id) else {
        return false;
    };
    if data.is_null() || len == 0 || len > isize::MAX as usize {
        return false;
    }
    let Ok(text) = core::str::from_utf8(unsafe { core::slice::from_raw_parts(data, len) }) else {
        return false;
    };
    let normalized = text.replace("\r\n", "\n");
    let Some(pages) = normalized
        .trim()
        .split("\n\n")
        .map(Page::parse)
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    if pages.len() > 255 {
        return false;
    }
    entry.manual.with(|slot| {
        if slot.is_some() {
            return false;
        }
        *slot = Some(Box::leak(pages.into_boxed_slice()));
        true
    })
}

type Book = (&'static Entry, &'static [Page]);
fn book(e: &'static Entry) -> Option<Book> {
    e.manual.with(|pages| pages.map(|p| (e, p)))
}

// 16320E0 contains GU's equipment categories (weapon id + 7) in notebook order.
const WEAPONS: [u32; 14] = [0, 7, 1, 11, 2, 12, 3, 9, 8, 14, 13, 6, 4, 10];
const ROWS: [u32; 8] = [9, 10, 11, 12, 13, 16, 15, 14];
fn books(index: u8) -> impl Iterator<Item = Book> {
    WEAPONS
        .get(usize::from(index))
        .into_iter()
        .flat_map(|&w| entries(w))
        .filter_map(book)
}

unsafe fn get<T: Copy>(p: u32, offset: u32) -> T {
    unsafe { ((p + offset) as *const T).read_unaligned() }
}
unsafe fn put<T>(p: u32, offset: u32, value: T) {
    unsafe { ((p + offset) as *mut T).write_unaligned(value) };
}
macro_rules! gu {
    ($offset:expr $(, $arg:expr)*) => {{
        let f: unsafe extern "C" fn($(gu!(@word $arg)),*) -> u32 =
            core::mem::transmute(forge::mem::text_addr() + $offset);
        f($($arg),*)
    }};
    (@word $arg:expr) => { u32 };
}
unsafe fn cursor_value(cursor: u32, method: u32) -> usize {
    unsafe {
        let f: unsafe extern "C" fn(u32) -> u32 =
            core::mem::transmute(get::<u32>(get(cursor, 0), method));
        f(cursor) as usize
    }
}

#[forge::hook(offset = 0x5b1294)]
unsafe fn notes_count(panel: u32) -> u32 {
    let result = original!(panel);
    unsafe {
        if get::<u8>(panel, 88) == 2 && get::<u8>(panel, 80) == 6 {
            let index = get(get(panel, 92), 1);
            put(panel, 80, (6 + books(index).count()).min(255) as u8);
        }
    }
    result
}

#[forge::hook(offset = 0x5b0f70)]
unsafe fn notes_list(panel: u32) -> u32 {
    unsafe {
        let total = get::<u8>(panel, 80);
        if get::<u8>(panel, 88) != 2 || total <= 6 {
            return original!(panel);
        }
        let cursor = get(panel, 76);
        let first = 8 * cursor_value(cursor, 36);
        // The stock draw indexes a six-element label table. Let it draw only
        // its rows (or an empty page), then reuse the remaining row widgets.
        put(panel, 80, if first == 0 { 6u8 } else { first as u8 });
        let result = original!(panel);
        put(panel, 80, total);
        let layout: u32 = get(panel, 36);
        let index = get(get(panel, 92), 1);
        for (row, (entry, _)) in ROWS
            .iter()
            .skip(6usize.saturating_sub(first))
            .zip(books(index).skip(first.saturating_sub(6)))
        {
            gu!(0x54533c, layout, *row, 1);
            gu!(0x56e530, panel, *row, 1, 1);
            let color = gu!(0x550354, 1, 0, 0, 0);
            gu!(0x56e6fc, panel, *row, 1, color);
            gu!(0x56e538, panel, *row, 1_000_051);
            gu!(0x56e808, panel, *row, 1, entry.name.as_ptr() as u32);
        }
        result
    }
}

unsafe fn detail_book(panel: u32) -> Option<Book> {
    unsafe {
        if get::<u8>(panel, 83) != 255 {
            return None;
        }
        let row = usize::from(get::<u8>(panel, 82)).checked_sub(6)?;
        books(get(panel, 81)).nth(row)
    }
}

#[forge::hook(offset = 0x646a3c)]
unsafe fn notes_pages(panel: u32, cursor: u32) -> u32 {
    unsafe {
        let Some((_, pages)) = detail_book(panel) else {
            return original!(panel, cursor);
        };
        put(panel, 76, cursor);
        put(panel, 80, pages.len() as u8);
        gu!(0x54edec, cursor, 1, 0, 0);
        gu!(0x54ee34, cursor, pages.len() as u32, 10);
        let arrows: u32 = get(panel, 84);
        put(arrows, 124, cursor);
        gu!(0x600de4, arrows, 1)
    }
}

#[forge::hook(offset = 0x646ad0)]
unsafe fn notes_draw(panel: u32) -> u32 {
    unsafe {
        let Some((entry, pages)) = detail_book(panel) else {
            return original!(panel);
        };
        let layout = get(panel, 36);
        let cursor = get(panel, 76);
        let page = cursor_value(cursor, 36).min(pages.len() - 1);
        let index = u32::from(get::<u8>(panel, 81));
        gu!(0x65eb68, 0);
        gu!(0x54533c, layout, 0x27, 1);
        gu!(0x56e538, panel, 0x27, 1_000_006);
        let id: u32 = get(forge::mem::text_addr() + 0x163b05c, index * 4);
        let weapon = gu!(0x55bc4c, get(forge::mem::text_addr(), 0x1899220), 1, id);
        gu!(0x56e808, panel, 0x27, 2, weapon);
        gu!(
            0x56e6a8,
            panel,
            0x27,
            3,
            0,
            get(forge::mem::text_addr() + 0x163b094, index * 4)
        );
        let page = &pages[page];
        let table = !page.rows.is_empty();
        let title = if table { &page.heading } else { &entry.name };
        gu!(0x56e808, panel, 3, 2, title.as_ptr() as u32);
        gu!(0x54533c, layout, 0, u32::from(table));
        gu!(0x54533c, layout, 0x10, u32::from(!table));
        gu!(0x54533c, layout, 0x13, 0);
        if !table {
            return gu!(0x56e808, panel, 0x11, 1, page.body.as_ptr() as u32);
        }
        for i in 0..9 {
            let row = get::<u8>(forge::mem::text_addr() + 0x163b0d4, i) as u32;
            let (input, action) = page
                .rows
                .get(i as usize)
                .map_or((c"", c""), |(k, a)| (k.as_c_str(), a.as_c_str()));
            gu!(0x56e808, panel, row, 2, input.as_ptr() as u32);
            gu!(0x56e808, panel, row, 3, action.as_ptr() as u32);
        }
        0
    }
}

// The pause manual uses six streamed texture tiles, without text-page slots.
// Offer the same books in GU's resident information dialog from that screen.
static READER: AtomicU32 = AtomicU32::new(0);
static HINT: AtomicBool = AtomicBool::new(false);
const HINT_TEXT: &CStr = c"<ICON PAD_A> 武器操作指南";
fn all_pages() -> impl Iterator<Item = (&'static Entry, usize, &'static CStr)> {
    WEAPONS
        .into_iter()
        .flat_map(entries)
        .filter_map(book)
        .flat_map(|(e, pages)| {
            pages
                .iter()
                .enumerate()
                .map(move |(i, p)| (e, i, p.body.as_c_str()))
        })
}

#[forge::hook(offset = 0x5cb780)]
unsafe fn pause_input(panel: u32) -> u32 {
    unsafe {
        if get::<u32>(panel, 76) != 0 || !registered_entries().any(|e| book(e).is_some()) {
            return original!(panel);
        }
        if !HINT.load(Relaxed) {
            let hint = gu!(0x65e580);
            // GU keeps three saved hint states. Restore ours when the manual closes.
            if hint != 0 && get::<u8>(hint, 156) == 0 {
                gu!(0x642c44, hint);
                gu!(0x642a78, hint, 4, 0);
                gu!(0x642b6c, hint, HINT_TEXT.as_ptr() as u32, 0);
                HINT.store(true, Relaxed);
            }
        }
        let pad: u32 = get(forge::mem::text_addr(), 0x188c670);
        let pressed: u32 = get(pad, 27644);
        let current = READER.load(Relaxed);
        let next = if current == 0 {
            if pressed & 0x2000 == 0 {
                return original!(panel);
            }
            let dialog = gu!(0x65efd0);
            if dialog == 0 || get::<u8>(dialog, 24) != 0 {
                return original!(panel);
            }
            0
        } else if pressed & 0x4000 != 0 {
            close_dialog();
            READER.store(0, Relaxed);
            return 0;
        } else if pressed & (0x10 | 0x40) != 0 {
            let starts = || {
                all_pages()
                    .enumerate()
                    .filter_map(|(i, (_, p, _))| (p == 0).then_some(i as u32))
            };
            if pressed & 0x40 != 0 {
                starts().find(|&start| start >= current).unwrap_or(0)
            } else {
                let active = starts()
                    .take_while(|&start| start < current)
                    .last()
                    .unwrap_or(0);
                starts()
                    .take_while(|&start| start < active)
                    .last()
                    .or_else(|| starts().last())
                    .unwrap_or(0)
            }
        } else if pressed & (0x20 | 0x80) != 0 {
            let n = all_pages().count() as u32;
            if pressed & 0x20 != 0 {
                current % n
            } else {
                (current + n - 2) % n
            }
        } else {
            return 0;
        };
        if let Some((entry, page, body)) = all_pages().nth(next as usize) {
            let text = CString::new(alloc::format!(
                "{}　{}　左右翻页　上下换武器\r\n{}",
                entry.name.to_string_lossy(),
                page + 1,
                body.to_string_lossy()
            ))
            .unwrap();
            if current == 0 {
                if gu!(0x65f190, 0, text.as_ptr() as u32, 0, 0, 0) == 0 {
                    return 0;
                }
            } else {
                let dialog = gu!(0x65efd0);
                if dialog != 0 {
                    gu!(0x56e808, dialog, 7, 1, text.as_ptr() as u32);
                }
            }
            READER.store(next + 1, Relaxed);
        }
        0
    }
}

unsafe fn close_dialog() {
    unsafe {
        let dialog = gu!(0x65efd0);
        if dialog != 0 {
            gu!(0x64c600, dialog);
        }
    }
}

unsafe extern "C" fn close_manual() -> u32 {
    const UI_MANAGER: u32 = 0x1899458;
    const MANUAL_SLOT: u32 = 13;

    unsafe {
        if READER.swap(0, Relaxed) != 0 {
            close_dialog();
        }
        if HINT.swap(false, Relaxed) {
            let hint = gu!(0x65e580);
            if hint != 0 {
                gu!(0x642d34, hint);
            }
        }

        // GU 65F7A4 loads a PC-relative literal in its second instruction.
        // Forge's ARM trampoline copies it without relocation. Preserve the
        // native close and removal sequence through its original callees.
        let base = forge::mem::text_addr();
        let container = gu!(0x55d2b4, get::<u32>(base, UI_MANAGER), 0);
        if container != 0 {
            let dialog = gu!(0x65cb3c, container, MANUAL_SLOT);
            if dialog != 0 {
                gu!(0x56e04c, dialog);
            }
        }
        let container = gu!(0x55d2b4, get::<u32>(base, UI_MANAGER), 0);
        if container == 0 {
            return 0;
        }
        gu!(0x65cb04, container, MANUAL_SLOT)
    }
}

pub(super) fn install(base: u32) {
    static SERVICE: manual::Service = manual::Service(bind);
    if unsafe { manual::publish(&SERVICE) } {
        forge::install_hook!(base, notes_count);
        forge::install_hook!(base, notes_list);
        forge::install_hook!(base, notes_pages);
        forge::install_hook!(base, notes_draw);
        forge::install_hook!(base, pause_input);
        unsafe {
            forge::sys::hook::forge_hook_create(
                (base + 0x65f7a4) as *const _,
                close_manual as *const _,
                core::ptr::null_mut(),
            );
        }
    }
}
