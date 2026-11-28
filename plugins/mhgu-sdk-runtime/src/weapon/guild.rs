//! Extension data for GU's card, diary and weapon-usage views.
use super::{gu, icon, inventory, model, records::DisplayEquipment, read, set, style};
use crate::save::{self, CHARACTER, CardWeapon, DiaryWeapon, OwnedWeapon};
use alloc::{ffi::CString, format, vec::Vec};
use core::ffi::CStr;
use mhgu_sdk::sync::Shared;

const GUILD: u32 = 0x1857668;
const HUNTERS: u32 = 0x1885b58;
const CARD_SIZE: usize = 6328;
const CARD_WEAPON: u32 = 84;
const DIARY: u32 = 2328;
const DIARY_ROWS: usize = 10;
const EVENT_BYTES: u32 = 160;
const USAGE: u32 = 2234;
const COLUMNS: usize = 15;
const USAGE_GROUPS: u32 = 0x1631104;
const COLUMN_PARENT: u32 = 0x24; // guildcard_down.gui: Null_bar
const ICON_PART: u32 = 1;
const BAR_PART: u32 = 2;
const COUNT_PART: u32 = 3;

fn manager() -> u32 {
    unsafe { read(forge::mem::text_addr(), GUILD) }
}

unsafe fn own(card: u32) -> bool {
    let guild = manager();
    guild != 0 && card != 0 && unsafe { gu!(0x166a30, guild, card, guild + 32) == 1 }
}

unsafe fn fingerprint(addr: u32, size: usize) -> u64 {
    unsafe { core::slice::from_raw_parts(addr as *const u8, size) }
        .iter()
        .fold(0xcbf29ce484222325, |hash, &b| {
            (hash ^ u64::from(b)).wrapping_mul(0x100000001b3)
        })
}

// 1623B4 and 166344 copy the retail weapon fields individually. These
// reserved bytes travel with the card, including copies kept in the album.
unsafe fn stamped_weapon(row: u32) -> Option<OwnedWeapon> {
    unsafe {
        ([read::<u8>(row, 1), read(row, 5), read(row, 43)] == *b"FZW").then(|| OwnedWeapon {
            equipment: read(row, 6),
            upgrade: read(row, 14),
        })
    }
}

unsafe fn stamp_weapon(row: u32, weapon: Option<OwnedWeapon>) {
    unsafe {
        if let Some(weapon) = weapon {
            for (offset, byte) in [(1, b'F'), (5, b'Z'), (43, b'W')] {
                set(row, offset, byte);
            }
            set(row, 6, weapon.equipment);
            set(row, 14, weapon.upgrade);
        } else if stamped_weapon(row).is_some() {
            for offset in [1, 5, 43] {
                set(row, offset, 0u8);
            }
            set(row, 6, 0u16);
            set(row, 14, 0u16);
        }
    }
}

// 161AC8 refreshes the card with 1623B4 only when GU allows equipment updates.
#[forge::hook(offset = 0x1623b4)]
unsafe fn snapshot_equipment(owner: u32, card: u32, equipment: u32) {
    original!(owner, card, equipment);
    let guild = manager();
    if guild == 0 || card != guild + 32 || !save::character_loaded() {
        return;
    }
    let item = unsafe { crate::weapon::records::display(equipment) }.map(|weapon| OwnedWeapon {
        equipment: weapon.definition.equipment.id.get(),
        upgrade: weapon.upgrade,
    });
    unsafe { stamp_weapon(card + CARD_WEAPON, item) };
    let weapon = item.map(|weapon| CardWeapon {
        weapon,
        fingerprint: unsafe { fingerprint(card + CARD_WEAPON, 44) },
    });
    CHARACTER.with(|data| data.guild_card.equipment = weapon);
}

unsafe fn card_equipment(card: u32) -> Option<DisplayEquipment> {
    if card == 0 {
        return None;
    }
    if let Some(weapon) = unsafe { stamped_weapon(card + CARD_WEAPON) } {
        return DisplayEquipment::registered(weapon.equipment, weapon.upgrade);
    }
    if !unsafe { own(card) } {
        return None;
    }
    let snapshot = CHARACTER.with(|data| data.guild_card.equipment);
    if let Some(snapshot) = snapshot
        && snapshot.fingerprint == unsafe { fingerprint(card + CARD_WEAPON, 44) }
    {
        return DisplayEquipment::registered(snapshot.weapon.equipment, snapshot.weapon.upgrade);
    }
    unsafe {
        let (item, _) = inventory::selected()?;
        let native = inventory::current_equipment();
        if native == 0 {
            return None;
        }
        // Cards saved with the presentation locked skip 1623B4. For a card
        // without SDK metadata, match the saved retail equipment before using
        // the extension selected over that same record.
        if read::<u8>(card, 2232) & 1 != 0 {
            let header: u16 = read(native, 28);
            if u16::from(read::<u8>(card, CARD_WEAPON)) != header & 31
                || read::<u16>(card, CARD_WEAPON + 2) != read::<u16>(native, 30)
                || u16::from(read::<u8>(card, CARD_WEAPON + 4)) != (header >> 5) & 31
            {
                return None;
            }
        }
        DisplayEquipment::registered(item.equipment.get(), item.upgrade)
    }
}

#[forge::hook(offset = 0x5a41a0)]
unsafe fn draw_equipment(view: u32) -> u32 {
    let out = original!(view);
    unsafe {
        if let Some(weapon) = card_equipment(read(view, 80)) {
            // First row of the native dword_16310CC table.
            let group = read(forge::mem::text_addr(), 0x16310cc);
            gu!(
                0x56e808,
                view,
                group,
                2,
                weapon.definition.name.as_ptr() as u32
            );
            icon::equipment(view, group, 1, weapon.definition);
        }
    }
    out
}

static PREVIEW_LOAD: Shared<Option<(u32, DisplayEquipment)>> = Shared::new(None);
static PREVIEW_ACTIVE: Shared<Option<(u32, DisplayEquipment)>> = Shared::new(None);

#[forge::hook(offset = 0x15fadc)]
unsafe fn build_preview(preview: u32) -> u32 {
    let manager: u32 = unsafe { read(forge::mem::text_addr(), HUNTERS) };
    let card: u32 = unsafe { read(preview, 24) };
    let weapon = (manager != 0 && card != 0 && unsafe { read::<u8>(card, 2232) } & 2 == 0)
        .then(|| unsafe { card_equipment(card) })
        .flatten();
    if let Some(model) = weapon.and_then(|weapon| weapon.definition.model) {
        model::mesh(model);
    }
    PREVIEW_LOAD.with(|scope| *scope = weapon.map(|weapon| (manager, weapon)));
    let out = original!(preview);
    PREVIEW_LOAD.with(|scope| *scope = None);
    out
}

// The same scratch slot is reused for other GU preview screens.
#[forge::hook(offset = 0x2744bc)]
unsafe fn load_preview_equipment(manager: u32, slot: u32, equipment: u32) -> u32 {
    if slot == 0 {
        PREVIEW_ACTIVE.with(|active| {
            *active = PREVIEW_LOAD.with(|scope| scope.filter(|(owner, _)| *owner == manager));
        });
    }
    original!(manager, slot, equipment)
}

pub(super) unsafe fn preview_equipment(manager: u32, actor: u32) -> Option<DisplayEquipment> {
    if unsafe { read::<u32>(actor, 5064) } != 4 {
        return None;
    }
    let (owner, weapon) = PREVIEW_ACTIVE.with(|active| *active)?;
    (owner == manager
        && (unsafe { read::<u32>(manager, 48) } == actor
            || PREVIEW_LOAD.with(|scope| scope.is_some())))
    .then_some(weapon)
}

static QUEST_WEAPON: Shared<Option<(u32, DiaryWeapon)>> = Shared::new(None);

#[forge::hook(offset = 0x388ab4)]
unsafe fn begin_diary_entry(result: u32) -> u32 {
    QUEST_WEAPON.with(|event| *event = None);
    original!(result)
}

unsafe fn usage_source() -> Option<(u32, u16, u32, u16)> {
    if !save::character_loaded() {
        return None;
    }
    let (item, entry) = inventory::selected()?;
    let kind = entry.kind?.id.get();
    let guild = manager();
    let base = forge::mem::text_addr();
    let quest: u32 = unsafe { read(base, 0x188ad90) };
    let quests: u32 = unsafe { read(base, 0x188ae38) };
    let hunters: u32 = unsafe { read(base, HUNTERS) };
    unsafe {
        if guild == 0
            || quest == 0
            || quests == 0
            || hunters == 0
            || gu!(0x3b9870, quests, read(quest, 224)) == 1
        {
            return None;
        }
        // 38EAE4 uses the local hunter record in solo play and the matching
        // participant's recorded loadout online. GU remaps its native kind here.
        let record = gu!(0xdf814, hunters + 128, 0);
        let native_kind = u32::from(read::<u16>(record, 4) & 31);
        let index = native_kind.checked_sub(if (7..=11).contains(&native_kind) {
            7
        } else {
            8
        })?;
        (index < 14).then_some((guild + 32, kind, index, item.equipment.get()))
    }
}

#[forge::hook(offset = 0x38eae4)]
unsafe fn record_usage(result: u32) -> u32 {
    let Some((card, kind, native_index, equipment)) = (unsafe { usage_source() }) else {
        return original!(result);
    };
    unsafe {
        // Let GU choose the bucket and apply all its eligibility checks. Use
        // zero as the starting count even if the fallback reached GU's cap.
        let first = card + USAGE + 2 * native_index;
        let offsets = [0, 30, 60];
        let counts = offsets.map(|offset| read::<u16>(first, offset));
        let event = result + 760;
        let previous: [u8; 28] = read(event, 40);
        for offset in offsets {
            set(first, offset, 0u16);
        }
        let out = original!(result);
        let bucket = offsets
            .iter()
            .position(|&offset| read::<u16>(first, offset) == 1);
        for (offset, count) in offsets.into_iter().zip(counts) {
            set(first, offset, count);
        }
        // A zero total can make the excluded-quest branch attempt a native
        // milestone of zero. Restore the original subevents if it did.
        let types: [u8; 3] = read(event, 40);
        let spurious = types.iter().enumerate().any(|(i, &ty)| {
            ty == 26
                && read::<u32>(event, 44 + i as u32 * 4) == native_index
                && read::<u32>(event, 56 + i as u32 * 4) == 0
        });
        if spurious {
            set(event, 40, previous);
        }
        if let Some(bucket) = bucket {
            let milestone = CHARACTER.with(|data| {
                let counts = data.guild_card.usage.entry(kind).or_default();
                let before = counts.iter().copied().map(u32::from).sum::<u32>();
                counts[bucket] = counts[bucket].saturating_add(1).min(9999);
                let total = counts.iter().copied().map(u32::from).sum::<u32>();
                if total > before && total % 100 == 0 {
                    total as u16
                } else {
                    0
                }
            });
            QUEST_WEAPON.with(|pending| {
                *pending = Some((
                    event,
                    DiaryWeapon {
                        equipment,
                        milestone,
                    },
                ));
            });
        }
        out
    }
}

#[forge::hook(offset = 0x162758)]
unsafe fn store_diary_entry(guild: u32, event: u32) {
    original!(guild, event);
    let pending = QUEST_WEAPON.with(Option::take);
    if guild != manager() || !save::character_loaded() {
        return;
    }
    unsafe {
        let card = guild + 32;
        let first = fingerprint(card + DIARY, EVENT_BYTES as usize);
        let entries = core::array::from_fn::<_, DIARY_ROWS, _>(|i| {
            fingerprint(card + DIARY + EVENT_BYTES * i as u32, EVENT_BYTES as usize)
        });
        CHARACTER.with(|data| {
            data.guild_card.diary.retain(|key, _| entries.contains(key));
            if let Some((owner, weapon)) = pending
                && owner == event
                && matches!(read::<u16>(event, 4), 7 | 8)
                && fingerprint(event, EVENT_BYTES as usize) == first
            {
                data.guild_card.diary.insert(first, weapon);
            }
        });
    }
}

#[forge::hook(offset = 0x5a7b00)]
unsafe fn draw_diary(view: u32) -> u32 {
    let out = original!(view);
    unsafe {
        let card: u32 = read(view, 80);
        if !own(card) {
            return out;
        }
        let start = u32::from(read::<u8>(view, 743));
        let rows = (u32::from(read::<u8>(view, 742)).saturating_sub(start)).min(11);
        for row in 0..rows {
            // 5A19C8 uses ten text pointers per native diary event. Index 1
            // within each block is the quest headline, index 0 is the date.
            let index = u32::from(read::<u8>(view, 632 + start + row));
            if index % 10 != 1 || index / 10 >= DIARY_ROWS as u32 {
                continue;
            }
            let event = card + DIARY + EVENT_BYTES * (index / 10);
            let key = fingerprint(event, EVENT_BYTES as usize);
            let saved = CHARACTER.with(|data| data.guild_card.diary.get(&key).copied());
            if let Some(saved) = saved
                && let Some((style, _)) = inventory::definition(saved.equipment)
                && let Some(name) = style.weapon_name
            {
                let row_data: u32 = read(view, 192 + index * 4);
                let title = CStr::from_ptr(read::<u32>(row_data, 0) as *const _).to_string_lossy();
                let name = name.to_string_lossy();
                let text = if saved.milestone == 0 {
                    format!("{title}  {name}")
                } else {
                    format!("{title}  {name}累计{}次", saved.milestone)
                };
                if let Ok(text) = CString::new(text) {
                    let part = read(forge::mem::text_addr(), 0x163116c + row * 4);
                    gu!(0x56ea08, view, 0xac, part, text.as_ptr() as u32);
                }
            }
        }
    }
    out
}

#[derive(Clone, Copy)]
struct Column {
    group: u32,
    position: [f32; 2],
    center: f32,
    icon_size: [f32; 2],
    bar_width: f32,
}

struct Chart {
    view: u32,
    layout: u32,
    stock: [Column; COLUMNS],
    extra: Vec<Column>,
}

static CHART: Shared<Option<Chart>> = Shared::new(None);

#[forge::hook(offset = 0x59eaa0)]
unsafe fn close_card(view: u32) -> u32 {
    CHART.with(|chart| {
        if chart.as_ref().is_some_and(|chart| chart.view == view) {
            *chart = None;
        }
    });
    original!(view)
}

unsafe fn chart_group(index: usize) -> u32 {
    unsafe { read(forge::mem::text_addr(), USAGE_GROUPS + index as u32 * 4) }
}

unsafe fn column(layout: u32, group: u32) -> Option<Column> {
    unsafe {
        let node = gu!(0x544fe8, layout, group);
        if node == 0 {
            return None;
        }
        let children: u32 = read(node, 216);
        if children == 0 {
            return None;
        }
        let picture: u32 = read(children, ICON_PART * 4);
        let bar: u32 = read(children, BAR_PART * 4);
        if picture == 0 || bar == 0 || read::<u32>(children, COUNT_PART * 4) == 0 {
            return None;
        }
        let position: [f32; 2] = read(node, 144);
        Some(Column {
            group,
            position,
            center: position[0] + read::<f32>(picture, 128),
            icon_size: read(picture, 180),
            bar_width: read(bar, 180),
        })
    }
}

// These native setters pass the float in s0.
unsafe fn set_part_float(offset: u32, layout: u32, group: u32, part: u32, value: f32) {
    unsafe {
        core::arch::asm!(
            "blx {f}", f = in(reg) forge::mem::text_addr() + offset,
            in("r0") layout, in("r1") group, in("r2") part,
            in("s0") value, clobber_abi("C"),
        );
    }
}

unsafe fn place_column(layout: u32, column: Column, center: f32, scale: f32) {
    let Column {
        group,
        mut position,
        icon_size,
        bar_width,
        ..
    } = column;
    position[0] += center - column.center;
    unsafe {
        gu!(0x545bcc, layout, group, position.as_ptr() as u32);
        set_part_float(0x546afc, layout, group, ICON_PART, icon_size[0] * scale);
        set_part_float(0x546afc, layout, group, BAR_PART, bar_width * scale);
        set_part_float(0x546b64, layout, group, ICON_PART, icon_size[1] * scale);
    }
}

unsafe fn paint_count(view: u32, layout: u32, group: u32, count: u32) {
    unsafe {
        let unit = [0.01, 0.002, 0.001, 0.0001][read::<u32>(view, 152).min(3) as usize];
        let ratio = count as f32 * unit;
        let height = (ratio * 0.9).min(1.) * read::<f32>(view, 160);
        let y = read::<f32>(view, 164) - 2. - height;
        gu!(0x56fde0, view, group, COUNT_PART, count, 0, 0);
        set_part_float(0x545cb0, layout, group, COUNT_PART, y);
        set_part_float(0x546b64, layout, group, BAR_PART, height);
    }
}

fn count(totals: [u16; 3], tab: u32) -> u32 {
    match tab {
        0 => totals.into_iter().map(u32::from).sum::<u32>().min(9999),
        1..=3 => u32::from(totals[(tab - 1) as usize]).min(9999),
        _ => 0,
    }
}

unsafe fn chart_for(view: u32, layout: u32, extras: usize) -> Option<Chart> {
    unsafe {
        let mut chart = if let Some(chart) = CHART
            .with(Option::take)
            .filter(|chart| chart.view == view && chart.layout == layout)
        {
            chart
        } else {
            let mut stock = [column(layout, chart_group(0))?; COLUMNS];
            for (i, node) in stock.iter_mut().enumerate().skip(1) {
                *node = column(layout, chart_group(i))?;
            }
            // The usage table is indexed by weapon ID, not screen position.
            stock.sort_unstable_by(|a, b| a.center.total_cmp(&b.center));
            Chart {
                view,
                layout,
                stock,
                extra: Vec::new(),
            }
        };
        while chart.extra.len() < extras {
            let anchor = chart.stock[COLUMNS - 1];
            let node = gu!(0x544fe8, layout, anchor.group);
            let resource: u32 = read(node, 208);
            let template: u32 = read(node, 212);
            let group = gu!(
                0x54501c,
                layout,
                resource,
                template,
                COLUMN_PARENT,
                forge::mem::text_addr() + 0x2123290
            );
            if group == u32::MAX {
                break;
            }
            // 54501C selects sequence 1 by default. The usage template
            // has one sequence per native column (1000004 through 1000018).
            gu!(0x546c74, layout, group, gu!(0x546cb0, layout, anchor.group));
            if column(layout, group).is_none() {
                gu!(0x54533c, layout, group, 0);
                break;
            }
            for part in [ICON_PART, BAR_PART, COUNT_PART] {
                let position = gu!(0x545ce8, layout, anchor.group, part);
                gu!(0x545d60, layout, group, part, position);
            }
            chart.extra.push(Column { group, ..anchor });
        }
        Some(chart)
    }
}

#[forge::hook(offset = 0x59f560)]
unsafe fn change_card_page(view: u32) -> u32 {
    unsafe {
        if read::<u32>(view, 132) != 3
            && let Some((layout, columns)) = CHART.with(|state| {
                state
                    .as_ref()
                    .filter(|chart| chart.view == view && chart.layout == read::<u32>(view, 36))
                    .map(|chart| (chart.layout, chart.extra.clone()))
            })
        {
            for column in columns {
                gu!(0x54533c, layout, column.group, 0);
            }
        }
    }
    original!(view)
}

unsafe fn draw_columns(chart: &Chart, extras: &[(u32, u32)]) {
    let first = chart.stock[0].center;
    let last = chart.stock[COLUMNS - 1].center;
    let stock_step = (last - first) / (COLUMNS - 1) as f32;
    let scale = COLUMNS as f32 / (COLUMNS + extras.len()) as f32;
    let left = first - stock_step * 0.5;
    let step = stock_step * scale;
    let center = |index: usize| left + (index as f32 + 0.5) * step;
    let Chart { view, layout, .. } = *chart;
    unsafe {
        for (i, column) in chart.stock.iter().copied().enumerate() {
            let x = if extras.is_empty() {
                column.center
            } else {
                center(i)
            };
            place_column(layout, column, x, scale);
        }
        for (i, (&column, &(icon, count))) in chart.extra.iter().zip(extras).enumerate() {
            let group = column.group;
            paint_count(view, layout, group, count);
            gu!(0x56e6a8, view, group, ICON_PART, 0, icon);
            gu!(0x56e6fc, view, group, ICON_PART, u32::MAX);
            gu!(0x54533c, layout, group, 1);
            place_column(layout, column, center(COLUMNS + i), scale);
        }
        for column in chart.extra.iter().skip(extras.len()) {
            gu!(0x54533c, layout, column.group, 0);
        }
    }
}

#[forge::hook(offset = 0x5a6cb4)]
unsafe fn draw_usage(view: u32) -> u32 {
    unsafe {
        let card: u32 = read(view, 80);
        let layout: u32 = read(view, 36);
        if layout == 0 || card == 0 {
            return original!(view);
        }
        let mut kinds: Vec<_> = if own(card) {
            style::registered_entries()
                .filter_map(|entry| entry.kind)
                .collect()
        } else {
            Vec::new()
        };
        kinds.sort_unstable_by_key(|kind| kind.id.get());
        let Some(chart) = chart_for(view, layout, kinds.len()) else {
            return original!(view);
        };
        let tab: u32 = read(view, 156);
        let stock_count = |i: usize| {
            let totals =
                [0u32, 30, 60].map(|bucket| read::<u16>(card, USAGE + i as u32 * 2 + bucket));
            count(totals, tab)
        };
        let extras: Vec<_> = CHARACTER.with(|data| {
            kinds
                .iter()
                .take(chart.extra.len())
                .map(|kind| {
                    let totals = data
                        .guild_card
                        .usage
                        .get(&kind.id.get())
                        .copied()
                        .unwrap_or_default();
                    (u32::from(kind.icon), count(totals, tab))
                })
                .collect()
        });
        let max_extra = extras.iter().map(|&(_, count)| count).max().unwrap_or(0);
        let stock_max = (0..COLUMNS).map(stock_count).max().unwrap_or(0);
        let out = if max_extra > stock_max && tab <= 3 {
            // Allow the native legend and zoom logic to see the largest value
            // in either set. Restore the original first value after the draw.
            let mut copy = alloc::boxed::Box::new([0u32; CARD_SIZE / 4]);
            core::ptr::copy_nonoverlapping(card as *const u32, copy.as_mut_ptr(), copy.len());
            let copy_addr = copy.as_mut_ptr() as u32;
            for bucket in 0..3 {
                set(
                    copy_addr,
                    USAGE + 30 * bucket,
                    if bucket == tab.saturating_sub(1) {
                        max_extra as u16
                    } else {
                        0
                    },
                );
            }
            set(view, 80, copy_addr);
            let out = original!(view);
            set(view, 80, card);
            paint_count(view, layout, chart_group(0), stock_count(0));
            out
        } else {
            original!(view)
        };
        draw_columns(&chart, &extras);
        CHART.with(|state| *state = Some(chart));
        out
    }
}

pub(super) fn install(base: u32) {
    forge::install_hook!(base, snapshot_equipment);
    forge::install_hook!(base, draw_equipment);
    forge::install_hook!(base, build_preview);
    forge::install_hook!(base, load_preview_equipment);
    forge::install_hook!(base, begin_diary_entry);
    forge::install_hook!(base, record_usage);
    forge::install_hook!(base, store_diary_entry);
    forge::install_hook!(base, draw_diary);
    forge::install_hook!(base, close_card);
    forge::install_hook!(base, change_card_page);
    forge::install_hook!(base, draw_usage);
}
