//! Frontier curves compiled to GU EFL model particles. No playback code is
//! embedded in the plugin: A9958C/A9A280 animate model particles, AFB2BC and
//! AF938C evaluate their vector/colour keys, and AEDE08 owns their lifetime.
use super::{BANKS, CHARGE_AURA_MESH, half, sheet::Sheet};
use anyhow::{Context, Result, ensure};
use mhgu_resource::arc::{self, Asset};
use std::collections::BTreeSet;

const EFL: u32 = 0x6d5ae854;
const PEL: u32 = 0x5a525c16;
type V3 = [f32; 3];
type Keys = Vec<(u32, V3)>;

fn word(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn put(b: &mut [u8], p: usize, n: u32) {
    b[p..p + 4].copy_from_slice(&n.to_le_bytes());
}
fn short(b: &mut [u8], p: usize, n: u16) {
    b[p..p + 2].copy_from_slice(&n.to_le_bytes());
}
fn float(b: &mut [u8], p: usize, n: f32) {
    put(b, p, n.to_bits());
}
fn vec(b: &[u8], p: usize) -> V3 {
    core::array::from_fn(|i| f32::from_bits(word(b, p + i * 4)))
}
fn append(b: &mut Vec<u8>, data: &[u8]) -> usize {
    b.resize(b.len().next_multiple_of(16), 0);
    let p = b.len();
    b.extend(data);
    p
}
fn particle_block(data: &mut Vec<u8>, slot: usize, kind: u8, block: &[u8]) {
    let offset = append(data, block) as u32;
    put(data, slot, (offset << 8) | u32::from(kind));
}
fn vector_keys(
    b: &mut Vec<u8>,
    keys: &[(u32, V3)],
    random: V3,
    repeat: bool,
    proportional: bool,
) -> Result<u16> {
    ensure!(
        !keys.is_empty() && keys.len() < 256,
        "invalid EFL vector curve"
    );
    let p = b.len();
    b.extend(((keys.len() as u32) | if repeat { 0x40000000 } else { 0 }).to_le_bytes());
    for &(t, value) in keys {
        b.extend(t.to_le_bytes());
        for (v, r) in value.into_iter().zip(random) {
            let r = if proportional { v * r } else { r };
            b.extend((v - r).to_le_bytes());
            b.extend((r * 2.).to_le_bytes());
        }
    }
    Ok(p.try_into()?)
}
fn sample(keys: &[(u32, V3)], t: u32) -> V3 {
    let mut prev = keys[0];
    for &(frame, value) in &keys[1..] {
        if t <= frame {
            let w = (t.saturating_sub(prev.0) as f32 / (frame - prev.0) as f32).min(1.);
            return core::array::from_fn(|i| prev.1[i] + w * (value[i] - prev.1[i]));
        }
        prev = (frame, value);
    }
    prev.1
}

// Keep scale and colour at zero until the delayed emission starts.
fn delay_visible_keys<T: Default + Copy>(keys: &mut Vec<(u32, T)>, delay: u32) {
    if delay == 0 {
        return;
    }
    // Frontier clamps to the first value from birth, even if its key is later.
    if keys[0].0 != 0 {
        keys.insert(0, (0, keys[0].1));
    }
    for (frame, _) in keys.iter_mut() {
        *frame += delay;
    }
    if delay > 1 {
        keys.insert(0, (delay - 1, T::default()));
    }
    keys.insert(0, (0, T::default()));
}

fn primary_uv(
    model: &mut Vec<u8>,
    sheet: Option<&Sheet>,
    speed: [f32; 2],
    delay: u32,
) -> Result<()> {
    if sheet.is_none() && speed == [0.; 2] {
        return Ok(());
    }
    let mut uv = [0; 64];
    float(&mut uv, 32, 1.);
    if let Some(sheet) = sheet {
        for axis in 0..2 {
            let offsets = sheet.uv_offsets();
            ensure!(offsets.len() < 256, "too many primary UV keys");
            put(&mut uv, 48 + axis * 4, model.len().try_into()?);
            // GU's stepped primary UV keys disable interpolation between tiles.
            model.extend((0x18000000 | offsets.len() as u32).to_le_bytes());
            for (age, offset) in offsets.enumerate() {
                model.extend((delay + age as u32).to_le_bytes());
                model.extend(offset[axis].to_le_bytes());
                model.extend(0f32.to_le_bytes());
            }
        }
    } else {
        // A6B824 initializes primary UVs; A6BC0C advances them per EFL frame.
        for (axis, speed) in speed.into_iter().enumerate() {
            float(&mut uv, axis * 8, -(delay as f32) * speed);
            float(&mut uv, 16 + axis * 8, speed);
        }
    }
    let offset = append(model, &uv).try_into()?;
    short(model, 308, offset);
    Ok(())
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum Facing {
    World = 0,
    Camera = 1,
    CameraYaw = 3,
}

impl Facing {
    fn from_frontier(flags: u32) -> Self {
        // HD 113CEA0A (short definitions) and 113D3133 (wide) apply the
        // inverse view rotation or its yaw-only basis after local rotation.
        if flags & 1 != 0 {
            Self::Camera
        } else if flags & 2 != 0 {
            Self::CameraYaw
        } else {
            Self::World
        }
    }
}

struct Layer {
    mesh: u16,
    frames_per_second: f32,
    blend: u8,
    delay: u16,
    life: u16,
    count: u16,
    position: V3,
    spread: V3,
    rotation: Keys,
    rotation_range: V3,
    scale: Keys,
    scale_range: V3,
    movement: Keys,
    color: Vec<(u32, [u8; 4])>,
    uv_speed: [f32; 2],
    pulse: bool,
    facing: Facing,
    camera_depth: Keys,
}

pub(super) struct Compiler {
    common: Vec<u8>,
    model_flags: u32,
    joint: Vec<u8>,
    proof: [u8; 160],
    rows: Vec<u8>,
    count: u16,
    routes: Vec<u8>,
    assets: Vec<Asset>,
    pub(super) meshes: BTreeSet<usize>,
    pub(super) sheets: Vec<Sheet>,
}
impl Compiler {
    pub fn new(weapon: &[arc::Entry<'_>], mut proof: [u8; 160]) -> Result<Self> {
        let efl = weapon
            .iter()
            .find(|e| e.kind == EFL && e.name == r"effect\pl\w00\w00_000")
            .context("missing native model-particle template")?
            .unpack()?;
        ensure!(
            efl.starts_with(b"EFL\0\x06\x03\x12\x20"),
            "unexpected EFL version"
        );
        let data = &efl[48..];
        ensure!(word(data, 4) & 255 == 5, "expected a native model particle");
        let c = (word(data, 0) >> 8) as usize;
        ensure!(
            word(data, c) == word(&proof, 72),
            "EFL and PEL generator masks differ"
        );
        let m = (word(data, 4) >> 8) as usize;
        // GU reads a joint-offset table immediately after the particle list.
        // The first retail joint is an identity transform with no curves.
        let lists = usize::from(half(&efl, 16)?);
        let j = word(data, lists * 16) as usize;
        ensure!(
            data[j + 104..j + 112] == [0; 8],
            "expected a native joint without external curves"
        );
        proof[..32].fill(0);
        for i in 0..3 {
            float(&mut proof, 32 + i * 4, 1.);
        }
        proof[54] = 0; // native parent/joint attachment
        proof[56] = 2; // world orientation, independent of joint rotation
        proof[58] = 0; // end with child particles
        Ok(Self {
            common: data[c..c + 192].to_vec(),
            model_flags: word(
                data.get(m..m + 320)
                    .context("short native model particle")?,
                0,
            ) & 0x00ff_ffff,
            joint: data[j..j + 112].to_vec(),
            proof,
            rows: vec![],
            count: 0,
            routes: vec![],
            assets: vec![],
            meshes: BTreeSet::new(),
            sheets: Vec::new(),
        })
    }
    fn layer(&mut self, mut layer: Layer, sheet: Option<Sheet>) -> Result<()> {
        self.meshes.insert(usize::from(layer.mesh));
        let id = self.count;
        self.count = self
            .count
            .checked_add(1)
            .context("too many effect layers")?;
        let path = format!(r"effect\frontier_gs\e{id}");
        // One particle directory entry, followed by one joint offset.
        let mut data = vec![0; 20];
        let mut common = self.common.clone();
        // One native emission, with no LOD thinning of required feedback.
        put(&mut common, 8, word(&self.common, 8) | 4);
        short(&mut common, 12, layer.count);
        put(&mut common, 72, layer.count.into());
        put(&mut common, 76, 1);
        put(&mut common, 80, 1);
        put(&mut common, 84, 0);
        // Native box emission, independently randomized per particle.
        common[120] = u8::from(layer.spread != [0.; 3]);
        for (i, range) in layer.spread.into_iter().enumerate() {
            float(&mut common, 96 + i * 8, range);
            float(&mut common, 100 + i * 8, 0.);
        }
        particle_block(&mut data, 0, 0, &common);
        let mut model = vec![0; 320];
        // A9958C copies this selector into each particle; B007FC then chooses
        // the native blend state, overriding the MRL. 0 = AddAlpha, 1 = BlendAlpha.
        put(
            &mut model,
            0,
            self.model_flags | u32::from(layer.blend) << 24,
        );
        float(&mut model, 28, 1.);
        float(&mut model, 32, 1.);
        float(&mut model, 40, 1.);
        let mesh = match &sheet {
            Some(sheet) => sheet.path(),
            None => format!(r"effect\frontier_gs\m{}", layer.mesh),
        };
        model[80..80 + mesh.len()].copy_from_slice(mesh.as_bytes());
        // A949F8 reads Euler order 4 and travel axis 6 (disabled).
        // A956D8 reads the camera mode separately at bits 8..11.
        put(&mut model, 256, 0x64 | (layer.facing as u32) << 8);
        let depth = &layer.camera_depth;
        if depth.iter().any(|&(_, v)| v[2] != 0.) {
            // A956D8 moves the final model origin along the camera-to-particle ray.
            put(&mut model, 268, 1 << 28);
            float(&mut model, 272, -depth[0].1[2]);
            if depth.len() > 1 {
                // The native model name ends at its NUL. The private suffix
                // locates the depth keys without altering the model path.
                ensure!(mesh.len() < 48, "model name overlaps its depth keys");
                model[128..132].copy_from_slice(b"FZD1");
                let offset = model.len() as u32;
                put(&mut model, 132, offset);
                model.extend((depth.len() as u32).to_le_bytes());
                for &(frame, value) in depth {
                    model.extend((frame + u32::from(layer.delay)).to_le_bytes());
                    model.extend((-value[2]).to_le_bytes());
                }
            }
        }
        // Signed ranges become native [minimum, width] random curves.
        let delay = u32::from(layer.delay);
        for (frame, _) in &mut layer.rotation {
            *frame += delay;
        }
        let rot = vector_keys(
            &mut model,
            &layer.rotation,
            layer.rotation_range,
            false,
            false,
        )?;
        short(&mut model, 304, rot);
        delay_visible_keys(&mut layer.scale, delay);
        // Random scale is factored into each authored key, not resampled per frame.
        let sc = vector_keys(
            &mut model,
            &layer.scale,
            layer.scale_range,
            layer.pulse,
            true,
        )?;
        short(&mut model, 306, sc);
        let col: u16 = model.len().try_into()?;
        if sheet.is_some() {
            layer.color = vec![(0, [255; 4])];
        }
        delay_visible_keys(&mut layer.color, delay);
        ensure!(
            !layer.color.is_empty() && layer.color.len() < 256,
            "invalid EFL colour curve"
        );
        model.extend((layer.color.len() as u32 | 0x200).to_le_bytes());
        for (t, rgba) in layer.color {
            model.extend(t.to_le_bytes());
            model.extend(rgba);
            model.extend(rgba);
        }
        short(&mut model, 66, col);
        primary_uv(&mut model, sheet.as_ref(), layer.uv_speed, delay)?;
        self.sheets.extend(sheet);
        particle_block(&mut data, 4, 5, &model);
        // Native lifetime envelope; the authored colour curve supplies alpha.
        let duration = delay + u32::from(layer.life);
        let mut life = vec![0; 16];
        put(&mut life, 4, duration);
        particle_block(&mut data, 8, 0x12, &life);
        let moving = layer.movement.iter().any(|&(_, v)| v != [0.; 3]);
        let mut movement = vec![0; 128];
        put(&mut movement, 0, 4);
        if moving {
            // These source banks use vertical, piecewise-linear travel.
            // Emit the exact per-tick velocity into GU's native speed curve.
            ensure!(
                layer
                    .movement
                    .iter()
                    .all(|&(_, v)| v[0] == 0. && v[2] == 0.),
                "unsupported nonvertical effect travel"
            );
            // A59268 reads the axis from bits 8..11 of the second word.
            // A64DD8 needs a direction curve to restart from a zero speed.
            put(&mut movement, 4, 2 << 8); // positive Y
            let direction = vector_keys(&mut movement, &[(0, [0.; 3])], [0.; 3], false, false)?;
            short(&mut movement, 56, direction);
            let offset: u16 = movement.len().try_into()?;
            ensure!(duration < 255, "effect travel curve too long");
            movement.extend((duration + 1).to_le_bytes());
            for t in 0..=duration {
                let age = t.saturating_sub(delay);
                let speed = if t < delay {
                    0.
                } else {
                    sample(&layer.movement, age + 1)[1] - sample(&layer.movement, age)[1]
                };
                movement.extend(t.to_le_bytes());
                movement.extend(speed.to_le_bytes());
                movement.extend(0f32.to_le_bytes());
            }
            short(&mut movement, 58, offset);
        }
        particle_block(&mut data, 12, u8::from(moving), &movement);
        // The PEL supplies the event's world transform. AEB390 reads this
        // local translation, so the five uppercut emitters turn with the swing.
        let mut joint = self.joint.clone();
        if matches!(layer.facing, Facing::Camera | Facing::CameraYaw) {
            // GU joint mode 2 transforms the origin by the parent but keeps
            // the local basis. Inheriting yaw here would turn billboards edge-on.
            put(&mut joint, 0, (word(&self.joint, 0) & !0xf000) | 0x2000);
        }
        for (axis, value) in layer.position.into_iter().enumerate() {
            float(&mut joint, 32 + axis * 4, value);
        }
        let j = append(&mut data, &joint);
        put(&mut data, 16, j.try_into()?);
        let mut efl = vec![0; 48];
        efl[..4].copy_from_slice(b"EFL\0");
        put(&mut efl, 4, 0x20120306);
        put(&mut efl, 8, data.len().try_into()?);
        float(&mut efl, 12, layer.frames_per_second);
        short(&mut efl, 16, 1);
        short(&mut efl, 18, 1);
        put(&mut efl, 20, 0x100);
        efl.extend(data);
        self.assets.push(Asset {
            name: path.clone(),
            kind: EFL,
            data: efl,
        });
        self.rows.push(path.len().try_into()?);
        self.rows.extend(path.bytes());
        self.rows.push(0);
        let mut proof = self.proof;
        short(&mut proof, 48, id);
        self.rows.extend(proof);
        Ok(())
    }
    fn route(&mut self, start: u16) {
        self.routes.extend(start.to_le_bytes());
        self.routes.extend((self.count - start).to_le_bytes());
    }
    pub fn bank(&mut self, data: &[u8], bank: u16, selectors: &[u16]) -> Result<()> {
        ensure!(half(data, 0)? >= 4, "unsupported Frontier effect bank");
        let radiance = bank == 4022;
        let mut tail = data.get(28..).context("short effect bank")?;
        let mut tables = [&[][..]; 6];
        for (i, stride) in [112, 24, 16, 16, 56, 140].into_iter().enumerate() {
            (tables[i], tail) = tail
                .split_at_checked(usize::from(half(data, 2 + 2 * i)?) * stride)
                .context("truncated effect table")?;
        }
        let [emitters, vectors, colors, mesh_keys, short_defs, wide_defs] = tables;
        let vector_curve = |id, initial, time_scale: f32| -> Keys {
            let mut keys: Keys = vectors
                .chunks_exact(24)
                .skip_while(|k| u16::from(k[18]) != id)
                .take_while(|k| u16::from(k[18]) == id)
                .map(|k| ((word(k, 12) as f32 * time_scale) as u32, vec(k, 0)))
                .collect();
            if keys.is_empty() {
                keys.push((0, initial));
            }
            keys
        };
        for &selector in selectors {
            let first = self.count;
            for row in emitters.chunks_exact(112) {
                if half(row, 80)? != selector {
                    continue;
                }
                let before = self.count;
                let definition = half(row, 76)?;
                let wide = half(row, 84)? & 1 != 0;
                let (defs, stride) = if wide {
                    (wide_defs, 140)
                } else {
                    (short_defs, 56)
                };
                let rotation_offset = vec(row, 24);
                let base_scale = vec(row, 48);
                let scale_spread = vec(row, 60);
                // 81F482A4 passes row+92 to the child. 113CCAF0 and
                // 113CDE18 use the same multiplier for key times and lifetime.
                let rate = f32::from_bits(word(row, 92));
                let time_scale = if rate >= 0.01 { rate } else { 1. };
                for def in defs.chunks_exact(stride) {
                    if half(def, 4)? != definition {
                        continue;
                    }
                    ensure!(half(def, 6)? == 0, "effect requires a private object bank");
                    let mesh_key = half(def, 10)?;
                    for key in mesh_keys.chunks_exact(16) {
                        if half(key, 6)? == mesh_key {
                            let id = i32::from_le_bytes(key[8..12].try_into()?);
                            if let Ok(id) = usize::try_from(id) {
                                self.meshes.insert(id);
                            }
                        }
                    }
                    // 113CFE32 evaluates short definitions' local rotation in degrees.
                    let local_rotation = radiance && !wide && half(def, 38)? != 0;
                    let rotation_id = half(def, if local_rotation { 38 } else { 16 })?;
                    let rotation = vector_curve(rotation_id, [0.; 3], time_scale)
                        .into_iter()
                        .map(|(t, v)| {
                            let v = if local_rotation {
                                v.map(f32::to_radians)
                            } else {
                                v
                            };
                            (t, core::array::from_fn(|i| v[i] + rotation_offset[i]))
                        })
                        .collect();
                    let scale = vector_curve(half(def, 18)?, [1.; 3], time_scale)
                        .into_iter()
                        .map(|(t, v)| (t, core::array::from_fn(|i| v[i] * base_scale[i])))
                        .collect();
                    let ck = half(def, if wide { 110 } else { 32 })?;
                    let mut color: Vec<_> = colors
                        .chunks_exact(16)
                        .skip_while(|k| u16::from(k[6]) != ck)
                        .take_while(|k| u16::from(k[6]) == ck)
                        // Source 113CCAF0 reads R,G,B,A at offsets 8..11.
                        // GU's native colour curve has the same byte order.
                        .map(|k| {
                            (
                                (word(k, 0) as f32 * time_scale) as u32,
                                [k[8], k[9], k[10], k[11]],
                            )
                        })
                        .collect();
                    if color.is_empty() {
                        color.push((0, [255; 4]));
                    }
                    let mut uv_speed = [0.; 2];
                    if !wide {
                        // 113D0690 advances U/V once per two bank frames.
                        let axes = match def[22] {
                            0 => [false, false],
                            1 => [true, true],
                            2 => [true, false],
                            3 => [false, true],
                            mode => anyhow::bail!("unsupported Frontier UV mode {mode}"),
                        };
                        for (axis, enabled) in axes.into_iter().enumerate() {
                            if enabled {
                                let period = word(def, 24 + 4 * axis) as i32;
                                ensure!(period != 0, "zero UV scroll period");
                                uv_speed[axis] = 0.5 / (period as f32 * time_scale);
                            }
                        }
                    }
                    let mesh = half(def, 8)?;
                    let life = ((f32::from(half(def, 12)?) * time_scale).ceil() as u16).max(1);
                    let sheet =
                        radiance.then(|| Sheet::new(self.count, mesh, life, &color, uv_speed));
                    let camera_depth = if radiance && !wide {
                        vector_curve(half(def, 34)?, [0.; 3], time_scale)
                    } else {
                        Vec::new()
                    };
                    self.layer(
                        Layer {
                            mesh,
                            // 113CE033 advances two bank frames per 30 Hz simulation tick.
                            frames_per_second: 60.,
                            // Frontier HD 113CEBFA/113CEC69: byte 21 selects the
                            // destination blend factor. A mesh can use both modes.
                            blend: u8::from(def[21] != 1),
                            delay: half(row, 78)?,
                            life,
                            count: half(row, 88)?,
                            position: vec(row, 0),
                            spread: vec(row, 12),
                            rotation,
                            rotation_range: vec(row, 36),
                            scale,
                            scale_range: core::array::from_fn(|i| {
                                if base_scale[i] == 0. {
                                    0.
                                } else {
                                    scale_spread[i] / base_scale[i]
                                }
                            }),
                            movement: vector_curve(half(def, 14)?, [0.; 3], time_scale),
                            color,
                            uv_speed,
                            pulse: false,
                            facing: Facing::from_frontier(word(def, 0)),
                            camera_depth,
                        },
                        sheet,
                    )?;
                }
                ensure!(
                    self.count > before,
                    "missing definition for emitter {selector}"
                );
            }
            ensure!(self.count > first, "missing Frontier emitter {selector}");
            self.route(first);
        }
        Ok(())
    }
    pub fn charge(&mut self) -> Result<()> {
        let first = self.count;
        for kind in 0..2 {
            for mirrored in [false, true] {
                let rate = [364., 127.][kind] * core::f32::consts::TAU / 65536.;
                let y = if mirrored { core::f32::consts::PI } else { 0. };
                self.layer(
                    Layer {
                        mesh: [CHARGE_AURA_MESH, 72][kind],
                        uv_speed: [0.; 2],
                        frames_per_second: 30.,
                        // Limit the combined brightness of the overlapping auras
                        // before GU's HDR and bloom passes.
                        blend: 1,
                        delay: 0,
                        life: 600,
                        count: 1,
                        position: [0.; 3],
                        spread: [0.; 3],
                        rotation: vec![(0, [0., y, 0.]), (600, [0., y, 600. * rate])],
                        rotation_range: [0., 0., core::f32::consts::PI],
                        scale: if kind == 0 {
                            vec![(0, [2., 2., 1.]), (2, [3., 3., 1.]), (4, [2., 2., 1.])]
                        } else {
                            vec![(0, [2., 2., 1.]), (90, [5., 5., 1.])]
                        },
                        scale_range: [0.; 3],
                        movement: vec![(0, [0.; 3])],
                        color: if kind == 0 {
                            vec![
                                (0, [0; 4]),
                                (30, [255, 192, 64, 96]),
                                (90, [255, 192, 64, 96]),
                            ]
                        } else {
                            vec![(0, [224, 32, 32, 0]), (90, [224, 32, 32, 96])]
                        },
                        pulse: kind == 0,
                        facing: Facing::Camera,
                        camera_depth: Vec::new(),
                    },
                    None,
                )?;
            }
        }
        self.route(first);
        for level in 1..=4 {
            let first = self.count;
            let factor = 0.5 + 0.25 * level as f32;
            for kind in 0..2 {
                let scale: &[(u32, f32)] = if kind == 0 {
                    &[(0, 6.), (3, 3.), (12, 2.)]
                } else {
                    &[(0, 0.2), (2, 1.), (8, 1.5)]
                };
                self.layer(
                    Layer {
                        mesh: [CHARGE_AURA_MESH, 88][kind],
                        uv_speed: [0.; 2],
                        frames_per_second: 30.,
                        blend: 1,
                        delay: 0,
                        life: [12, 8][kind],
                        count: 1,
                        position: [0.; 3],
                        spread: [0.; 3],
                        rotation: vec![(0, [0.; 3])],
                        rotation_range: if kind == 0 {
                            [0., 0., core::f32::consts::PI]
                        } else {
                            [0.; 3]
                        },
                        scale: scale
                            .iter()
                            .map(|&(t, s)| (t, [s * factor, s * factor, factor]))
                            .collect(),
                        scale_range: [0.; 3],
                        movement: vec![(0, [0.; 3])],
                        color: vec![
                            (0, [255, 176, 176, 255]),
                            (if kind == 0 { 3 } else { 2 }, [255, 176, 176, 255]),
                            (if kind == 0 { 12 } else { 8 }, [64, 0, 0, 16]),
                        ],
                        pulse: false,
                        facing: Facing::Camera,
                        camera_depth: Vec::new(),
                    },
                    None,
                )?;
            }
            self.route(first);
        }
        Ok(())
    }
    pub fn finish(mut self) -> (Vec<Asset>, Vec<u8>) {
        let mut pel = b"PEL\0".to_vec();
        for n in [15u32, 0, u32::from(self.count)] {
            pel.extend(n.to_le_bytes());
        }
        pel.extend(self.rows);
        pel.extend(0u32.to_le_bytes());
        self.assets.push(Asset {
            name: r"effect\frontier_gs\particles".into(),
            kind: PEL,
            data: pel,
        });
        debug_assert_eq!(
            self.routes.len(),
            (BANKS.iter().map(|(_, v)| v.len()).sum::<usize>() + 5) * 4
        );
        (self.assets, self.routes)
    }
}
