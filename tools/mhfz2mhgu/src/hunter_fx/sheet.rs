//! Compile Frontier's particle colour and alpha into GU texture animation frames.
use anyhow::{Result, ensure};
use glam::{IVec2, Vec2};
use mhf_resource::image::Image;
use mhgu_resource::{model::MeshGroup, tex};

pub(super) struct Sheet {
    id: u16,
    pub mesh: u16,
    frames: Vec<[u8; 4]>,
    scroll: Vec2,
}

fn tile(age: usize, columns: i32) -> IVec2 {
    IVec2::new(age as i32 % columns, age as i32 / columns)
}

impl Sheet {
    pub fn new(id: u16, mesh: u16, life: u16, keys: &[(u32, [u8; 4])], scroll: [f32; 2]) -> Self {
        let frames = (0..=u32::from(life))
            .map(|age| {
                let next = keys.partition_point(|&(t, _)| t <= age);
                let (t, a) = keys[next.saturating_sub(1)];
                let Some(&(end, b)) = keys.get(next) else {
                    return a;
                };
                if age <= t {
                    return a;
                }
                let w = (age - t) as f32 / (end - t) as f32;
                core::array::from_fn(|i| {
                    (f32::from(a[i]) + w * (f32::from(b[i]) - f32::from(a[i]))).round() as u8
                })
            })
            .collect();
        Self {
            id,
            mesh,
            frames,
            scroll: Vec2::from_array(scroll),
        }
    }

    fn grid(&self) -> IVec2 {
        let n = self.frames.len() as i32;
        let cols = (n as f32).sqrt().ceil() as i32;
        IVec2::new(cols, (n + cols - 1) / cols)
    }

    pub fn path(&self, namespace: &str) -> String {
        format!(r"effect\{namespace}\r{}", self.id)
    }

    pub fn uv_offsets(&self) -> impl ExactSizeIterator<Item = Vec2> {
        let grid = self.grid();
        (0..self.frames.len()).map(move |age| tile(age, grid.x).as_vec2() / grid.as_vec2())
    }

    pub fn texture(
        &self,
        mesh: &mut MeshGroup,
        original_uv: &[[f32; 2]],
        src: &Image,
        wrap: bool,
    ) -> Result<Vec<u8>> {
        let size = IVec2::new(src.width as i32, src.height as i32);
        let mut lo = Vec2::splat(f32::INFINITY);
        let mut hi = Vec2::splat(f32::NEG_INFINITY);
        for &uv in original_uv {
            let xy = Vec2::from_array(uv) * size.as_vec2();
            lo = lo.min(xy);
            hi = hi.max(xy);
        }
        // One source texel of gutter on each side for GU's linear sampler.
        let start = lo.floor().as_ivec2() - IVec2::ONE;
        let cell = hi.ceil().as_ivec2() + IVec2::ONE - start;
        let grid = self.grid();
        let extent = grid * cell;
        ensure!(
            extent.min_element() > 0 && extent.max_element() <= 8191,
            "radiance sheet exceeds TEX dimensions"
        );
        let mut pixels = vec![0; (extent.x * extent.y * 4) as usize];
        let texel = |p: IVec2| {
            let p = if wrap {
                p.rem_euclid(size)
            } else {
                p.clamp(IVec2::ZERO, size - IVec2::ONE)
            };
            ((p.y * size.x + p.x) * 4) as usize
        };
        for (age, rgba) in self.frames.iter().enumerate() {
            let tile = tile(age, grid.x) * cell;
            let scroll = self.scroll * size.as_vec2() * age as f32;
            let delta = scroll.floor();
            let frac = scroll - delta;
            let origin = start + delta.as_ivec2();
            for y in 0..cell.y {
                for x in 0..cell.x {
                    let at = origin + IVec2::new(x, y);
                    let [a, b, c, d] =
                        [at, at + IVec2::X, at + IVec2::Y, at + IVec2::ONE].map(texel);
                    let dst = (((tile.y + y) * extent.x + tile.x + x) * 4) as usize;
                    for channel in 0..4 {
                        let sample = |i: usize| f32::from(src.rgba[i + channel]);
                        let top = sample(a) + frac.x * (sample(b) - sample(a));
                        let bottom = sample(c) + frac.x * (sample(d) - sample(c));
                        pixels[dst + channel] =
                            ((top + frac.y * (bottom - top)) * f32::from(rgba[channel]) / 255.)
                                .round() as u8;
                    }
                }
            }
        }
        for (vertex, &uv) in mesh.vertices.iter_mut().zip(original_uv) {
            vertex.uv =
                (Vec2::from_array(uv) * size.as_vec2() - start.as_vec2()) / extent.as_vec2();
        }
        tex::rgba8_unmipped(extent.x as u32, extent.y as u32, &pixels)
    }
}
