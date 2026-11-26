use crate::{battle, native, program, read, write};
use mhfz_sdk::sync::Shared;
use mhgu_sdk::weapon::style::hud::GaugePlate;

const BASE: u32 = 0x4000_0400;
const fn plate(id: u32) -> GaugePlate {
    GaugePlate::new(id, cr"GUI\frontier_sa\gauge", [0.; 4]).with_cells(16, [192, 32])
}
static PLATES: [GaugePlate; 2] = [plate(BASE), plate(BASE + 1)];

pub(super) unsafe fn preload() {
    for plate in &PLATES {
        unsafe { plate.preload() };
    }
}

fn pointer(value: u32) -> Option<u32> {
    (value != 0 && value & 3 == 0).then_some(value)
}

unsafe fn group(layout: u32, index: u32) -> Option<u32> {
    unsafe {
        let stock = read::<u32>(layout, 664);
        let (table, slot) = if index < stock {
            (248, index)
        } else if index - stock < read::<u32>(layout, 648) {
            (660, index - stock)
        } else {
            return None;
        };
        pointer(read(pointer(read(layout, table))?, slot * 4))
    }
}

#[derive(Clone, Copy)]
struct Shape {
    node: u32,
    size: [f32; 2],
}

impl Shape {
    unsafe fn capture(node: u32) -> Self {
        Self {
            node,
            size: unsafe { read(node, 180) },
        }
    }

    unsafe fn resize(self, size: [f32; 2]) {
        unsafe { native!(0xa4e214, self.node, size.as_ptr() as u32) };
    }
}

#[derive(Clone, Copy)]
struct Gauge {
    sprite: Shape,
    crop: [f32; 4],
    position: [f32; 2],
    polygons: [Shape; 2],
}

unsafe fn members(group: u32) -> Option<[u32; 3]> {
    // hud11_s_axe.gui groups 0 and 1 have a root followed by three shapes.
    unsafe { Some(read(pointer(read(group, 216))?, 4)) }
}

unsafe fn is_texture(node: u32) -> bool {
    unsafe {
        matches!(
            read::<u32>(node, 0).wrapping_sub(forge::mem::text_addr()),
            0x178fcc8 | 0x176c7c4
        )
    }
}

unsafe fn is_polygon(node: u32) -> bool {
    unsafe {
        matches!(
            read::<u32>(node, 0).wrapping_sub(forge::mem::text_addr()),
            0x178fc48 | 0x176c744
        )
    }
}

impl Gauge {
    unsafe fn capture(group: u32) -> Option<Self> {
        unsafe {
            let nodes = members(group)?;
            let mut texture = None;
            let mut polygons = [0; 2];
            let mut polygon_slots = polygons.iter_mut();
            for node in nodes {
                let node = pointer(node)?;
                if is_polygon(node) {
                    *polygon_slots.next()? = node;
                } else if is_texture(node) && texture.is_none() {
                    texture = Some(node);
                } else {
                    return None;
                }
            }
            let sprite = texture?;
            // During quest entry the texture node can exist before its stock
            // descriptor is bound. GU needs both pointers to restore the skin.
            if polygons[0] == polygons[1]
                || pointer(read(sprite, 120)).is_none()
                || pointer(read(sprite, 260)).is_none()
                || (BASE..=BASE + 1).contains(&read::<u32>(sprite, 256))
            {
                return None;
            }
            Some(Self {
                sprite: Shape::capture(sprite),
                crop: read(sprite, 268),
                position: read(sprite, 128),
                polygons: polygons.map(|p| Shape::capture(p)),
            })
        }
    }

    unsafe fn set_position(&self, xy: [f32; 2]) {
        unsafe {
            write(self.sprite.node, 128, xy);
            write(
                self.sprite.node,
                84,
                read::<u32>(self.sprite.node, 84) | 0x20000,
            );
        }
    }

    unsafe fn restore(self, index: usize, group: u32) {
        unsafe {
            let Some(live) = members(group) else { return };
            let node = self.sprite.node;
            if live.contains(&node)
                && is_texture(node)
                && read::<u32>(node, 256) == BASE + index as u32
            {
                PLATES[index].show_sprite(node, None, self.crop);
                self.set_position(self.position);
                self.sprite.resize(self.sprite.size);
            }
            for p in self.polygons {
                if live.contains(&p.node) && is_polygon(p.node) {
                    p.resize(p.size);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Visibility {
    group: u32,
    bits: u32,
}

impl Visibility {
    unsafe fn capture(group: u32) -> Self {
        Self {
            group,
            bits: unsafe { read::<u32>(group, 84) } & 3,
        }
    }

    unsafe fn set(self, bits: u32) {
        // The native hide helper recursively resets the animation's child list.
        // Only borrow its two visibility bits until the next native HUD update.
        unsafe { write(self.group, 84, (read::<u32>(self.group, 84) & !3) | bits) }
    }
}

struct Overlay {
    layout: u32,
    groups: [Visibility; 3],
    gauges: [Gauge; 2],
}

impl Overlay {
    unsafe fn show(layout: u32, frames: [u32; 2]) -> Option<Self> {
        unsafe {
            let groups = [group(layout, 0)?, group(layout, 1)?, group(layout, 2)?];
            let gauges = [Gauge::capture(groups[0])?, Gauge::capture(groups[1])?];
            if groups[0] == groups[1]
                || groups[..2].contains(&groups[2])
                || gauges[0].sprite.node == gauges[1].sprite.node
            {
                return None;
            }
            let groups = groups.map(|g| Visibility::capture(g));
            for (i, gauge) in gauges.iter().enumerate() {
                PLATES[i].show_sprite(gauge.sprite.node, Some(frames[i]), gauge.crop);
                if read::<u32>(gauge.sprite.node, 256) != BASE + i as u32 {
                    if i == 1 {
                        PLATES[0].show_sprite(gauges[0].sprite.node, None, gauges[0].crop);
                    }
                    return None;
                }
            }
            for (i, gauge) in gauges.iter().enumerate() {
                let mut xy = gauge.position;
                xy[0] += 104. * i as f32;
                gauge.set_position(xy);
                gauge.sprite.resize([192., 32.]);
                for p in gauge.polygons {
                    p.resize([0., 0.]);
                }
                groups[i].set(3);
            }
            groups[2].set(0);
            Some(Self {
                layout,
                groups,
                gauges,
            })
        }
    }

    unsafe fn restore(self, layout: u32) {
        if layout != self.layout {
            return;
        }
        unsafe {
            for (i, saved) in self.groups.into_iter().enumerate() {
                if group(layout, i as u32) == Some(saved.group) {
                    saved.set(saved.bits);
                    if let Some(gauge) = self.gauges.get(i) {
                        gauge.restore(i, saved.group);
                    }
                }
            }
        }
    }
}

static OVERLAY: Shared<Option<Overlay>> = Shared::new(None);

#[forge::hook(offset = 0x64aefc)]
unsafe fn draw(widget: u32) -> u32 {
    unsafe {
        if let Some(previous) = OVERLAY.with(Option::take) {
            previous.restore(read(widget, 36));
        }
        let result = original!(widget);
        if let (Some((slash, light, unlocked)), Some(layout)) =
            (battle::hud_gauges(), pointer(read(widget, 36)))
        {
            let light_max = u32::from(program::param(18));
            let stage = if unlocked {
                3
            } else if u32::from(light) >= light_max {
                2
            } else {
                1
            };
            let frames = [
                100 * u32::from(slash) / u32::from(program::param(15)),
                101 * stage + 100 * u32::from(light) / light_max,
            ];
            let next = Overlay::show(layout, frames);
            OVERLAY.with(|saved| *saved = next);
        }
        result
    }
}

pub fn install() {
    forge::install_hook!(forge::mem::text_addr(), draw);
}
