//! GU's mono Switch wave contains Nintendo DSP ADPCM, 14 samples per frame.
const FILTERS: [(i32, i32); 8] = [
    (0, 0),
    (2048, 0),
    (3072, -1024),
    (4096, -2048),
    (1536, 0),
    (2560, -768),
    (3584, -1664),
    (1920, -256),
];

pub fn encode(pcm: &[i16], rate: u32) -> Vec<u8> {
    let n = pcm.len();
    let mut out = vec![0u8; 120];
    out[..8].copy_from_slice(&[2, 0, 0, 0, 1, 0, 0, 1]);
    let size = n / 14 * 8
        + if n % 14 == 0 {
            0
        } else {
            1 + (n % 14).div_ceil(2)
        };
    out[8..12].copy_from_slice(&(size as u32).to_le_bytes());
    out[12..16].copy_from_slice(&(n as f32 / rate as f32).to_le_bytes());
    // Packed GU sound resource revision, preceding the 16 signed coefficients.
    out[16..24].copy_from_slice(&[0xe1, 7, 8, 2, 12, 3, 35, 0]);
    for (i, (a, b)) in FILTERS.into_iter().enumerate() {
        out[24 + 4 * i..26 + 4 * i].copy_from_slice(&(a as i16).to_le_bytes());
        out[26 + 4 * i..28 + 4 * i].copy_from_slice(&(b as i16).to_le_bytes());
    }
    for (at, val) in [(96, n as u32), (100, rate), (112, n as u32 - 1)] {
        out[at..at + 4].copy_from_slice(&val.to_le_bytes());
    }
    let mut history = [0i32; 2];
    for block in pcm.chunks(14) {
        let mut best = (i64::MAX, [0u8; 8], history);
        for (pred, (a, b)) in FILTERS.into_iter().enumerate() {
            let mut h = history;
            let mut peak = 0;
            for &x in block {
                let x = i32::from(x);
                peak = peak.max((x - ((a * h[0] + b * h[1] + 1024) >> 11)).abs());
                h = [x, h[0]];
            }
            let scale: i32 = (0..=12).find(|s| peak <= 7 << s).unwrap_or(12);
            for exp in scale.saturating_sub(1i32)..=scale {
                let step = 1 << exp;
                let mut h = history;
                let mut data = [0u8; 8];
                data[0] = (pred as u8) << 4 | exp as u8;
                let mut err = 0i64;
                for (i, &x) in block.iter().enumerate() {
                    let base = (a * h[0] + b * h[1] + 1024) >> 11;
                    let delta = i32::from(x) - base;
                    let q = ((delta + step / 2).div_euclid(step)).clamp(-8, 7);
                    let y = (base + q * step).clamp(-32768, 32767);
                    err += i64::from(i32::from(x) - y).pow(2);
                    data[1 + i / 2] |= (q as u8 & 15) << (4 * (1 - i % 2));
                    h = [y, h[0]];
                }
                if err < best.0 {
                    best = (err, data, h);
                }
            }
        }
        history = best.2;
        out.extend(best.1);
    }
    out.resize(120 + size.next_multiple_of(256), 0);
    out
}
