//! RGBA8 (0x07) Switch TEX. Each mip is stored in Tegra block-linear GOB order.
use crate::binary::Fields;
use anyhow::{Result, ensure};

fn surface(width: usize, height: usize, pixels: &[u8]) -> Vec<u8> {
    let gob_height = height.div_ceil(8).next_power_of_two().min(16);
    let columns = (width * 4).div_ceil(64);
    let mut tiled = vec![0; columns * height.div_ceil(8 * gob_height) * gob_height * 512];
    for (i, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let x = (i % width) * 4;
        let y = i / width;
        let gob = (y / (8 * gob_height) * columns + x / 64) * gob_height + (y / 8) % gob_height;
        let offset = 512 * gob
            + (x % 64 / 32) * 256
            + (y % 8 / 2) * 64
            + (x % 32 / 16) * 32
            + (y % 2) * 16
            + x % 16;
        tiled[offset..offset + 4].copy_from_slice(pixel);
    }
    tiled
}

fn downsample(pixels: &[u8], w: usize, h: usize) -> Vec<u8> {
    let nw = (w / 2).max(1);
    let nh = (h / 2).max(1);
    let mut out = Vec::with_capacity(nw * nh * 4);
    for y in 0..nh {
        for x in 0..nw {
            let mut rgb = [0u32; 3];
            let mut alpha = 0;
            let mut count = 0;
            for sy in y * h / nh..(y + 1) * h / nh {
                for sx in x * w / nw..(x + 1) * w / nw {
                    let p = &pixels[(sy * w + sx) * 4..];
                    let a = u32::from(p[3]);
                    for k in 0..3 {
                        rgb[k] += u32::from(p[k]) * a;
                    }
                    alpha += a;
                    count += 1;
                }
            }
            out.extend(rgb.map(|v| ((v + alpha / 2) / alpha.max(1)) as u8));
            out.push(((alpha + count / 2) / count) as u8);
        }
    }
    out
}

pub fn rgba8(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        (1..=8191).contains(&width) && (1..=8191).contains(&height),
        "dimensions do not fit a 2D TEX"
    );
    let (mut w, mut h) = (width as usize, height as usize);
    ensure!(rgba.len() == w * h * 4, "wrong RGBA8 image length");
    let mut pixels = std::borrow::Cow::Borrowed(rgba);
    let mut offsets = vec![];
    let mut data = vec![];
    loop {
        offsets.push(u32::try_from(data.len())?);
        data.extend(surface(w, h, &pixels));
        if (w, h) == (1, 1) {
            break;
        }
        pixels = downsample(&pixels, w, h).into();
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    let mut out = b"TEX\0".to_vec();
    out.u32(0x200080a0);
    out.u32(height << 19 | width << 6 | offsets.len() as u32);
    out.extend([1, 7, 1, 0]);
    out.u32(u32::try_from(data.len())?);
    for off in offsets {
        out.u32(off);
    }
    out.extend(data);
    Ok(out)
}
