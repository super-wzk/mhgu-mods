//! Supply expansion rows to GU's weapon shop and smithy controllers.
use super::{ACCEPT, Catalogue, Place, global, inventory, put, read};
use crate::{save::CHARACTER, weapon::model};
use core::sync::atomic::{AtomicU32, Ordering};
use mhgu_sdk::sync::Shared;

static ACTIVE: Shared<Option<u32>> = Shared::new(None);
static TRADE_RESULT: AtomicU32 = AtomicU32::new(0);
const EQUIP_ANSWERED: u32 = 1;
const TRADE_FAILED: u32 = 2;

fn active(owner: u32, place: Place) -> Option<&'static Catalogue> {
    let address = ACTIVE.with(|active| *active)?;
    if !mhgu_sdk::service::on_game_thread() {
        return None;
    }
    // Only valid for the synchronous native controller invocation.
    let catalogue = unsafe { &*(address as *const Catalogue) };
    (catalogue.owner == owner && catalogue.place == place).then_some(catalogue)
}

fn with_catalogue<T>(catalogue: &Catalogue, invoke: impl FnOnce() -> T) -> T {
    let previous = ACTIVE.with(|active| active.replace(catalogue as *const Catalogue as u32));
    let result = invoke();
    ACTIVE.with(|active| *active = previous);
    result
}

fn preview_pose() -> (u16, u16) {
    let equipment = unsafe { inventory::current_equipment() };
    if equipment == 0 {
        return (0, 0);
    }
    let category = unsafe { read::<u16>(equipment + 28) } & 31;
    if (7..=21).contains(&category) {
        (category, unsafe { read(equipment + 30) })
    } else {
        (0, 0)
    }
}

unsafe fn rows(catalogue: &Catalogue) {
    let owner = catalogue.owner;
    let first = catalogue.selected() / 8 * 8;
    unsafe {
        let (native_rows, stride) = match catalogue.place {
            Place::Shop => {
                put(owner + 66, 0u16);
                (owner + 1652, 12)
            }
            Place::Smith => {
                put(owner + 92, 0u16);
                (owner + 10452, 48)
            }
        };
        core::ptr::write_bytes(native_rows as *mut u8, 0, stride as usize * 8);
        for row in 0..8 {
            put(native_rows + stride * row, 16u16);
        }
        for (row, offer) in catalogue.entries.iter().skip(first).take(8).enumerate() {
            let record = native_rows + stride * row as u32;
            let status = CHARACTER.with(|data| match offer.check(&data.inventory) {
                Ok(()) => 0u16,
                Err(super::super::Blocked::Money) => 2,
                Err(super::super::Blocked::Material) => 4,
                Err(_) => 16,
            });
            put(record, status);
            if catalogue.place == Place::Shop {
                put(record + 8, offer.recipe.zenny);
            } else {
                put(record + 16, offer.recipe.zenny);
                for (index, material) in offer.recipe.materials.iter().enumerate() {
                    put(record + 20 + 4 * index as u32, material.item);
                    put(record + 22 + 4 * index as u32, material.quantity as u8);
                }
            }
        }
        catalogue.rows();
    }
}

unsafe fn preview(catalogue: &Catalogue) {
    let Some(offer) = catalogue.entries.get(catalogue.selected()) else {
        return;
    };
    let viewer = catalogue.viewer();
    if viewer == 0 {
        return;
    }
    model::preview(offer.equipment);
    unsafe {
        match catalogue.place {
            Place::Shop => {
                let pad = global(0x188c670);
                with_catalogue(catalogue, || {
                    gu!(0x6ed600, catalogue.owner, read::<u32>(pad + 27636))
                });
                // Start GU's own preview state machine with the live pose parameters.
                put(viewer + 53, 0u8);
                put(viewer + 60, 1u32);
            }
            Place::Smith => {
                put(catalogue.owner + 59, 0u8);
                with_catalogue(catalogue, || gu!(0x6fc2c8, catalogue.owner));
            }
        }
    }
}

unsafe fn init_cursor(catalogue: &Catalogue) {
    let owner = catalogue.owner;
    unsafe {
        gu!(0x54edec, catalogue.cursor(), 8, 1, 0);
        gu!(
            0x54ee34,
            catalogue.cursor(),
            catalogue.entries.len().max(1) as u32,
            3
        );
        gu!(0x54ecc0, catalogue.cursor(), 0);
        let cursor = if catalogue.place == Place::Shop {
            owner + 128
        } else {
            owner + 8072
        };
        gu!(0x54edec, cursor, 1, 5, 0);
        gu!(0x54ecc0, cursor, 0);
    }
}

pub(super) unsafe fn tick(catalogue: &mut Catalogue, original: impl FnOnce() -> u32) -> u32 {
    let owner = catalogue.owner;
    let selected = catalogue.selected();
    unsafe {
        let was_open = catalogue.list;
        if !was_open {
            catalogue.stock_cursor = read(catalogue.cursor());
        } else if read::<u32>(global(0x188c670) + 27644) & ACCEPT != 0 && read::<u8>(owner + 5) == 0
        {
            rows(catalogue);
        }
        TRADE_RESULT.store(0, Ordering::Relaxed);
        let result = with_catalogue(catalogue, original);
        catalogue.list = read::<u8>(owner + 4) == catalogue.place.trade_phase();
        if !catalogue.list {
            model::preview(0);
            if was_open {
                put(catalogue.cursor(), catalogue.stock_cursor);
            }
            return result;
        }

        let trade = TRADE_RESULT.swap(0, Ordering::Relaxed);
        if trade != 0 {
            catalogue.refresh();
            // Rearm GU's weapon-page initializer after the reward prompt.
            // The native storage widget cannot display an SDK-only slot.
            with_catalogue(catalogue, || match catalogue.place {
                Place::Shop => {
                    put(owner + 5, 0u8);
                    gu!(0x6e94b4, owner)
                }
                Place::Smith => gu!(0x6f2c90, owner),
            });
            if !catalogue.entries.is_empty() && catalogue.place == Place::Shop {
                gu!(
                    0x557d90,
                    catalogue.cursor(),
                    selected.min(catalogue.entries.len() - 1) as u32
                );
                rows(catalogue);
                shop_prompt(catalogue);
            }
            if trade == TRADE_FAILED {
                gu!(0x54f260, 0, 3);
            }
        }
        if !was_open || selected != catalogue.selected() || trade != 0 {
            preview(catalogue);
        }
        result
    }
}

unsafe fn shop_prompt(catalogue: &Catalogue) {
    unsafe {
        let status = read::<u16>(catalogue.owner + 1652 + (catalogue.selected() % 8) as u32 * 12);
        let message = if status & 2 != 0 {
            11
        } else if status & 4 != 0 {
            10
        } else {
            9
        };
        gu!(0x65e5bc, 4, 1, 13, message, 0, 0, 0);
    }
}

#[forge::hook(offset = 0x6eb9fc)]
unsafe fn shop_page(owner: u32, page: u32, init: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Shop) else {
        return original!(owner, page, init);
    };
    unsafe {
        if init != 0 {
            init_cursor(catalogue);
        }
        rows(catalogue);
        if init != 0 {
            shop_prompt(catalogue);
        }
    }
    0
}

#[forge::hook(offset = 0x6eb6c0)]
unsafe fn shop_rows(owner: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Shop) else {
        return original!(owner);
    };
    unsafe { rows(catalogue) };
    0
}

#[forge::hook(offset = 0x6ec2cc)]
unsafe fn shop_details(owner: u32, init: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Shop) else {
        return original!(owner, init);
    };
    unsafe { catalogue.rows() };
    0
}

#[forge::hook(offset = 0x6ece8c)]
unsafe fn shop_delivery(owner: u32, kind: u32, item: u32) -> u32 {
    if active(owner, Place::Shop).is_some() {
        1
    } else {
        original!(owner, kind, item)
    }
}

#[forge::hook(offset = 0x6ed128)]
unsafe fn shop_commit(owner: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Shop) else {
        return original!(owner);
    };
    let index = unsafe { read::<u16>(owner + 108) } as usize * 8
        + unsafe { read::<u8>(owner + 58) } as usize;
    if let Some(offer) = catalogue.entries.get(index) {
        // The projected category and item are zero. GU's allocation
        // routine returns 0xffff before touching the retail equipment box.
        let result = offer.complete(|| {
            original!(owner);
        });
        remember_reward(catalogue, result.ok());
    } else {
        remember_reward(catalogue, None);
    }
    unsafe { put(owner + 72, u16::MAX) };
    0
}

#[forge::hook(offset = 0x6ed200)]
unsafe fn shop_equipment_box(owner: u32) -> u32 {
    if let Some(catalogue) = active(owner, Place::Shop) {
        answer_equip(catalogue, false);
        0
    } else {
        original!(owner)
    }
}

#[forge::hook(offset = 0x6ed344)]
unsafe fn shop_equip(owner: u32) -> u32 {
    if let Some(catalogue) = active(owner, Place::Shop) {
        answer_equip(catalogue, true);
        0
    } else {
        original!(owner)
    }
}

// The native smithy enters phase 9 while its list provider gathers rows.
#[forge::hook(offset = 0x6f9438)]
unsafe fn smith_catalogue(owner: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Smith) else {
        return original!(owner);
    };
    unsafe {
        put(owner + 98, catalogue.entries.len() as u16);
        put(owner + 60, 0u8);
    }
    0
}

#[forge::hook(offset = 0x6f53e4)]
unsafe fn smith_page(owner: u32, page: u32, init: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Smith) else {
        return original!(owner, page, init);
    };
    unsafe {
        if init != 0 {
            init_cursor(catalogue);
        }
        rows(catalogue);
    }
    0
}

#[forge::hook(offset = 0x6f8648)]
unsafe fn smith_rows(owner: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Smith) else {
        return original!(owner);
    };
    unsafe { rows(catalogue) };
    0
}

#[forge::hook(offset = 0x6f9b18)]
unsafe fn smith_details(owner: u32, mode: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Smith) else {
        return original!(owner, mode);
    };
    unsafe {
        catalogue.rows();
        gu!(0x6fa3d8, owner);
    }
    0
}

#[forge::hook(offset = 0x6fcabc)]
unsafe fn smith_delivery(owner: u32, kind: u32, item: u32) -> u32 {
    if active(owner, Place::Smith).is_some() {
        // 70B6EC returns bit 0 for a compatible weapon.
        1
    } else {
        original!(owner, kind, item)
    }
}

#[forge::hook(offset = 0x6fb224)]
unsafe fn smith_commit(owner: u32) -> u32 {
    let Some(catalogue) = active(owner, Place::Smith) else {
        return original!(owner);
    };
    let index = unsafe { read::<u16>(owner + 8052) } as usize * 8
        + unsafe { read::<u8>(owner + 68) } as usize;
    if let Some(offer) = catalogue.entries.get(index) {
        // The projected category and item are zero. GU skips retail
        // allocation and runs its original recipe debit.
        let result = offer.complete(|| {
            original!(owner);
        });
        remember_reward(catalogue, result.ok());
    } else {
        remember_reward(catalogue, None);
    }
    unsafe { put(owner + 102, u16::MAX) };
    0
}

#[forge::hook(offset = 0x6fb364)]
unsafe fn smith_equipment_box(owner: u32) -> u32 {
    if let Some(catalogue) = active(owner, Place::Smith) {
        answer_equip(catalogue, false);
        0
    } else {
        original!(owner)
    }
}

#[forge::hook(offset = 0x6fbf4c)]
unsafe fn smith_equip(owner: u32) -> u32 {
    if let Some(catalogue) = active(owner, Place::Smith) {
        answer_equip(catalogue, true);
        0
    } else {
        original!(owner)
    }
}

fn remember_reward(catalogue: &Catalogue, reward: Option<u32>) {
    catalogue.pending_equip.set(reward);
    if reward.is_none() {
        TRADE_RESULT.store(TRADE_FAILED, Ordering::Relaxed);
    }
}

fn answer_equip(catalogue: &Catalogue, equip: bool) {
    if let Some(id) = catalogue.pending_equip.take()
        && equip
    {
        inventory::select(Some(id));
    }
    if TRADE_RESULT.load(Ordering::Relaxed) != TRADE_FAILED {
        TRADE_RESULT.store(EQUIP_ANSWERED, Ordering::Relaxed);
    }
}

// GU's viewer still needs a pose from the current native loadout. Give
// only the preview callback those inputs; the commerce rows retain no
// valid retail equipment identity.
unsafe fn with_preview_pose(catalogue: &Catalogue, invoke: impl FnOnce() -> u32) -> u32 {
    let (category, row_id) = preview_pose();
    let index = (catalogue.selected() % 8) as u32;
    let (category_field, row_field) = match catalogue.place {
        Place::Shop => (catalogue.owner + 66, catalogue.owner + 1656 + index * 12),
        Place::Smith => (catalogue.owner + 92, catalogue.owner + 10454 + index * 48),
    };
    unsafe {
        let old_category = read::<u16>(category_field);
        let old_row = read::<u16>(row_field);
        put(category_field, category);
        put(row_field, row_id);
        let result = invoke();
        put(row_field, old_row);
        put(category_field, old_category);
        result
    }
}

#[forge::hook(offset = 0x6ed600)]
unsafe fn shop_preview(owner: u32, buttons: u32) -> u32 {
    if let Some(catalogue) = active(owner, Place::Shop) {
        unsafe { with_preview_pose(catalogue, || original!(owner, buttons)) }
    } else {
        original!(owner, buttons)
    }
}

#[forge::hook(offset = 0x6fc2c8)]
unsafe fn smith_preview(owner: u32) -> u32 {
    if let Some(catalogue) = active(owner, Place::Smith) {
        unsafe { with_preview_pose(catalogue, || original!(owner)) }
    } else {
        original!(owner)
    }
}

#[forge::hook(offset = 0x54f260)]
unsafe fn trade_sound(group: u32, sound: u32) -> u32 {
    // GU plays these cues immediately after its reward callback, which has
    // no failure return. Suppress them if the scoped SDK debit rolled back.
    if matches!((group, sound), (0, 10) | (1, 117))
        && ACTIVE.with(|active| active.is_some())
        && mhgu_sdk::service::on_game_thread()
        && TRADE_RESULT.load(Ordering::Relaxed) == TRADE_FAILED
    {
        return 0;
    }
    original!(group, sound)
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, shop_page);
    forge::install_hook!(base, shop_rows);
    forge::install_hook!(base, shop_details);
    forge::install_hook!(base, shop_delivery);
    forge::install_hook!(base, shop_commit);
    forge::install_hook!(base, shop_equipment_box);
    forge::install_hook!(base, shop_equip);
    forge::install_hook!(base, smith_catalogue);
    forge::install_hook!(base, smith_page);
    forge::install_hook!(base, smith_rows);
    forge::install_hook!(base, smith_details);
    forge::install_hook!(base, smith_delivery);
    forge::install_hook!(base, smith_commit);
    forge::install_hook!(base, smith_equipment_box);
    forge::install_hook!(base, smith_equip);
    forge::install_hook!(base, trade_sound);
    forge::install_hook!(base, shop_preview);
    forge::install_hook!(base, smith_preview);
}
