//! Install an extended weapon's meter in GU's quest HUD.
use super::{gu, local_hunter, read, set, style};
use core::sync::atomic::{AtomicBool, Ordering};
use mhgu_sdk::{
    sync::Shared,
    weapon::style::hud::{
        self, GaugePlate, MAX_METER_SEGMENTS, METER_SPRITES, Meter, MeterLabel, MeterPanel,
        MeterService, MeterSprite, SampleMeter, Signal,
    },
};

static EXTENDED_WEAPON: AtomicBool = AtomicBool::new(false);

const VITAL_WIDGET: u32 = 0x1777d58;
const SHARPNESS: u32 = 5; // hud0.gui: Anm_sharp.
const HUD_ROOT: u32 = 10; // hud0.gui: Nul_hud0.
const BAR_TEMPLATE: u32 = 5; // hud0.gui: vital_gauge, root and two polygons.
const LABEL_TEMPLATE: u32 = 16; // hud0.gui: pl_name, root and Mes_plname.
const PANEL_CENTER: f32 = 640.; // hud0.gui uses a 1280 by 720 canvas.
const PANEL_WIDTH: f32 = 224.;
const PANEL_LABEL_Y: f32 = 308.;
const PANEL_TRACK_Y: f32 = 337.;
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
    panel: PanelLayout,
    sprites: [Option<(HudGroup, u32)>; METER_SPRITES],
}

impl MeterLayout {
    const fn new(owner: u32, root: u32) -> Self {
        Self {
            owner,
            root,
            groups: [None; TRACK_START + 2],
            panel: PanelLayout::new(),
            sprites: [None; METER_SPRITES],
        }
    }
}

static METER: Shared<MeterLayout> = Shared::new(MeterLayout::new(0, 0));

#[derive(Clone, Copy)]
struct HudGroup {
    index: u32,
    node: u32,
}

impl HudGroup {
    unsafe fn create(layout: u32, template: u32) -> Option<Self> {
        unsafe {
            let index = gu!(
                0x54501c,
                layout,
                u32::MAX,
                template,
                HUD_ROOT,
                forge::mem::text_addr() + 0x2123290
            );
            let node = group(layout, index);
            (node != 0).then_some(Self { index, node })
        }
    }

    unsafe fn set_visible(self, layout: u32, visible: bool) {
        unsafe { gu!(0x54533c, layout, self.index, u32::from(visible)) };
    }

    unsafe fn position(self, layout: u32, xy: [f32; 2]) {
        unsafe { gu!(0x545bcc, layout, self.index, xy.as_ptr() as u32) };
    }
}

#[derive(Clone, Copy)]
struct MeterGroup {
    group: HudGroup,
    refill_sprite: u32,
    spend_sprite: u32,
}

struct PanelLayout {
    caption: Option<HudGroup>,
    label: Option<&'static MeterLabel>,
    tracks: [Option<HudGroup>; 2],
}

impl PanelLayout {
    const fn new() -> Self {
        Self {
            caption: None,
            label: None,
            tracks: [None; 2],
        }
    }

    unsafe fn update(&mut self, layout: u32, sample: MeterPanel) {
        unsafe {
            if self
                .caption
                .is_some_and(|g| group(layout, g.index) != g.node)
            {
                self.caption = None;
                self.label = None;
            }
            if sample.label.is_some() && self.caption.is_none() {
                self.caption = HudGroup::create(layout, LABEL_TEMPLATE);
            }
            let mut visible = false;
            if let Some(caption) = self.caption {
                if let Some(label) = sample.label
                    && let Some(message) = message(caption.node)
                {
                    if self.label.is_none_or(|old| !core::ptr::eq(old, label)) {
                        // Align the native text geometry around the panel's center.
                        let align = read::<u32>(message, 160);
                        set(message, 160, (align & !15) | 1);
                        set(message, 84, read::<u32>(message, 84) | 0x20000);
                        let corner = [0f32; 2];
                        gu!(0x545d60, layout, caption.index, 1, corner.as_ptr() as u32);
                        caption.position(layout, [PANEL_CENTER, PANEL_LABEL_Y]);
                        let text = label.text();
                        gu!(
                            0xa537d0,
                            message,
                            text.as_ptr() as u32,
                            text.to_bytes().len() as u32
                        );
                        self.label = Some(label);
                    }
                    visible = true;
                }
                caption.set_visible(layout, visible);
            }
            for (row, slot) in self.tracks.iter_mut().enumerate() {
                if slot.is_some_and(|g| group(layout, g.index) != g.node) {
                    *slot = None;
                }
                let track = sample.tracks[row];
                let show = visible && track.color != 0;
                if show && slot.is_none() {
                    *slot = HudGroup::create(layout, BAR_TEMPLATE);
                }
                let Some(bar) = *slot else { continue };
                let polygons = show.then(|| polygons(bar.node, 3)).flatten();
                if let Some(polygons) = polygons {
                    let width = PANEL_WIDTH * f32::from(track.percent.min(100)) / 100.;
                    bar.position(
                        layout,
                        [
                            PANEL_CENTER - PANEL_WIDTH / 2.,
                            PANEL_TRACK_Y + 12. * row as f32,
                        ],
                    );
                    progress_bar(
                        layout,
                        bar.index,
                        polygons,
                        PANEL_WIDTH,
                        width,
                        HEIGHT,
                        track.color,
                    );
                }
                bar.set_visible(layout, polygons.is_some());
            }
        }
    }
}

unsafe fn message(node: u32) -> Option<u32> {
    unsafe {
        if read::<u32>(node, 212) != LABEL_TEMPLATE || gu!(0xa38ffc, node) != 2 {
            return None;
        }
        let members = read::<u32>(node, 216);
        if members == 0 {
            return None;
        }
        let child = read::<u32>(members, 4);
        (child != 0
            && matches!(
                read::<u32>(child, 0).wrapping_sub(forge::mem::text_addr()),
                0x178fdc8 | 0x178fe58 // cGUIObjMessage / cGUIObjText
            ))
        .then_some(child)
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
        let size = atlas.and_then(|atlas| atlas.paint_meter_sprite(*sprite, u16::from(frame)));
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

unsafe fn progress_bar(
    layout: u32,
    index: u32,
    [front, back]: [u32; 2],
    width: f32,
    fill: f32,
    height: f32,
    color: u32,
) {
    unsafe {
        rectangle(layout, index, 1, front, [0., 0.], [fill, height], color);
        rectangle(
            layout,
            index,
            2,
            back,
            [fill, 0.],
            [width - fill, height],
            TRACK,
        );
    }
}

unsafe fn update_artwork(
    layout: u32,
    origin: [f32; 2],
    slot: &mut Option<(HudGroup, u32)>,
    sample: Option<MeterSprite>,
) {
    unsafe {
        if slot.is_some_and(|(g, _)| group(layout, g.index) != g.node) {
            *slot = None;
        }
        if sample.is_some() && slot.is_none() {
            let Some(group) = HudGroup::create(layout, BAR_TEMPLATE) else {
                return;
            };
            *slot = Some((group, 0));
        }
        let Some((group, sprite)) = slot else { return };
        let Some(sample) = sample else {
            group.set_visible(layout, false);
            return;
        };
        if *sprite == 0 {
            if let Some(bars) = polygons(group.node, 3) {
                for bar in bars {
                    set(bar, 84, read::<u32>(bar, 84) & !1);
                }
            }
            *sprite = create_overlay_sprite(layout, group.node);
        }
        let size = (*sprite != 0)
            .then(|| sample.atlas?.paint_meter_sprite(*sprite, sample.frame))
            .flatten();
        group.set_visible(layout, size.is_some());
        if let Some(size) = size {
            let origin = if sample.centered {
                [640., 360.]
            } else {
                origin
            };
            group.position(
                layout,
                [origin[0] + sample.offset[0], origin[1] + sample.offset[1]],
            );
            let size = [size[0] * sample.scale[0], size[1] * sample.scale[1]];
            gu!(0xa4fbf4, *sprite, size.as_ptr() as u32);
            set(*sprite, 84, read::<u32>(*sprite, 84) | 1);
        }
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
            let artwork = sample.is_some_and(|meter| meter.sprites[0].atlas.is_some());
            for (index, slot) in state.sprites.iter_mut().enumerate() {
                update_artwork(
                    layout,
                    origin,
                    slot,
                    sample
                        .map(|meter| meter.sprites[index])
                        .filter(|sprite| sprite.atlas.is_some()),
                );
            }
            let count = if artwork { 0 } else { count };
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
                if slot.is_some_and(|m| group(layout, m.group.index) != m.group.node) {
                    *slot = None;
                }
                if show && slot.is_none() {
                    *slot = HudGroup::create(layout, BAR_TEMPLATE).map(|group| MeterGroup {
                        group,
                        refill_sprite: 0,
                        spend_sprite: 0,
                    });
                }
                let Some(group) = slot else {
                    continue;
                };
                let HudGroup { index, node } = group.group;
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
                    progress_bar(layout, index, [front, back], WIDTH, fill, height, color);
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
            let panel = sample.map_or_else(MeterPanel::default, |meter| meter.panel);
            state.panel.update(layout, panel);
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
