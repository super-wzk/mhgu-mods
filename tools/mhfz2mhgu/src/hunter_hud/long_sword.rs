//! Composite w07's retail quest-HUD sprites and keyed sequences into GU atlas cells.
use anyhow::{Context, Result, ensure};
use mhf_resource::{
    image::{self, Image},
    pac,
};
use std::path::Path;

const W: usize = 192;
const H: usize = 32;
const COLS: usize = 16;
const SCALE: f32 = 0.8;
const ATLAS_H: usize = 1024;

fn bytes<const N: usize>(data: &[u8], at: usize) -> Result<[u8; N]> {
    Ok(data
        .get(at..at + N)
        .context("short HUD record")?
        .try_into()?)
}
fn half(data: &[u8], at: usize) -> Result<usize> {
    Ok(u16::from_le_bytes(bytes(data, at)?) as usize)
}
fn word(data: &[u8], at: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(bytes(data, at)?) as usize)
}
fn section(data: &[u8], index: usize) -> Result<&[u8]> {
    let start = word(data, 16 + 4 * index)?;
    data.get(start..start + word(data, start + 4)?)
        .context("short HUD section")
}

#[derive(Clone, Copy)]
struct Pose {
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    alpha: f32,
}

fn composite(dst: &mut [u8], src: &[u8], color: [u8; 4], opacity: f32, add: bool) {
    let a = src[3] as f32 / 255. * color[3] as f32 / 255. * opacity;
    let d = dst[3] as f32 / 255.;
    let total = a + d * (1. - a);
    if total <= 0. {
        return;
    }
    for c in 0..3 {
        let ink = src[c] as f32 * color[c] as f32 / 255.;
        dst[c] =
            ((ink * a + dst[c] as f32 * d * if add { 1. } else { 1. - a }) / total).round() as u8;
    }
    dst[3] = (255. * total).round() as u8;
}

struct Sprites<'a> {
    layout: &'a [u8],
    texture: Image,
}

impl Sprites<'_> {
    fn paint(
        &self,
        tile: &mut [u8],
        id: usize,
        part: Option<usize>,
        pose: Pose,
        tint: u8,
    ) -> Result<()> {
        let s = section(self.layout, 0)?;
        ensure!(id < half(s, 12)?, "missing HUD sprite {id}");
        let row = half(s, word(s, 8)? + 2 * id)?;
        let count = half(s, row)?;
        let parts = s
            .get(row + 4..row + 4 + 28 * count)
            .context("short HUD sprite")?;
        for (index, quad) in parts.chunks_exact(28).enumerate() {
            if part.is_some_and(|p| p != index) {
                continue;
            }
            let value = |n| i16::from_le_bytes([quad[n], quad[n + 1]]);
            let (u, v, sw, sh) = (value(8), value(10), value(12), value(14));
            let (dx, dy, w, h) = (value(16), value(18), value(20) as f32, value(22) as f32);
            ensure!(
                u >= 0
                    && v >= 0
                    && sw > 0
                    && sh > 0
                    && (u + sw) as u32 <= self.texture.width
                    && (v + sh) as u32 <= self.texture.height,
                "invalid HUD sprite crop"
            );
            let flag = half(quad, 0)?;
            ensure!(
                flag & !0x300 == 0 && half(quad, 2)? == 0,
                "unsupported HUD texture flags"
            );
            let angle = value(24);
            ensure!(matches!(angle, 0 | 16384), "unsupported HUD rotation");
            let ink = u32::from_le_bytes(bytes(quad, 4)?);
            let [b, g, r, a] = ink.to_le_bytes();
            let color = [r, g, b].map(|c| (u16::from(c) * u16::from(tint) / 255) as u8);
            let color = [color[0], color[1], color[2], a];
            if pose.sx <= 0. || pose.sy <= 0. {
                continue;
            }
            // The sprite-local origin is its top-left corner. Quarter turns
            // on the reticle and guard rotate about that origin.
            let (xmin, xmax, ymin, ymax) = if angle == 0 {
                (0., w, 0., h)
            } else {
                (-h, 0., 0., w)
            };
            let x = pose.x + dx as f32 * pose.sx;
            let y = pose.y + dy as f32 * pose.sy;
            for py in ((y + ymin * pose.sy).floor().max(0.) as usize)
                ..((y + ymax * pose.sy).ceil() as usize).min(H)
            {
                for px in ((x + xmin * pose.sx).floor().max(0.) as usize)
                    ..((x + xmax * pose.sx).ceil() as usize).min(W)
                {
                    let (lx, ly) = (
                        (px as f32 + 0.5 - x) / pose.sx,
                        (py as f32 + 0.5 - y) / pose.sy,
                    );
                    let (mut tx, mut ty) = if angle == 0 {
                        (lx / w, ly / h)
                    } else {
                        (ly / w, -lx / h)
                    };
                    if !(0. ..1.).contains(&tx) || !(0. ..1.).contains(&ty) {
                        continue;
                    }
                    if flag & 256 != 0 {
                        tx = 1. - tx;
                    }
                    if flag & 512 != 0 {
                        ty = 1. - ty;
                    }
                    let ix = u as usize + ((sw as f32 * tx) as usize).min(sw as usize - 1);
                    let iy = v as usize + ((sh as f32 * ty) as usize).min(sh as usize - 1);
                    let src = (iy * self.texture.width as usize + ix) * 4;
                    let dst = (py * W + px) * 4;
                    composite(
                        &mut tile[dst..dst + 4],
                        &self.texture.rgba[src..src + 4],
                        color,
                        pose.alpha,
                        half(quad, 26)? == 1,
                    );
                }
            }
        }
        Ok(())
    }

    fn sequence(&self, tile: &mut [u8], seq: usize, frame: usize) -> Result<()> {
        self.sequence_at(tile,seq,frame,40.,15.)
    }

    fn sequence_at(&self,tile:&mut [u8],seq:usize,frame:usize,x:f32,y:f32)->Result<()> {
        let s = section(self.layout, 2)?;
        let start = half(s, word(s, 8)? + seq * 2)?;
        let nodes = word(s, 16)? + start * 20;
        let count = half(s, nodes)?;
        // These four sequences consist of one static root and sprite children.
        ensure!(
            half(s, nodes + 2)? + 1 == count && half(s, nodes + 4)? == 65535,
            "unexpected sword HUD sequence"
        );
        let mut origin = Pose {
            x,
            y,
            sx: SCALE,
            sy: SCALE,
            alpha: 1.,
        };
        for i in 0..count {
            let n = nodes + 20 * i;
            let keys = word(s, 12)? + 36 * half(s, n + 8)?;
            let num = half(s, n + 10)?;
            ensure!(num > 0, "empty HUD track");
            let mut left = keys;
            while left + 36 < keys + 36 * num && word(s, left + 36)? <= frame {
                left += 36;
            }
            let right = (left + 36).min(keys + 36 * (num - 1));
            let t = if right == left {
                0.
            } else {
                (frame - word(s, left)?) as f32 / (word(s, right)? - word(s, left)?) as f32
            };
            let lerp = |a: f32, b: f32| a + (b - a) * t;
            let sample = |off| -> Result<f32> {
                Ok(lerp(
                    f32::from_le_bytes(bytes(s, left + off)?),
                    f32::from_le_bytes(bytes(s, right + off)?),
                ))
            };
            let pose = Pose {
                x: origin.x + origin.sx * sample(8)?,
                y: origin.y + origin.sy * sample(12)?,
                sx: origin.sx * sample(16)?,
                sy: origin.sy * sample(20)?,
                alpha: origin.alpha * lerp(s[left + 7] as f32, s[right + 7] as f32) / 255.,
            };
            if i == 0 {
                origin = pose;
            } else {
                let p = half(s, n + 6)?;
                self.paint(tile, half(s, n + 4)?, (p != 65535).then_some(p), pose, 255)?;
            }
        }
        Ok(())
    }
}

pub fn convert(dat: &Path) -> Result<Vec<u8>> {
    let data = pac::decode(crate::read(&dat.join("mhf.bin"))?)?;
    let cut_layout = pac::member(&data, 20)?;
    let spirit_layout = pac::member(&data, 19)?;
    let cut = Sprites {
        layout: &cut_layout,
        texture: image::decode(&pac::member(&data, 4)?)?,
    };
    let spirit = Sprites {
        layout: &spirit_layout,
        texture: image::decode(&pac::member(&data, 2)?)?,
    };
    let mut atlas = vec![0; W * COLS * ATLAS_H * 4];
    // 60 progress frames, then 16 pulse intensities for each of 23 flash
    // frames and one steady state. The native 60-tick pulse is symmetric.
    for index in 0..60 + 24 * 16 {
        let mut tile = vec![0; W * H * 4];
        if index < 60 {
            cut.sequence(&mut tile, 46, index)?;
        } else {
            let phase = index - 60;
            cut.sequence(&mut tile, 48, 0)?;
            if phase / 16 < 23 {
                cut.sequence(&mut tile, 47, phase / 16)?;
            }
            cut.sequence(&mut tile, 49, phase % 16)?;
        }
        // Native group 3 sprites 91..93 surround a 120-unit spirit fill.
        let mut pose = Pose {
            x: 36.,
            y: 1.,
            sx: SCALE,
            sy: SCALE,
            alpha: 1.,
        };
        for id in core::iter::once(91)
            .chain(core::iter::repeat_n(92, 10))
            .chain(core::iter::once(93))
        {
            spirit.paint(&mut tile, id, None, pose, 54)?;
            pose.x += if id == 91 { 14. * SCALE } else { 12. * SCALE };
        }
        // The native GU polygons meet inside the spirit frame. Punch the
        // channel out so its fill is visible in either GUI draw-pass order.
        for y in 5..7 {
            tile[(y * W + 47) * 4..(y * W + 143) * 4].fill(0);
        }
        for (y, row) in tile.chunks_exact(W * 4).enumerate() {
            let dst = ((index / COLS * 32 + y) * W * COLS + index % COLS * W) * 4;
            atlas[dst..dst + row.len()].copy_from_slice(row);
        }
    }
    mhgu_resource::tex::rgba8_unmipped((COLS * W) as u32, ATLAS_H as u32, &atlas)
}

// The w12 extreme branch of 8121C0AC draws sequence 55 for slash energy,
// then 40/42 for the locked/ready light gauge or 41 while the stance is active.
pub fn switch_axe(dat:&Path)->Result<Vec<u8>>{
    let data=pac::decode(crate::read(&dat.join("mhf.bin"))?)?;
    let layout=pac::member(&data,20)?;
    let sprites=Sprites {layout:&layout,texture:image::decode(&pac::member(&data,4)?)?};
    let mut atlas=vec![0;W*COLS*ATLAS_H*4];
    // Each gauge has 101 independent percentage samples; the two HUD sprites
    // choose their frames separately, including during an interrupted change.
    for (group,(seq,end)) in [(55,120),(40,360),(42,360),(41,360)].into_iter().enumerate(){
        for percent in 0..=100 {
            let mut tile=vec![0;W*H*4];
            sprites.sequence_at(&mut tile,seq,percent*end/100,2.,3.)?;
            let index=group*101+percent;
            for (y,row) in tile.chunks_exact(W*4).enumerate(){
                let dst=((index/COLS*H+y)*W*COLS+index%COLS*W)*4;
                atlas[dst..dst+row.len()].copy_from_slice(row);
            }
        }
    }
    mhgu_resource::tex::rgba8_unmipped((COLS*W) as u32,ATLAS_H as u32,&atlas)
}

pub fn magnet_spike(dat: &Path) -> Result<Vec<u8>> {
    let data = pac::decode(crate::read(&dat.join("mhf.bin"))?)?;
    let layout = pac::member(&data, 20)?;
    let sprites = Sprites {
        layout: &layout,
        texture: image::decode(&pac::member(&data, 4)?)?,
    };
    // Keep the sequence artwork at the same scale as the GU meter cells.
    const CELL: usize = 36;
    const COLUMNS: usize = 2;
    const ATLAS_W: usize = CELL * COLUMNS;
    let mut atlas = vec![0; ATLAS_W * ATLAS_H * 4];
    let mut tile = vec![0; W * H * 4];
    // Frame zero hides the sprite. FZ 8122670E samples group 4 sequences
    // 0x41 (0..=30) for spending and 0x42 (0..=20) for filling a segment.
    for (sequence, first, last, x, y) in [(0x41, 1, 30, 18., 16.), (0x42, 32, 20, 9.2, 12.4)] {
        for frame in 0..=last {
            tile.fill(0);
            sprites.sequence_at(&mut tile, sequence, frame, x, y)?;
            let index = first + frame;
            for (y, row) in tile.chunks_exact(W * 4).enumerate() {
                let dst = ((index / COLUMNS * H + y) * ATLAS_W + index % COLUMNS * CELL) * 4;
                atlas[dst..dst + CELL * 4].copy_from_slice(&row[..CELL * 4]);
            }
        }
    }
    mhgu_resource::tex::rgba8_unmipped(ATLAS_W as u32, ATLAS_H as u32, &atlas)
}
