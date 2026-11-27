//! Install an extended weapon's meter in GU's quest HUD.
use super::{gu, local_hunter, read, set, style};
use core::sync::atomic::{AtomicBool, Ordering};
use mhgu_sdk::{
    sync::Shared,
    weapon::style::hud::{
        self, GaugePlate, MAX_METER_SEGMENTS, Meter, MeterService, SampleMeter, Signal,
    },
};

static EXTENDED_WEAPON: AtomicBool = AtomicBool::new(false);

const VITAL_WIDGET: u32 = 0x1777d58;
const SHARPNESS: u32 = 5; // hud0.gui: Anm_sharp.
const HUD_ROOT: u32 = 10; // hud0.gui: Nul_hud0.
const BAR_TEMPLATE: u32 = 5; // hud0.gui: vital_gauge, root and two polygons.
const SIGNAL_INDEX: usize = MAX_METER_SEGMENTS;
const TRACK_START: usize = SIGNAL_INDEX + 1;
const STEP: f32 = 19.;
const WIDTH: f32 = 16.;
const HEIGHT: f32 = 8.;
const TRACK: u32 = 0xe04a3b30;
const CHARGE: u32 = 0xff7beed3;
const TARGET: u32 = 0xff64cfff;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LayoutRole {
    Status,
    Weapon,
    Other,
}

// uGUIResourceOwner records attached cUIO widgets via GU 544E44. Hide a whole
// layout only when it is dedicated to weapon gauges. HUD0, Hunter Arts and
// other shared resources can have several widget owners.
unsafe fn layout_role(layout: u32) -> LayoutRole {
    unsafe {
        let count = read::<u32>(layout, 608);
        let widgets = read::<u32>(layout, 620);
        if widgets == 0 || count == 0 || count > 96 {
            return LayoutRole::Other;
        }
        let base = forge::mem::text_addr();
        let mut role = LayoutRole::Weapon;
        for i in 0..count {
            let widget = read::<u32>(widgets, 4 * i);
            if widget == 0 {
                role = LayoutRole::Other;
                continue;
            }
            match read::<u32>(widget, 0).wrapping_sub(base) {
                VITAL_WIDGET => return LayoutRole::Status,
                0x1777dd4 // Bow
                | 0x1777e5c // Charge Blade
                | 0x1778a68 // Bowgun
                | 0x1778af0 // Gunlance
                | 0x1778b78 // Insect Glaive
                | 0x1778c20 // Hunting Horn
                | 0x1778cbc // Switch Axe
                | 0x1778d44 // Long Sword
                | 0x1778dcc => {} // Dual Blades
                _ => role = LayoutRole::Other,
            }
        }
        role
    }
}

/// Resolve stock and dynamically inserted indices in the current layout.
unsafe fn group(layout: u32, index: u32) -> u32 {
    unsafe {
        let count = read::<u32>(layout, 664);
        if index < count {
            if read::<u32>(layout, 248) == 0 {
                return 0;
            }
        } else if index - count >= read::<u32>(layout, 648) || read::<u32>(layout, 660) == 0 {
            return 0;
        }
        gu!(0x544fe8, layout, index)
    }
}

unsafe fn polygons(node: u32, expected_members: u32) -> Option<[u32; 2]> {
    unsafe {
        if read::<u32>(node, 212) != BAR_TEMPLATE || gu!(0xa38ffc, node) != expected_members {
            return None;
        }
        let members = read::<u32>(node, 216);
        if members == 0 {
            return None;
        }
        let polygons = read::<[u32; 2]>(members, 4);
        polygons
            .iter()
            .all(|&p| {
                p != 0
                    && matches!(
                        read::<u32>(p, 0).wrapping_sub(forge::mem::text_addr()),
                        0x178fc48 | 0x176c744
                    )
            })
            .then_some(polygons)
    }
}

struct MeterLayout {
    owner: u32,
    root: u32,
    groups: [Option<MeterGroup>; TRACK_START + 2],
}

impl MeterLayout {
    const fn new(owner: u32, root: u32) -> Self {
        Self {
            owner,
            root,
            groups: [None; TRACK_START + 2],
        }
    }
}

static METER: Shared<MeterLayout> = Shared::new(MeterLayout::new(0, 0));

#[derive(Clone, Copy)]
struct MeterGroup {
    index: u32,
    node: u32,
    refill_sprite: u32,
    spend_sprite: u32,
}

impl MeterGroup {
    unsafe fn create(layout: u32) -> Option<Self> {
        unsafe {
            let index = gu!(
                0x54501c,
                layout,
                u32::MAX,
                BAR_TEMPLATE,
                HUD_ROOT,
                forge::mem::text_addr() + 0x2123290
            );
            let node = group(layout, index);
            (node != 0).then_some(Self {
                index,
                node,
                refill_sprite: 0,
                spend_sprite: 0,
            })
        }
    }
}

unsafe fn overlay_sprite(
    layout: u32,
    parent: u32,
    sprite: &mut u32,
    atlas: Option<&GaugePlate>,
    frame: u8,
) {
    unsafe {
        let atlas = atlas.filter(|_| frame != 0);
        if *sprite == 0 && atlas.is_some() {
            *sprite = create_overlay_sprite(layout, parent);
        }
        if *sprite == 0 {
            return;
        }
        let size = atlas.and_then(|atlas| atlas.paint_meter_sprite(*sprite, frame));
        if let Some(size) = size {
            let corner = [(WIDTH - size[0]) * 0.5, (HEIGHT - size[1]) * 0.5];
            if read::<[f32; 2]>(*sprite, 128) != corner {
                set(*sprite, 128, corner);
                set(*sprite, 84, read::<u32>(*sprite, 84) | 0x20000);
            }
            gu!(0xa4fbf4, *sprite, size.as_ptr() as u32);
        }
        let flags = read::<u32>(*sprite, 84);
        set(*sprite, 84, (flags & !1) | u32::from(size.is_some()));
    }
}

unsafe fn create_overlay_sprite(layout: u32, parent: u32) -> u32 {
    unsafe {
        // cGUIInstAnimation owns a cGUIObject tree. Attach the texture after
        // its stock polygons so the original draw traversal draws it on top.
        let root = read::<u32>(parent, 220);
        if root == 0 {
            return 0;
        }
        let create: unsafe extern "C" fn() -> u32 =
            core::mem::transmute(forge::mem::text_addr() + 0xa4f550);
        let sprite = create();
        if sprite == 0 {
            return 0;
        }
        set(sprite, 116, layout);
        set(sprite, 120, read::<u32>(root, 120));
        set(
            sprite,
            84,
            (read::<u32>(sprite, 84) & !0x400000) | (read::<u32>(root, 84) & 0x400000),
        );
        let mut child = read::<u32>(root, 104);
        if child == 0 {
            gu!(0xa48460, root, sprite);
        } else {
            loop {
                let next = read::<u32>(child, 108);
                if next == 0 {
                    break;
                }
                child = next;
            }
            gu!(0xa4844c, child, sprite);
        }
        sprite
    }
}

unsafe fn rectangle(
    layout: u32,
    index: u32,
    part: u32,
    node: u32,
    corner: [f32; 2],
    size: [f32; 2],
    color: u32,
) {
    unsafe {
        // The two stock vital polygons can have different anchors.
        let align = read::<u32>(node, 160);
        if align & 15 != 0 {
            set(node, 160, align & !15);
            set(node, 84, read::<u32>(node, 84) | 0x20000);
        }
        gu!(0x545d60, layout, index, part, corner.as_ptr() as u32);
        gu!(0xa4e214, node, size.as_ptr() as u32);
        gu!(0xa4e074, node, &color as *const u32 as u32);
    }
}

unsafe fn update_meter(layout: u32, sample: Option<Meter>) {
    unsafe {
        let root = group(layout, HUD_ROOT);
        let sharpness = group(layout, SHARPNESS);
        if root == 0 || sharpness == 0 {
            return;
        }
        let origin = read::<[f32; 2]>(sharpness, 144);
        if !origin.iter().all(|v| v.is_finite()) {
            return;
        }
        METER.with(|state| {
            let count = sample
                .filter(|m| m.units_per_segment != 0)
                .map_or(0, |m| usize::from(m.segments).min(MAX_METER_SEGMENTS));
            if state.owner != layout || state.root != root {
                if count == 0 {
                    return;
                }
                *state = MeterLayout::new(layout, root);
            }
            for (i, slot) in state.groups.iter_mut().enumerate() {
                let track = i
                    .checked_sub(TRACK_START)
                    .and_then(|column| sample?.tracks.get(column).copied())
                    .filter(|track| count != 0 && track.color != 0);
                let show = i < count
                    || (i == SIGNAL_INDEX
                        && count != 0
                        && sample.is_some_and(|m| !matches!(m.signal, Signal::None)))
                    || track.is_some();
                if slot.is_some_and(|g| group(layout, g.index) != g.node) {
                    *slot = None;
                }
                if show && slot.is_none() {
                    *slot = MeterGroup::create(layout);
                }
                let Some(group) = slot else {
                    continue;
                };
                let MeterGroup { index, node, .. } = *group;
                let Some(meter) = sample.filter(|_| show) else {
                    gu!(0x54533c, layout, index, 0);
                    continue;
                };
                let members =
                    3 + u32::from(group.refill_sprite != 0) + u32::from(group.spend_sprite != 0);
                let Some([front, back]) = polygons(node, members) else {
                    gu!(0x54533c, layout, index, 0);
                    continue;
                };
                let (column, y) = if i >= TRACK_START {
                    (i - TRACK_START, 14.)
                } else {
                    (i.min(count), 3.)
                };
                let xy = [origin[0] + 56. + STEP * column as f32, origin[1] + y];
                gu!(0x545bcc, layout, index, xy.as_ptr() as u32);
                if i == SIGNAL_INDEX {
                    let far = matches!(meter.signal, Signal::Far);
                    rectangle(layout, index, 1, front, [1., 2.], [4., 4.], TARGET);
                    rectangle(
                        layout,
                        index,
                        2,
                        back,
                        [7., 2.],
                        [4., 4.],
                        if far { TARGET } else { 0 },
                    );
                } else {
                    let (fill, height, color) = if let Some(track) = track {
                        (
                            WIDTH * f32::from(track.percent.min(100)) / 100.,
                            4.,
                            track.color,
                        )
                    } else {
                        let units = u32::from(meter.units_per_segment);
                        let charge = u32::from(meter.charge).saturating_sub(i as u32 * units);
                        (
                            WIDTH * charge.min(units) as f32 / units as f32,
                            HEIGHT,
                            CHARGE,
                        )
                    };
                    rectangle(layout, index, 1, front, [0., 0.], [fill, height], color);
                    rectangle(
                        layout,
                        index,
                        2,
                        back,
                        [fill, 0.],
                        [WIDTH - fill, height],
                        TRACK,
                    );
                    if i < count {
                        let overlay = meter.overlay;
                        let refill = if fill == WIDTH { overlay.refill[i] } else { 0 };
                        overlay_sprite(
                            layout,
                            node,
                            &mut group.refill_sprite,
                            overlay.atlas,
                            refill,
                        );
                        if i == 0 {
                            overlay_sprite(
                                layout,
                                node,
                                &mut group.spend_sprite,
                                overlay.atlas,
                                overlay.spend,
                            );
                        }
                    }
                }
                gu!(0x54533c, layout, index, 1);
            }
        });
    }
}

extern "C" fn bind_meter(id: u32, sample: SampleMeter) -> bool {
    let Some(entry) = style::registered_entry(id).filter(|entry| entry.kind.is_some()) else {
        return false;
    };
    entry.meter.store(sample as *mut (), Ordering::Release);
    true
}

#[forge::hook(offset = 0x544cc8)]
unsafe fn update(layout: u32) -> u32 {
    let role = unsafe { layout_role(layout) };
    if role == LayoutRole::Status {
        let actor = unsafe { local_hunter() };
        let sample = unsafe { style::local_entry(actor) }.and_then(|entry| {
            let callback = entry.meter.load(Ordering::Acquire);
            (!callback.is_null()).then(|| {
                let sample: SampleMeter = unsafe { core::mem::transmute(callback) };
                sample(actor)
            })
        });
        unsafe { update_meter(layout, sample) };
    }
    let out = original!(layout);
    if role == LayoutRole::Weapon {
        // The render callback only needs the selection, never a hunter pointer.
        let extended =
            unsafe { style::local_entry(local_hunter()) }.is_some_and(|entry| entry.kind.is_some());
        EXTENDED_WEAPON.store(extended, Ordering::Release);
    }
    out
}

#[forge::hook(offset = 0x544db8)]
unsafe fn draw(layout: u32, context: u32) -> u32 {
    if EXTENDED_WEAPON.load(Ordering::Acquire)
        && unsafe { layout_role(layout) } == LayoutRole::Weapon
    {
        return 0;
    }
    original!(layout, context)
}

pub(super) fn install(base: u32) {
    static SERVICE: MeterService = MeterService(bind_meter);
    unsafe { hud::publish_meter(&SERVICE) };
    forge::install_hook!(base, update);
    forge::install_hook!(base, draw);
}
