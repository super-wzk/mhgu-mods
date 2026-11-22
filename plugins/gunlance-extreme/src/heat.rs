//! Disable stock heat mechanics and reuse GU's gunlance presentation for ignition.
use crate::{battle, native, read, write};
use mhgu_sdk::{style::hunter, sync::Shared};

const HEAT: u32 = 13152;
const HEAT_LEVEL: u32 = 13164;
const HEAT_OVERRIDE: u32 = 13168;
const COLOR_BLUE: u32 = 0x00f3_a84c;
const COLOR_ORANGE: u32 = 0x000a_6ef5;
static HUD: Shared<Option<Overlay>> = Shared::new(None);

#[forge::hook(offset = 0x11aa480)]
unsafe fn add_heat(actor: u32, amount: i32, shell: u32) -> u32 {
    if unsafe { crate::active(actor) } {
        return 0;
    }
    original!(actor, amount, shell)
}

#[forge::hook(offset = 0x11aa34c)]
unsafe fn update_heat(actor: u32) -> u32 {
    if unsafe { crate::active(actor) } {
        // Keep GU's Wyvern Fire timer at +13096.
        for offset in [HEAT, HEAT_LEVEL, HEAT_OVERRIDE] {
            unsafe { write(actor, offset, 0u32) };
        }
    }
    original!(actor)
}

#[derive(Clone, Copy)]
struct Visibility {
    node: u32,
    bits: u32,
}

impl Visibility {
    unsafe fn capture(node: u32) -> Self {
        Self {
            node,
            bits: unsafe { read::<u32>(node, 84) } & 3,
        }
    }

    unsafe fn set(self, bits: u32) {
        unsafe { write(self.node, 84, (read::<u32>(self.node, 84) & !3) | bits) };
    }
}

unsafe fn hud_group(layout: u32, index: u32) -> Option<u32> {
    unsafe {
        let stock = read::<u32>(layout, 664);
        let table = if index < stock {
            read::<u32>(layout, 248)
        } else if index - stock < read::<u32>(layout, 648) {
            read::<u32>(layout, 660)
        } else {
            return None;
        };
        if table == 0 {
            return None;
        }
        let node = native!(0x545a78, layout, index);
        (node != 0).then_some(node)
    }
}

unsafe fn heat_nodes(widget: u32) -> Option<[u32; 4]> {
    unsafe {
        let layout: u32 = read(widget, 36);
        if layout == 0 {
            return None;
        }
        let gauge = hud_group(layout, 1)?;
        let marker = hud_group(layout, 2)?;
        let members: u32 = read(gauge, 216);
        if members == 0 || native!(0xa38ffc, gauge) != 4 {
            return None;
        }
        // hud9_g_lance group 1: root, Pol_red, Pol_org, Pol_yel.
        let polygons: [u32; 3] = read(members, 4);
        if !polygons.iter().all(|&node| {
            node != 0
                && matches!(
                    read::<u32>(node, 0).wrapping_sub(forge::mem::text_addr()),
                    0x176c744 | 0x178fc48
                )
        }) {
            return None;
        }
        let [red, orange, yellow] = polygons;
        Some([yellow, red, orange, marker])
    }
}

unsafe fn set_fill(node: u32, size: [f32; 2], colors: [u32; 4]) {
    unsafe {
        native!(0xa4e214, node, size.as_ptr() as u32);
        for (index, color) in colors.iter().enumerate() {
            native!(0xa4e0b8, node, color as *const u32 as u32, index as u32);
        }
    }
}

struct Overlay {
    nodes: [u32; 4],
    visibility: [Visibility; 4],
    size: [f32; 2],
    colors: [u32; 4],
}

impl Overlay {
    unsafe fn apply(nodes: [u32; 4], full_width: f32, (active, progress): (bool, f32)) -> Self {
        unsafe {
            let fill = nodes[0];
            let saved = Self {
                nodes,
                visibility: nodes.map(|node| Visibility::capture(node)),
                size: read(fill, 180),
                colors: read(fill, 164),
            };
            let rgb = if active { COLOR_ORANGE } else { COLOR_BLUE };
            set_fill(
                fill,
                [full_width * progress, saved.size[1]],
                saved.colors.map(|color| (color & 0xff00_0000) | rgb),
            );
            let [fill, red, orange, marker] = saved.visibility;
            fill.set(if progress > 0. { fill.bits } else { 0 });
            red.set(0);
            orange.set(0);
            marker.set(0);
            saved
        }
    }

    unsafe fn restore(self, live: Option<[u32; 4]>) {
        if live == Some(self.nodes) {
            unsafe {
                set_fill(self.nodes[0], self.size, self.colors);
                for node in self.visibility {
                    node.set(node.bits);
                }
            }
        }
    }
}

#[forge::hook(offset = 0x6487cc)]
unsafe fn heat_hud(widget: u32) -> u32 {
    if let Some(previous) = HUD.with(Option::take) {
        unsafe { previous.restore(heat_nodes(widget)) };
    }
    let result = original!(widget);
    unsafe {
        // GU 6485F0 caches the full width before 54F828 sizes the three color bands.
        let full_width = read::<i32>(widget, 100);
        if full_width > 0
            && let Some(actor) = hunter::local().filter(|&actor| crate::active(actor))
            && let Some(nodes) = heat_nodes(widget)
        {
            let meter = battle::ignition_hud(actor, read(actor, 5060));
            let overlay = Overlay::apply(nodes, full_width as f32, meter);
            HUD.with(|saved| *saved = Some(overlay));
        }
    }
    result
}

pub(super) fn install() {
    let base = forge::mem::text_addr();
    forge::install_hook!(base, add_heat);
    forge::install_hook!(base, update_heat);
    forge::install_hook!(base, heat_hud);
}
