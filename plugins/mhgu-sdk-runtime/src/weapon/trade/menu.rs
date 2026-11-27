//! Extension categories in the native Buy Weapon and Forge Weapon selectors.
//! Panel 2 consumes GU's 8-row catalogue, and smithy panel 10 its recipe.
use super::{Offer, global, has_offers, inventory, offers, read};
use alloc::borrow::ToOwned;
use alloc::{ffi::CString, vec::Vec};
use core::sync::atomic::{AtomicU32, Ordering};
use core::{cell::Cell, ffi::CStr};
use forge::log::prelude::info;
use mhgu_sdk::sync::Shared;

mod native;
mod smith_home;
mod upgrade;

const ACCEPT: u32 = 0x2000;
const BACK: u32 = 0x4000;
const UP: u32 = 0x10;
const DOWN: u32 = 0x40;
const PREV: u32 = 0x80;
const NEXT: u32 = 0x20;
const CATEGORY_ROWS: usize = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Shop,
    Smith,
}
impl Place {
    fn cursor(self) -> u32 {
        if self == Self::Shop { 92 } else { 8036 }
    }
    fn trade_phase(self) -> u8 {
        if self == Self::Shop { 5 } else { 3 }
    }
    fn category(self, owner: u32) -> bool {
        unsafe {
            match self {
                Self::Shop => read::<u8>(owner + 4) == 3 && read::<u8>(owner + 63) == 0,
                Self::Smith => read::<u8>(owner + 4) == 2 && read::<u8>(owner + 76) == 0,
            }
        }
    }
    fn rows(self) -> u32 {
        if self == Self::Shop { 1808 } else { 11600 }
    }
    fn category_rows(self) -> u32 {
        self.rows() - 60
    }
}

struct Catalogue {
    owner: u32,
    place: Place,
    weapon: u16,
    stock_cursor: [u32; 9],
    entries: Vec<Offer>,
    labels: Vec<CString>,
    list: bool,
    pending_equip: Cell<Option<u32>>,
}
static MENU: Shared<Option<Catalogue>> = Shared::new(None);
static PAGE_PANEL: Shared<Option<(u32, u32, Place)>> = Shared::new(None);

fn active_category(owner: u32, place: Place) -> Option<(u16, bool)> {
    MENU.with(|menu| {
        menu.as_ref()
            .filter(|menu| menu.owner == owner && menu.place == place)
            .map(|menu| (menu.weapon, menu.list))
    })
}

#[derive(Clone, Copy)]
struct WeaponCategory {
    id: u16,
    name: &'static CStr,
    icon: u8,
}

fn weapon_categories(place: Place) -> Vec<WeaponCategory> {
    let mut weapons: Vec<_> = crate::weapon::style::registered_entries()
        .filter_map(|entry| {
            let weapon = entry.kind?;
            let id = weapon.id.get();
            has_offers(place == Place::Smith, id).then_some(WeaponCategory {
                id,
                name: entry.weapon_name?,
                icon: weapon.icon,
            })
        })
        .collect();
    weapons.sort_unstable_by_key(|weapon| weapon.id);
    weapons
}

fn report_category(place: Place, drawing: bool, loaded: bool, entries: usize) {
    static REPORTED: AtomicU32 = AtomicU32::new(0);
    let index =
        place as u32 * 8 + u32::from(drawing) * 4 + u32::from(loaded) * 2 + u32::from(entries != 0);
    let bit = 1 << index;
    if REPORTED.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
        info!(
            "mhgu-sdk: {place:?} weapon categories (draw={drawing}, save={loaded}, custom={entries})"
        );
    }
}

unsafe fn category_position(cursor: u32) -> (usize, usize, usize) {
    unsafe {
        let page = usize::from(read::<u16>(cursor + 16));
        let width = usize::from(read::<u8>(cursor + 14));
        let first = page * width;
        let total = usize::from(read::<u16>(cursor + 20));
        let selected = usize::from(read::<u8>(cursor + 13));
        (first, total, selected)
    }
}
unsafe fn put<T>(a: u32, v: T) {
    unsafe { (a as *mut T).write_unaligned(v) }
}
impl Catalogue {
    fn cursor(&self) -> u32 {
        self.owner + self.place.cursor()
    }
    fn selected(&self) -> usize {
        unsafe { gu!(0x557d78, self.cursor()) as usize }
    }
    fn refresh(&mut self) {
        self.entries = offers(self.place == Place::Smith);
        self.entries.retain(|offer| {
            inventory::definition(offer.equipment)
                .is_some_and(|(_, entry)| entry.equipment.weapon.get() == self.weapon)
        });
        self.entries.truncate(u16::MAX as usize);
        self.labels = self
            .entries
            .iter()
            .map(|o| {
                let name = inventory::definition(o.equipment).unwrap().1.name;
                name.to_owned()
            })
            .collect();
    }
    unsafe fn rows(&self) {
        let selected = self.selected();
        let first = selected / 8 * 8;
        let header = self.owner + self.place.rows();
        unsafe {
            core::ptr::write_bytes(header as *mut u8, 0, 16 + 24 * 8);
            put(header + 2, 1u8);
            for (i, offer) in self.entries.iter().skip(first).take(8).enumerate() {
                let (_, weapon) = inventory::definition(offer.equipment).unwrap();
                let row = header + 16 + 24 * i as u32;
                put(row, super::super::icon::equipment_color(weapon));
                put(row + 4, u16::from(weapon.kind.icon));
                put(
                    row + 6,
                    if super::CHARACTER.with(|d| offer.check(&d.inventory)).is_ok() {
                        1u8
                    } else {
                        2u8
                    },
                );
                put(row + 8, offer.recipe.zenny);
                let label = self.labels[first + i].as_ptr() as u32;
                put(row + 16, label);
                put(row + 20, label);
            }
            if let Some(offer) = self.entries.get(selected) {
                let (_, entry) = inventory::definition(offer.equipment).unwrap();
                let level = match offer.kind {
                    super::Kind::Upgrade(level) => u32::from(level) + 2,
                    _ => 1,
                };
                super::super::menu::details(gu!(0x65cb3c, self.owner, 5), Some(entry), level);
                put(header, 1u8);
                put(header + 3, 0u8);
                put(
                    header + 6,
                    super::CHARACTER.with(|c| {
                        c.inventory
                            .instances
                            .values()
                            .filter(|i| i.equipment == offer.equipment)
                            .count() as u16
                    }),
                );
            }
        }
    }
    fn viewer(&self) -> u32 {
        unsafe {
            read(
                self.owner
                    + if self.place == Place::Shop {
                        2016
                    } else {
                        12816
                    },
            )
        }
    }
}

unsafe fn update(owner: u32, place: Place, original: impl FnOnce() -> u32) -> Option<u32> {
    unsafe {
        let mut menu = MENU
            .with(Option::take)
            .filter(|m| m.owner == owner && m.place == place);
        if let Some(mut current) = menu.take_if(|current| current.list) {
            let phase = read::<u8>(owner + 4);
            if phase == place.trade_phase() {
                let result = native::tick(&mut current, original);
                return Some(keep_category(current, result));
            }
        }
        if place == Place::Smith {
            if read::<u8>(owner + 4) == 1 {
                return smith_home::update(owner, original);
            }
            if read::<u8>(owner + 4) == 7 && upgrade::opened(owner) {
                return Some(upgrade::tick(owner, original));
            }
        }
        if !place.category(owner) {
            crate::weapon::model::preview(0);
            return None;
        }
        let loaded = crate::save::character_loaded();
        let weapons = weapon_categories(place);
        report_category(place, false, loaded, weapons.len());
        if !loaded {
            return None;
        }
        let pad = global(0x188c670);
        if pad == 0 {
            return None;
        }
        let pressed = read::<u32>(pad + 27644);
        let buttons = pressed | read::<u32>(pad + 27700);
        let cursor = owner + place.cursor();
        let panel = gu!(0x65cb3c, owner, 0);
        if panel == 0 || gu!(0x65cb3c, owner, 2) == 0 {
            return None;
        }
        let page_panel = gu!(0x65cb3c, owner, 1);
        PAGE_PANEL.with(|page| *page = Some((page_panel, cursor, place)));
        let (first, total, selected) = category_position(cursor);
        let selected_type = menu
            .as_ref()
            .filter(|menu| !menu.list)
            .and_then(|menu| weapons.iter().position(|weapon| weapon.id == menu.weapon));
        if gu!(0x65d9e8,) != 0 {
            let handled = menu.is_some();
            MENU.with(|s| *s = menu);
            return handled.then_some(0);
        }
        if pressed & (ACCEPT | BACK) != 0 {
            if pressed & (ACCEPT | BACK) == ACCEPT
                && let Some(mut current) = menu
            {
                let result = native::tick(&mut current, original);
                return Some(keep_category(current, result));
            }
            return None;
        }
        if total == 0 || weapons.is_empty() {
            return None;
        }

        let direction = buttons & (UP | DOWN | PREV | NEXT);
        let focus = selected_type.map_or(first + selected, |index| total + index);
        let count = total + weapons.len();
        let page = focus / CATEGORY_ROWS;
        let pages = count.div_ceil(CATEGORY_ROWS);
        let next = match direction {
            PREV => (page + pages - 1) % pages * CATEGORY_ROWS,
            NEXT => (page + 1) % pages * CATEGORY_ROWS,
            UP if selected_type.is_some() => focus - 1,
            DOWN if focus + 1 >= total => (focus + 1).min(count - 1),
            _ => return menu.map(|current| keep_category(current, 0)),
        };
        if next < total {
            if selected_type.is_none() {
                return None;
            }
            let target_page = next / CATEGORY_ROWS;
            let result = if target_page != first / CATEGORY_ROWS {
                // Ask the original facility to change stock pages so it
                // rebuilds the localized category descriptors.
                let stock_pages = total.div_ceil(CATEGORY_ROWS);
                let (key, before) = if direction == NEXT {
                    (NEXT, (target_page + stock_pages - 1) % stock_pages)
                } else {
                    (PREV, (target_page + 1) % stock_pages)
                };
                gu!(0x557d90, cursor, (before * CATEGORY_ROWS) as u32);
                let repeat = read::<u32>(pad + 27700);
                put(pad + 27644, (pressed & !(UP | DOWN | PREV | NEXT)) | key);
                put(pad + 27700, repeat & !(UP | DOWN | PREV | NEXT));
                let result = original();
                put(pad + 27644, pressed);
                put(pad + 27700, repeat);
                result
            } else {
                gu!(0x54f260, 0, 0);
                0
            };
            gu!(0x557d90, cursor, next as u32);
            return Some(result);
        }
        let weapon = weapons.get(next - total)?;
        let mut current = menu.unwrap_or_else(|| Catalogue {
            owner,
            place,
            weapon: weapon.id,
            stock_cursor: read(cursor),
            entries: Vec::new(),
            labels: Vec::new(),
            list: false,
            pending_equip: Cell::new(None),
        });
        if current.weapon != weapon.id || current.entries.is_empty() {
            current.weapon = weapon.id;
            current.refresh();
        }
        if current.entries.is_empty() {
            return None;
        }
        gu!(0x54f260, 0, 0);
        Some(keep_category(current, 0))
    }
}

fn keep_category(current: Catalogue, result: u32) -> u32 {
    MENU.with(|s| *s = Some(current));
    result
}

#[forge::hook(offset = 0x6e8558)]
unsafe fn shop(owner: u32) -> u32 {
    unsafe { update(owner, Place::Shop, || original!(owner)) }.unwrap_or_else(|| original!(owner))
}
#[forge::hook(offset = 0x6f1ae8)]
unsafe fn smith(owner: u32) -> u32 {
    unsafe { update(owner, Place::Smith, || original!(owner)) }.unwrap_or_else(|| original!(owner))
}

// cUIOShopTopPanel's virtual update tail-branches to the shared row renderer.
// Hook the virtual entry so recompiled callers also pass through the SDK.
#[forge::hook(offset = 0x592fe0)]
unsafe fn category(panel: u32) -> u32 {
    unsafe {
        let cursor = read::<u32>(panel + 76);
        let desc = read::<u32>(panel + 84);
        // GU binds both pointers to fields of the facility object. Resolve
        // the owner at draw time, including draws before its input callback.
        let Some(place) = [Place::Shop, Place::Smith]
            .into_iter()
            .find(|place| desc.checked_sub(cursor) == Some(place.category_rows() - place.cursor()))
        else {
            return original!(panel);
        };
        let Some(owner) = cursor.checked_sub(place.cursor()) else { return original!(panel) };
        if place == Place::Smith && read::<u8>(owner + 4) == 1 {
            return smith_home::draw(owner, panel, || original!(panel));
        }
        if !place.category(owner) {
            return original!(panel);
        }
        let active = active_category(owner, place);
        if active.is_some_and(|(_, list)| list) {
            return 0;
        }
        let loaded = crate::save::character_loaded();
        let weapons = weapon_categories(place);
        report_category(place, true, loaded, weapons.len());
        if !loaded || weapons.is_empty() {
            return original!(panel);
        }
        let (first, total, selected) = category_position(cursor);
        let selected_type = active.and_then(|(id, _)| weapons.iter().position(|w| w.id == id));
        let focus = selected_type.map_or(first + selected, |index| total + index);
        let page_start = focus / CATEGORY_ROWS * CATEGORY_ROWS;
        let stock = total.saturating_sub(page_start).min(CATEGORY_ROWS);
        if stock != 0 {
            // GU still owns the stock descriptor for this page. A focus beyond
            // its rows suppresses the stock highlight on a mixed page.
            put(cursor + 13, (focus % CATEGORY_ROWS) as u8);
            original!(panel);
            put(cursor + 13, selected as u8);
        } else {
            for widget in 0..CATEGORY_ROWS {
                gu!(0x56e528, panel, widget as u32, 0);
            }
        }
        for (row, weapon) in weapons
            .iter()
            .skip(page_start.saturating_sub(total))
            .take(CATEGORY_ROWS - stock)
            .enumerate()
        {
            let widget = (stock + row) as u32;
            gu!(0x56e528, panel, widget, 1);
            gu!(
                0x56e538,
                panel,
                widget,
                if focus % CATEGORY_ROWS == widget as usize {
                    4
                } else {
                    1
                }
            );
            // Native category layout variant 2 has a label and an atlas icon.
            for part in [2, 4, 9, 14] {
                gu!(0x56e530, panel, widget, part, u32::from(part == 9));
            }
            gu!(0x56e530, panel, widget, 24, 0);
            gu!(0x56e530, panel, widget, 10, 1);
            gu!(0x56e808, panel, widget, 10, weapon.name.as_ptr() as u32);
            gu!(0x56e530, panel, widget, 11, 1);
            gu!(0x56e6a8, panel, widget, 11, 0, u32::from(weapon.icon));
            gu!(0x56e6fc, panel, widget, 10, gu!(0x560e90, 0, 0));
        }
        0
    }
}

// Project the logical category page into GU's native page indicator.
#[forge::hook(offset = 0x593514)]
unsafe fn category_page(panel: u32) -> u32 {
    let Some((_, cursor, place)) =
        PAGE_PANEL.with(|page| page.filter(|context| context.0 == panel))
    else {
        return original!(panel);
    };
    unsafe {
        let owner = cursor - place.cursor();
        if read::<u32>(panel + 76) != cursor || !place.category(owner) {
            return original!(panel);
        }
        let active = active_category(owner, place);
        if active.is_some_and(|(_, list)| list) {
            return original!(panel);
        }
        let weapons = weapon_categories(place);
        if weapons.is_empty() || !crate::save::character_loaded() {
            return original!(panel);
        }
        let (first, total, _) = category_position(cursor);
        let pages = (total + weapons.len()).div_ceil(CATEGORY_ROWS);
        let visible_page = active
            .and_then(|(id, _)| weapons.iter().position(|weapon| weapon.id == id))
            .map_or(first, |index| total + index)
            / CATEGORY_ROWS;
        let mut shadow = read::<[u32; 9]>(cursor);
        let projected = shadow.as_mut_ptr() as u32;
        put(projected + 16, visible_page as u16);
        put(projected + 18, pages as u16);
        put(panel + 76, projected);
        let result = original!(panel);
        put(panel + 76, cursor);
        result
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, shop);
    forge::install_hook!(base, smith);
    forge::install_hook!(base, category);
    forge::install_hook!(base, category_page);
    native::install(base);
    upgrade::install(base);
}
