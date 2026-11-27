//! Pixel artwork for GU's 128 x 24 twin-sword HUD plate.
use anyhow::Result;

const WIDTH: usize = 128;
const HEIGHT: usize = 1024;
const PITCH: usize = 32;

const FLAME: &[&[u8]] = &[
    b"       .       ",
    b"       ox      ",
    b"      .oox     ",
    b"     .ooox     ",
    b"  . .oo+ox  .  ",
    b"  xoox++ox .o  ",
    b" .oo++*++oxoox ",
    b" xo++**+++o+ox ",
    b".o++***+*+++ox ",
    b"xo+******++oox ",
    b"xo+******++ox  ",
    b" xo++***++oox  ",
    b" .xo+++++oox   ",
    b"  .xooooox.    ",
    b"    xxxxx      ",
];

const BLADE: &[&[u8]] = &[
    b"      xxxxxx.",
    b"     o****+x ",
    b"    o***++x  ",
    b"   o**+++x   ",
    b"  o++++ox    ",
    b" xoooox.     ",
    b".xxxx.       ",
];

type Palette = [[u8; 3]; 4];

// The four weapon phases, then the full-gauge variants of the last two.
const FLAME_PALETTES: [Palette; 6] = [
    [[29, 35, 40], [55, 66, 74], [85, 98, 104], [142, 151, 145]],
    [[46, 31, 32], [95, 43, 40], [159, 61, 47], [223, 123, 69]],
    [[54, 34, 30], [153, 56, 34], [242, 119, 37], [255, 211, 102]],
    [[55, 26, 42], [158, 39, 67], [240, 80, 73], [255, 186, 108]],
    [[78, 39, 24], [213, 75, 34], [255, 178, 57], [255, 248, 188]],
    [[76, 22, 49], [212, 39, 77], [255, 123, 80], [255, 239, 190]],
];
const BLADE_LIT: Palette = [
    [24, 42, 44],
    [43, 116, 111],
    [102, 212, 180],
    [226, 255, 225],
];
const BLADE_DIM: Palette = [[21, 25, 31], [57, 63, 70], [66, 73, 80], [92, 98, 101]];

fn pixel(frame: &mut [u8], width: usize, x: usize, y: usize, [r, g, b]: [u8; 3]) {
    let i = (y * width + x) * 4;
    frame[i..i + 4].copy_from_slice(&[r, g, b, 255]);
}

fn sprite(
    frame: &mut [u8],
    width: usize,
    left: usize,
    top: usize,
    rows: &[&[u8]],
    palette: &Palette,
) {
    for (y, row) in rows.iter().enumerate() {
        for (x, shade) in row.iter().enumerate() {
            let color = match shade {
                b'.' | b'x' => palette[0],
                b'o' => palette[1],
                b'+' => palette[2],
                b'*' => palette[3],
                _ => continue,
            };
            pixel(frame, width, left + x, top + y, color);
        }
    }
}

pub fn dual_blades() -> Result<Vec<u8>> {
    let mut rgba = vec![0; WIDTH * HEIGHT * 4];
    // Columns stay fixed in GU's HUD. Rows encode the weapon state and 0..4
    // polished edges. The native fill polygon starts at (18,14), size 98 x 4.
    const FRAME_BYTES: usize = WIDTH * PITCH * 4;
    for (state, flame) in FLAME_PALETTES.iter().enumerate() {
        for level in 0..=4 {
            let offset = FRAME_BYTES * (5 * state + level);
            let frame = &mut rgba[offset..offset + FRAME_BYTES];
            for y in 12..20 {
                let inset = usize::from(y == 12 || y == 19);
                let color = match y {
                    12 => [151, 144, 125],
                    13 | 18 => [72, 68, 61],
                    _ => [25, 25, 31],
                };
                for x in 16 + inset..118 - inset {
                    pixel(frame, WIDTH, x, y, color);
                }
            }
            sprite(frame, WIDTH, 1, 4, FLAME, flame);
            for i in 0..4 {
                let palette = if i < level { &BLADE_LIT } else { &BLADE_DIM };
                sprite(frame, WIDTH, 20 + 15 * i, 4, BLADE, palette);
            }
        }
    }
    mhgu_resource::tex::rgba8_unmipped(WIDTH as u32, HEIGHT as u32, &rgba)
}

mod long_sword;
pub use long_sword::{convert as long_sword, magnet_panel, magnet_spike, switch_axe};
