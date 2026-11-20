//! Frontier's cut and spirit gauges on GU's long-sword widget.
use crate::{battle, local_actor, native, read, write};
use mhfz_sdk::sync::Shared;
use mhgu_sdk::style::hud::{GaugePlate, weapon_children};

const ID: u32 = 0x4000_0003;
const FLIP_Y: u32 = 1 << 22;
static PLATE: GaugePlate = GaugePlate::new(ID, c"GUI\\frontier_ls\\gauge", [368., 32., 496., 56.])
    .with_cells(16, [192, 32]);

fn frame(cut: u32, tick: u32, flash: u32) -> u32 {
    if cut < crate::program::CUT_MAX {
        return 60 * cut / crate::program::CUT_MAX;
    }
    let t = tick % 30;
    60 + 16 * flash.min(23) + t.min(30 - t)
}

#[derive(Clone, Copy)]
struct Rect {
    position: [f32; 2],
    size: [f32; 2],
    align: u32,
}

impl Rect {
    unsafe fn read(node: u32) -> Self {
        unsafe {
            Self {
                position: read(node, 128),
                size: read(node, 180),
                align: read::<u32>(node, 160) & 15,
            }
        }
    }

    unsafe fn apply(self, node: u32) {
        unsafe {
            if read::<[f32; 2]>(node, 128) != self.position
                || read::<u32>(node, 160) & 15 != self.align
            {
                write(node, 128, self.position);
                write(node, 160, (read::<u32>(node, 160) & !15) | self.align);
                write(node, 84, read::<u32>(node, 84) | 0x20000);
            }
            native!(0xa4e214, node, self.size.as_ptr() as u32);
        }
    }

    fn expanded(self, y_sign: f32) -> Self {
        let [w, h] = self.size;
        Self {
            position: [
                self.position[0] - (0.25 + 0.5 * (self.align & 3) as f32) * w,
                self.position[1] - y_sign * (1. / 6. + 0.5 * (self.align >> 2) as f32) * h,
            ],
            size: [w * 1.5, h * (4. / 3.)],
            align: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Nodes {
    plate: u32,
    polygons: [u32; 2],
    maximum: u32,
}

impl Nodes {
    unsafe fn read(layout: u32) -> Option<Self> {
        unsafe {
            let children = weapon_children(layout)?;
            let [plate, fill, background] = read::<[u32; 3]>(children, 4);
            if [plate, fill, background].contains(&0) {
                return None;
            }
            // Group 2 is the native maximum overlay (64B414).
            let maximum = native!(0x545a78, layout, 2);
            if maximum == 0 {
                return None;
            }
            let members = read::<u32>(maximum, 216);
            if members == 0 {
                return None;
            }
            // hud5_l_sword group 2: root, ColorAdjust, Txt_max.
            let maximum = read::<u32>(members, 8);
            (maximum != 0).then_some(Self {
                plate,
                polygons: [fill, background],
                maximum,
            })
        }
    }
}

#[derive(Clone, Copy)]
struct Polygon {
    rect: Rect,
    colors: [u32; 4],
}

unsafe fn set_colors(node: u32, colors: [u32; 4]) {
    unsafe {
        if read::<[u32; 4]>(node, 164) != colors {
            write(node, 164, colors);
            native!(0x54868c, node); // Mark native vertex buffers dirty.
        }
    }
}

struct Gauge {
    nodes: Nodes,
    stock: Rect,
    polygons: [Polygon; 2],
    maximum_size: [f32; 2],
}

impl Gauge {
    unsafe fn restore(&self) {
        unsafe {
            self.stock.apply(self.nodes.plate);
            for (node, original) in self.nodes.polygons.into_iter().zip(self.polygons) {
                original.rect.apply(node);
                set_colors(node, original.colors);
            }
            native!(
                0xa4e214,
                self.nodes.maximum,
                self.maximum_size.as_ptr() as u32
            );
        }
    }
}

static GAUGE: Shared<Option<Gauge>> = Shared::new(None);

#[forge::hook(offset = 0x64b3d8)]
unsafe fn gauges(widget: u32) -> u32 {
    unsafe {
        let layout = read(widget, 36);
        if let Some(old) = GAUGE.with(Option::take)
            && Nodes::read(layout) == Some(old.nodes)
            && read::<u32>(old.nodes.plate, 256) == ID
        {
            old.restore();
        }
        let actor = local_actor();
        if actor.is_none() {
            PLATE.show(layout, None);
        }
        let result = original!(widget);
        let Some(actor) = actor else { return result };
        let layout = read(widget, 36);
        let Some(nodes) = Nodes::read(layout) else {
            return result;
        };
        let (cut, tick, flash, enhanced) = battle::hud_state(actor);
        PLATE.show(layout, Some(frame(cut, tick, flash)));
        if read::<u32>(nodes.plate, 256) != ID {
            return result;
        }
        let gauge = Gauge {
            nodes,
            stock: Rect::read(nodes.plate),
            polygons: nodes.polygons.map(|node| Polygon {
                rect: Rect::read(node),
                colors: read(node, 164),
            }),
            maximum_size: read(nodes.maximum, 180),
        };
        let sign = if read::<u32>(nodes.plate, 84) & FLIP_Y == 0 {
            1.
        } else {
            -1.
        };
        let plate = gauge.stock.expanded(sign);
        plate.apply(nodes.plate);

        // Group 0's three siblings have the same parent, rotation and reference
        // scale. Tile pixels [47,143) x [5,7) are the opening in sprite 92.
        let [sx, sy] = [plate.size[0] / 192., plate.size[1] / 32.];
        let spirit = read::<i32>(actor, 10112).clamp(0, 100);
        let filled = (120 * spirit / 100) as f32 * 0.8;
        let t = if enhanced { tick % 20 } else { 0 };
        let c = 32
            + if t < 4 {
                223 * t / 4
            } else {
                223 * (20 - t) / 16
            };
        let red = u32::from_le_bytes([255, c as u8, c as u8, 0]);
        for ((node, original), (start, width, rgb)) in nodes
            .polygons
            .into_iter()
            .zip(gauge.polygons)
            .zip([(0., filled, red), (filled, 96. - filled, 0x0e0e0e)])
        {
            Rect {
                position: [
                    plate.position[0] + (47. + start) * sx,
                    plate.position[1] + sign * 5. * sy,
                ],
                size: [width * sx, 2. * sy],
                align: 0,
            }
            .apply(node);
            set_colors(
                node,
                original.colors.map(|color| (color & 0xff000000) | rgb),
            );
        }
        native!(0xa4e214, nodes.maximum, [0f32; 2].as_ptr() as u32);
        GAUGE.with(|state| *state = Some(gauge));
        result
    }
}

pub(super) fn install() {
    forge::install_hook!(forge::mem::text_addr(), gauges);
}
