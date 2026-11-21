//! Frontier curves compiled to GU EFL model particles. No playback code is
//! embedded in the plugin: A9958C/A9A280 animate model particles, AFB2BC and
//! AF938C evaluate their vector/colour keys, and AEDE08 owns their lifetime.
use super::{CHARGE_AURA_MESH, half, sheet::Sheet};
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

fn wide_curves(def: &[u8], life: u16, mut size: V3, velocity_curve: &[(u32, V3)]) -> (Keys, Keys) {
    let mut position = [0.; 3];
    let mut velocity = vec(def, 20);
    let growth = vec(def, 68);
    let gravity = f32::from_bits(word(def, 92));
    let mut movement = vec![(0, position)];
    let mut scale = vec![(0, size)];
    // FZ HD 113D2190 and 113D2700 integrate once per two bank frames.
    for frame in (0..u32::from(life)).step_by(2) {
        let multiplier = sample(velocity_curve, frame);
        for axis in 0..3 {
            position[axis] += velocity[axis];
            size[axis] = (size[axis] + growth[axis]).max(0.);
        }
        velocity[1] -= gravity;
        for (speed, multiplier) in velocity.iter_mut().zip(multiplier) {
            *speed *= multiplier;
        }
        movement.push((frame + 2, position));
        scale.push((frame + 2, size));
    }
    (movement, scale)
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
    origin: [f32; 2],
    offsets: &[(u32, V3)],
    delay: u32,
) -> Result<()> {
    if sheet.is_none() && offsets.is_empty() && speed == [0.; 2] && origin == [0.; 2] {
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
    } else if !offsets.is_empty() {
        ensure!(offsets.len() < 256, "too many primary UV keys");
        for axis in 0..2 {
            put(&mut uv, 48 + axis * 4, model.len().try_into()?);
            model.extend((offsets.len() as u32).to_le_bytes());
            for &(age, value) in offsets {
                model.extend((delay + age).to_le_bytes());
                model.extend(value[axis].to_le_bytes());
                model.extend(0f32.to_le_bytes());
            }
        }
    } else {
        // A6B824 initializes primary UVs; A6BC0C advances them per EFL frame.
        for (axis, speed) in speed.into_iter().enumerate() {
            float(&mut uv, axis * 8, origin[axis] - (delay as f32) * speed);
            float(&mut uv, 16 + axis * 8, speed);
        }
    }
    let offset = append(model, &uv).try_into()?;
    short(model, 308, offset);
    Ok(())
}

// A9958C copies this selector into each particle. B007FC selects the
// corresponding blend state instead of using the material's blend state.
#[repr(u32)]
#[derive(Clone, Copy)]
enum Blend {
    Additive = 0,
    Alpha = 1,
    ReverseSubtract = 6, // BSBlendRevSubBlendAlpha
}

impl Blend {
    fn from_frontier(mode: u8) -> Self {
        // FZ HD 113CEBFA / 113D3329 use the same alpha factors for modes
        // 0 and 2. Mode 2 changes the blend operation to reverse subtract.
        match mode {
            1 => Self::Additive,
            2 => Self::ReverseSubtract,
            _ => Self::Alpha,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
enum Facing {
    World = 0,
    Camera = 1,
    CameraYaw = 3,
    CameraPosition = 13,
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

#[derive(Clone)]
struct Layer {
    mesh: u16,
    frames_per_second: f32,
    blend: Blend,
    delay: u16,
    life: u16,
    count: u16,
    position: V3,
    spread: V3,
    rotation: Keys,
    rotation_range: V3,
    proportional_rotation: bool,
    scale: Keys,
    scale_range: V3,
    movement: Keys,
    color: Vec<(u32, [u8; 4])>,
    uv_speed: [f32; 2],
    uv_origin: [f32; 2],
    uv_keys: Keys,
    pulse: bool,
    facing: Facing,
    inherit_joint_rotation: bool,
    camera_depth: Keys,
    mirror_axes: u8,
    soft_particle: f32,
}

impl Layer {
    // The legacy common effects use the hunter's 30 Hz clock.
    fn legacy(mesh: u16, life: u16) -> Self {
        Self {
            mesh,
            frames_per_second: 30.,
            blend: Blend::Additive,
            delay: 0,
            life,
            count: 1,
            position: [0.; 3],
            spread: [0.; 3],
            rotation: vec![(0, [0.; 3])],
            rotation_range: [0.; 3],
            proportional_rotation: false,
            scale: vec![(0, [1.; 3])],
            scale_range: [0.; 3],
            movement: vec![(0, [0.; 3])],
            color: vec![(0, [255; 4])],
            uv_speed: [0.; 2],
            uv_origin: [0.; 2],
            uv_keys: Vec::new(),
            pulse: false,
            facing: Facing::World,
            inherit_joint_rotation: false,
            camera_depth: Vec::new(),
            mirror_axes: 0,
            soft_particle: 0.,
        }
    }
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
    namespace: &'static str,
}
impl Compiler {
    pub fn new(
        weapon: &[arc::Entry<'_>],
        mut proof: [u8; 160],
        namespace: &'static str,
    ) -> Result<Self> {
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
            namespace,
        })
    }
    fn layer(&mut self, mut layer: Layer, sheet: Option<Sheet>) -> Result<()> {
        self.meshes.insert(usize::from(layer.mesh));
        let id = self.count;
        self.count = self
            .count
            .checked_add(1)
            .context("too many effect layers")?;
        let path = format!(r"effect\{}\e{id}", self.namespace);
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
        put(&mut model, 0, self.model_flags | (layer.blend as u32) << 24);
        float(&mut model, 28, 1.);
        float(&mut model, 32, 1.);
        float(&mut model, 40, 1.);
        let mesh = match &sheet {
            Some(sheet) => sheet.path(self.namespace),
            None => format!(r"effect\{}\m{}", self.namespace, layer.mesh),
        };
        model[80..80 + mesh.len()].copy_from_slice(mesh.as_bytes());
        if layer.mirror_axes != 0 || layer.soft_particle > 0. {
            // Private fields after the NUL-terminated model path and FZD1 data.
            ensure!(mesh.len() < 48, "model name overlaps Frontier draw fields");
            model[136..139].copy_from_slice(b"FZF");
            model[139] = layer.mirror_axes;
            float(&mut model, 140, layer.soft_particle);
        }
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
            layer.proportional_rotation,
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
        primary_uv(
            &mut model,
            sheet.as_ref(),
            layer.uv_speed,
            layer.uv_origin,
            &layer.uv_keys,
            delay,
        )?;
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
            ensure!(duration < 255, "effect travel curve too long");
            let vertical = layer
                .movement
                .iter()
                .all(|&(_, v)| v[0] == 0. && v[2] == 0.);
            let mut directions = vec![(0, [0.; 3])];
            let mut speeds = Vec::new();
            // GU 9BD8E0 rotates the chosen axis, then A64DD8 applies speed.
            put(&mut movement, 4, if vertical { 2 << 8 } else { 0 });
            for t in 0..=duration {
                let age = t.saturating_sub(delay);
                let delta = if t < delay {
                    glam::Vec3::ZERO
                } else {
                    glam::Vec3::from(sample(&layer.movement, age + 1))
                        - glam::Vec3::from(sample(&layer.movement, age))
                };
                let speed = if vertical { delta.y } else { delta.length() };
                if !vertical && speed > 0. {
                    let angles = [
                        0.,
                        (-delta.z / speed).clamp(-1., 1.).asin(),
                        delta.y.atan2(delta.x),
                    ];
                    if t == 0 {
                        directions[0].1 = angles;
                    } else {
                        directions.push((t, angles));
                    }
                }
                speeds.push(speed);
            }
            let direction = vector_keys(&mut movement, &directions, [0.; 3], false, false)?;
            short(&mut movement, 56, direction);
            let offset: u16 = movement.len().try_into()?;
            movement.extend((duration + 1).to_le_bytes());
            for (t, speed) in speeds.into_iter().enumerate() {
                movement.extend((t as u32).to_le_bytes());
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
        // 31C3A8 property 7 supplies the orientation mode to 31D92C.
        // Mode 0 uses the current joint rotation on every live update.
        if layer.inherit_joint_rotation {
            proof[56] = 0;
        }
        short(&mut proof, 48, id);
        self.rows.extend(proof);
        Ok(())
    }
    fn mesh_track(&mut self, mut layer: Layer, keys: &[(u32, u16)], seed: u32) -> Result<()> {
        ensure!(
            layer.count == 1,
            "animated mesh emitter has multiple particles"
        );
        // Each EFL holds a single model. Bake one transform for the source
        // emitter so successive models cannot jump between random positions.
        let mut noise = seed;
        let mut signed_sample = || {
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (noise >> 8) as f32 * (2. / 16_777_216.) - 1.
        };
        for index in 0..3 {
            layer.position[index] += layer.spread[index] * signed_sample();
            let rotation = layer.rotation_range[index] * signed_sample();
            let scale = 1. + layer.scale_range[index] * signed_sample();
            for (_, key) in &mut layer.rotation {
                key[index] += rotation;
            }
            for (_, key) in &mut layer.scale {
                key[index] *= scale;
            }
        }
        layer.spread = [0.; 3];
        layer.rotation_range = [0.; 3];
        layer.scale_range = [0.; 3];

        let life = u32::from(layer.life);
        for (i, &(start, mesh)) in keys.iter().enumerate() {
            let end = keys.get(i + 1).map_or(life, |&(time, _)| time);
            let mut window = layer.clone();
            window.mesh = mesh;
            window.life = end.try_into()?;
            if start != 0 {
                let scale = sample(&window.scale, start);
                window.scale.retain(|&(time, _)| time > start);
                window.scale.insert(0, (start, scale));
                if start > 1 {
                    window.scale.insert(0, (start - 1, [0.; 3]));
                }
                window.scale.insert(0, (0, [0.; 3]));
            }
            self.layer(window, None)?;
        }
        Ok(())
    }

    fn route(&mut self, start: u16) {
        self.routes.extend(start.to_le_bytes());
        self.routes.extend((self.count - start).to_le_bytes());
    }
    pub fn bank(&mut self, data: &[u8], bank: u16, selectors: &[u16]) -> Result<()> {
        ensure!(half(data, 0)? >= 4, "unsupported Frontier effect bank");
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
            for (emitter, row) in emitters.chunks_exact(112).enumerate() {
                if half(row, 80)? != selector {
                    continue;
                }
                let before = self.count;
                let definition = half(row, 76)?;
                let emitter_flags = word(row, 84);
                let wide = emitter_flags & 1 != 0;
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
                    // 113CFE32 evaluates the short definition's local rotation
                    // curve in degrees. Other rotation curves use radians.
                    let local_rotation_id = if wide { 0 } else { half(def, 38)? };
                    let rotation_id = if local_rotation_id != 0 {
                        local_rotation_id
                    } else {
                        half(def, 16)?
                    };
                    let mut rotation = vector_curve(rotation_id, [0.; 3], time_scale);
                    for (_, angles) in &mut rotation {
                        if local_rotation_id != 0 {
                            *angles = angles.map(f32::to_radians);
                        }
                        for (angle, offset) in angles.iter_mut().zip(rotation_offset) {
                            *angle += offset;
                        }
                    }
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
                    // 113D1B30 / 113D0690 advance UV once per two bank frames.
                    let uv_mode = def[if wide { 108 } else { 22 }];
                    let mut uv_origin = [0.; 2];
                    let mut uv_keys = Vec::new();
                    let scroll_axes = match uv_mode {
                        0 => [false, false],
                        1 => [true, true],
                        2 => [true, false],
                        3 => [false, true],
                        7 if !wide => {
                            // 113D0A17 samples a vector UV offset at particle age.
                            let id = word(def, 24).try_into()?;
                            let keys = vector_curve(id, [0.; 3], time_scale);
                            let [u, v, _] = keys[0].1;
                            if keys.iter().all(|(_, p)| p[..2] == [u, v]) {
                                uv_origin = [u, v];
                            } else {
                                uv_keys = keys;
                            }
                            [false, false]
                        }
                        mode if wide => anyhow::bail!("unsupported wide Frontier UV mode {mode}"),
                        mode => anyhow::bail!("unsupported Frontier UV mode {mode}"),
                    };
                    let mut uv_speed = [0.; 2];
                    for (axis, enabled) in scroll_axes.into_iter().enumerate() {
                        if !enabled {
                            continue;
                        }
                        let period = if wide {
                            let period = half(def, 112 + 2 * axis)?;
                            ensure!(period != 0, "zero wide UV scroll period");
                            f32::from(period)
                        } else {
                            let period = word(def, 24 + 4 * axis) as i32;
                            ensure!(period != 0, "zero UV scroll period");
                            period as f32
                        };
                        uv_speed[axis] = 0.5 / (period * time_scale);
                    }
                    let mesh = half(def, 8)?;
                    let life = ((f32::from(half(def, 12)?) * time_scale).ceil() as u16).max(1);
                    let flags = word(def, 0);
                    let movement_id = half(def, 14)?;
                    let short_ground = !wide && bank == 4035;
                    // 113D05F0 (short) and 113D20E0 (wide) replace RGB with
                    // the map dust colour. Use the same map 0 tint for both.
                    if (wide || matches!(bank, 4006 | 4035)) && flags & 0x40 != 0 {
                        for (_, [r, g, b, _]) in &mut color {
                            [*r, *g, *b] = [126, 118, 109];
                        }
                    }
                    let (movement, scale) = if wide {
                        let velocity = vector_curve(movement_id, [1.; 3], time_scale);
                        wide_curves(def, life, base_scale, &velocity)
                    } else {
                        let scale = vector_curve(half(def, 18)?, [1.; 3], time_scale)
                            .into_iter()
                            .map(|(t, v)| (t, core::array::from_fn(|i| v[i] * base_scale[i])))
                            .collect();
                        (vector_curve(movement_id, [0.; 3], time_scale), scale)
                    };
                    let mut position = vec(row, 0);
                    if short_ground {
                        // 113CF31B samples the absolute offset. GU travel
                        // integrates deltas, so seed the emitter at its first key.
                        for (origin, offset) in position.iter_mut().zip(movement[0].1) {
                            *origin += offset;
                        }
                    }
                    // Keep the existing shockwave and ready atlases. The other
                    // bank layers use native colour keys like the other weapons.
                    let sheet = (bank == 4022 && matches!(selector, 5 | 6 | 10 | 16 | 21))
                        .then(|| Sheet::new(self.count, mesh, life, &color, uv_speed));
                    let facing = match Facing::from_frontier(flags) {
                        // The particle inherits actor yaw for travel while its
                        // billboard separately faces the camera.
                        Facing::Camera
                            if (wide || short_ground)
                                && movement.iter().any(|(_, p)| *p != [0.; 3]) =>
                        {
                            Facing::CameraPosition
                        }
                        facing => facing,
                    };
                    let camera_depth = if !wide {
                        vector_curve(half(def, 34)?, [0.; 3], time_scale)
                    } else {
                        Vec::new()
                    };
                    let layer = Layer {
                        mesh,
                        // 113CE033 advances two bank frames per 30 Hz simulation tick.
                        frames_per_second: 60.,
                        blend: Blend::from_frontier(def[if wide { 107 } else { 21 }]),
                        delay: half(row, 78)?,
                        life,
                        count: half(row, 88)?,
                        position,
                        spread: vec(row, 12),
                        rotation,
                        rotation_range: vec(row, 36),
                        proportional_rotation: false,
                        scale,
                        scale_range: core::array::from_fn(|i| {
                            if base_scale[i] == 0. {
                                0.
                            } else {
                                scale_spread[i] / base_scale[i]
                            }
                        }),
                        movement,
                        color,
                        uv_speed,
                        uv_origin,
                        uv_keys,
                        pulse: false,
                        facing,
                        // 81216E36 attaches the persistent sword glow to FZ node 21.
                        inherit_joint_rotation: matches!(bank, 4016..=4019) && selector == 8,
                        camera_depth,
                        // FZ 81F482A4 samples independent X/Y mirror signs.
                        mirror_axes: (emitter_flags as u8 >> 2) & 3,
                        // FZ HD 113CEC2B / 113D3354 select the soft-particle draw.
                        soft_particle: f32::from_bits(word(def, if wide { 128 } else { 40 })),
                    };
                    let mesh_key = half(def, 10)?;
                    let mut keys = vec![(0, mesh)];
                    for key in mesh_keys.chunks_exact(16) {
                        if half(key, 6)? != mesh_key {
                            continue;
                        }
                        let id = word(key, 8) as i32;
                        if bank != 4023 {
                            if let Ok(id) = usize::try_from(id) {
                                self.meshes.insert(id);
                            }
                            continue;
                        }
                        let frame = (word(key, 0) as f32 * time_scale) as u32;
                        let Ok(id) = u16::try_from(id) else { continue };
                        if frame >= u32::from(life) {
                            continue;
                        }
                        let last = keys.last_mut().unwrap();
                        if frame == last.0 {
                            last.1 = id;
                        } else if last.1 != id {
                            ensure!(frame > last.0, "mesh keys out of order");
                            keys.push((frame, id));
                        }
                    }
                    if keys.len() > 1 {
                        ensure!(sheet.is_none(), "animated mesh with an atlas");
                        self.mesh_track(layer, &keys, (u32::from(bank) << 16) | emitter as u32)?;
                    } else {
                        self.layer(
                            Layer {
                                mesh: keys[0].1,
                                ..layer
                            },
                            sheet,
                        )?;
                    }
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
                        // Limit the combined brightness of the overlapping auras
                        // before GU's HDR and bloom passes.
                        blend: Blend::Alpha,
                        rotation: vec![(0, [0., y, 0.]), (600, [0., y, 600. * rate])],
                        rotation_range: [0., 0., core::f32::consts::PI],
                        scale: if kind == 0 {
                            vec![(0, [2., 2., 1.]), (2, [3., 3., 1.]), (4, [2., 2., 1.])]
                        } else {
                            vec![(0, [2., 2., 1.]), (90, [5., 5., 1.])]
                        },
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
                        ..Layer::legacy([CHARGE_AURA_MESH, 72][kind], 600)
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
                        blend: Blend::Alpha,
                        rotation_range: if kind == 0 {
                            [0., 0., core::f32::consts::PI]
                        } else {
                            [0.; 3]
                        },
                        scale: scale
                            .iter()
                            .map(|&(t, s)| (t, [s * factor, s * factor, factor]))
                            .collect(),
                        color: vec![
                            (0, [255, 176, 176, 255]),
                            (if kind == 0 { 3 } else { 2 }, [255, 176, 176, 255]),
                            (if kind == 0 { 12 } else { 8 }, [64, 0, 0, 16]),
                        ],
                        facing: Facing::Camera,
                        ..Layer::legacy([CHARGE_AURA_MESH, 88][kind], [12, 8][kind])
                    },
                    None,
                )?;
            }
            self.route(first);
        }
        Ok(())
    }

    // FZ family 41. Types 0, 4 and 17 use meshes 827BFD4C, lifetimes
    // 827BFD58 and delays 827BFD64. Type 4 omits layers 3 and 5–7.
    pub fn charged_slash(&mut self) -> Result<()> {
        self.sword_flash(4)
    }

    pub fn sword_flash(&mut self, kind: u8) -> Result<()> {
        let blue = kind == 17;
        let bright = if blue { [128, 128, 255, 255] } else { [255; 4] };
        let fade = |mut color: [u8; 4]| {
            color[3] = 0;
            color
        };
        let middle = if blue {
            [128, 0, 255, 16]
        } else {
            [255, 0, 0, 16]
        };
        let dark = if blue { [128, 0, 0, 16] } else { middle };
        let first = self.count;
        for (mesh, delay, life, scales, colors) in [
            (
                81,
                0,
                6,
                &[(0, 1.), (6, 5.)][..],
                &[(0, bright), (6, fade(bright))][..],
            ),
            (
                81,
                2,
                12,
                &[(0, 2.), (12, 0.5)][..],
                &[(0, bright), (4, middle), (12, [0; 4])][..],
            ),
            (
                66,
                4,
                10,
                &[(0, 10.), (4, 4.), (10, 1.)][..],
                &[(0, bright), (6, dark), (10, [0; 4])][..],
            ),
            (
                88,
                2,
                6,
                &[(0, 0.5), (6, 1.2)][..],
                &[(0, [0, 0, 0, 255]), (4, [0, 0, 0, 255]), (6, [0; 4])][..],
            ),
        ] {
            if kind == 4 && mesh == 88 {
                continue;
            }
            let mut flash = Layer::legacy(mesh, life);
            flash.delay = delay;
            if mesh == 88 {
                flash.blend = Blend::Alpha;
            }
            flash.facing = Facing::Camera;
            flash.scale = scales.iter().map(|&(t, s)| (t, [s, s, 1.])).collect();
            flash.color = colors.to_vec();
            if mesh == 66 {
                flash.rotation_range[2] = core::f32::consts::PI;
            }
            self.layer(flash, None)?;
        }
        self.route(first);
        let first = self.count;
        let mut ring = Layer::legacy(42, 4);
        ring.delay = if kind == 4 { 2 } else { 5 };
        ring.scale = vec![(0, [4., 1., 4.]), (4, [12., 1., 12.])];
        ring.color = vec![
            (0, [bright[0], bright[1], bright[2], 128]),
            (4, fade(bright)),
        ];
        self.layer(ring, None)?;
        self.route(first);
        if kind != 4 {
            for delay in [5, 9, 13] {
                for direction in [1., -1.] {
                    let first = self.count;
                    let mut streak = Layer::legacy(38, 8);
                    streak.delay = delay;
                    // Source 81FC9E0A truncates (5461 + rand12) / 8, giving a
                    // speed of 682..1194 angle units/tick, with a separate sign bit.
                    let mean = 938. * core::f32::consts::TAU / 65536.;
                    streak.rotation.push((8, [0., 0., 8. * direction * mean]));
                    streak.proportional_rotation = true;
                    streak.rotation_range[2] = direction * 256. / 938.;
                    streak.scale = vec![(0, [4., 4., 1.]), (8, [6., 6., 1.])];
                    streak.color = vec![
                        (
                            0,
                            if blue {
                                [0, 0, 255, 128]
                            } else {
                                [255, 0, 0, 128]
                            },
                        ),
                        (8, [0; 4]),
                    ];
                    self.layer(streak, None)?;
                    self.route(first);
                }
            }
        }
        Ok(())
    }

    pub fn parry_impact(&mut self) -> Result<()> {
        // 81F86288 starts at -4, advances two units/tick, and draws meshes
        // 1..15 once each after crossing zero (81F867C0, subtype 13).
        let first = self.count;
        for mesh in 1..=15 {
            let mut layer = Layer::legacy(mesh, 1);
            layer.delay = mesh + 1;
            layer.scale = vec![(0, [1.5; 3])];
            layer.color = vec![(0, [255, 128, 128, 255])];
            layer.facing = Facing::Camera;
            self.layer(layer, None)?;
        }
        // 81FA1032 subtype 17 chooses a spark variant again on each of two
        // ticks. Runtime selects one route per tick. The ray is independent.
        let mut ray = Layer::legacy(21, 5);
        ray.scale = vec![
            (0, [0.; 3]),
            (1, [1.5; 3]),
            (2, [4.5, 4.5, 1.5]),
            (5, [6., 6., 1.5]),
        ];
        ray.color = vec![
            (0, [255, 128, 128, 0]),
            (1, [255, 128, 128, 255]),
            (2, [255, 128, 128, 255]),
            (5, [255, 128, 128, 51]),
        ];
        ray.camera_depth = vec![(0, [0., 0., 20.])];
        ray.position[1] = 10.;
        ray.spread = [2.; 3];
        self.layer(ray, None)?;
        self.route(first);
        for delay in 0..2 {
            for mesh in 32..=34 {
                let first = self.count;
                let mut spark = Layer::legacy(mesh, 1);
                spark.delay = delay;
                spark.facing = Facing::Camera;
                spark.rotation_range[2] = core::f32::consts::PI;
                let size = 1.5 * f32::from(delay + 1);
                spark.scale = vec![(0, [size, size, 1.5])];
                spark.color = vec![(0, [255, 128, 128, 255])];
                spark.camera_depth = vec![(0, [0., 0., 15.])];
                spark.position[1] = 10.;
                spark.spread = [2.; 3];
                self.layer(spark, None)?;
                self.route(first);
            }
        }
        Ok(())
    }

    fn surface_spray(&mut self, size: f32) -> Result<()> {
        let white = [255; 4];
        // 81F90ADA type 5: six surface sprites, alternating a four-tick delay.
        for i in 0..6 {
            let mut surface = Layer::legacy(26, 20);
            surface.facing = Facing::Camera;
            surface.blend = Blend::Alpha;
            surface.delay = 4 * (i % 2);
            surface.spread = [5.12 * size * f32::from(i), 0., 5.12 * size * f32::from(i)];
            surface.scale = [
                (0, 1.3, 0.7),
                (1, 1.3, 0.7),
                (4, 2.28, 2.28),
                (9, 2.88, 2.8),
                (13, 3.46, 2.9),
                (20, 3.9, 2.75),
            ]
            .map(|(t, x, y)| (t, [size * x, size * y, size]))
            .to_vec();
            surface.color = vec![(0, white), (9, white), (20, [255, 255, 255, 25])];
            self.layer(surface, None)?;
        }
        Ok(())
    }

    pub fn footfalls(&mut self) -> Result<()> {
        let first = self.count;
        self.surface_spray(0.45)?;
        self.route(first);
        // 81F9A86C uses common mesh 74. Normal contacts create one
        // particle, while type 7 (a full-body slide) creates five.
        for slide in [false, true] {
            let first = self.count;
            let mut dust = Layer::legacy(74, 16);
            dust.count = if slide { 5 } else { 1 };
            dust.blend = Blend::Alpha;
            dust.spread = if slide { [30., 0., 30.] } else { [0.; 3] };
            dust.facing = Facing::Camera;
            dust.scale = vec![(0, [0.55; 3]), (16, [1.5; 3])];
            dust.color = vec![(0, [255, 255, 255, 130]), (16, [255, 255, 255, 0])];
            dust.movement.push((16, [0., 32., 0.]));
            self.layer(dust, None)?;
            self.route(first);
        }
        Ok(())
    }

    pub fn spirit_full(&mut self) -> Result<()> {
        // 81F8F096 draws meshes 84, 37 and 88 for type 0. Type 1
        // overlays four instances of 38, spaced by three FZ ticks.
        let first = self.count;
        for (mesh, life, peak, alpha) in [(84, 30, 1.4, 176), (37, 36, 2., 144), (88, 30, 1.2, 96)]
        {
            let mut flash = Layer::legacy(mesh, life);
            flash.facing = Facing::Camera;
            if mesh == 88 {
                flash.blend = Blend::Alpha;
            }
            flash.scale = vec![(0, [0.4; 3]), (4, [peak; 3]), (u32::from(life), [0.9; 3])];
            flash.color = vec![
                (0, [255, 210, 210, alpha]),
                (6, [255, 96, 96, alpha]),
                (u32::from(life), [255, 64, 64, 0]),
            ];
            self.layer(flash, None)?;
        }
        self.route(first);
        let first = self.count;
        for delay in [0, 3, 6, 9] {
            let mut streak = Layer::legacy(38, 22);
            streak.delay = delay;
            streak.facing = Facing::Camera;
            streak.rotation_range = [0., 0., core::f32::consts::PI];
            streak.scale = vec![(0, [0.5; 3]), (22, [1.5; 3])];
            streak.color = vec![(0, [255, 88, 88, 128]), (22, [255, 88, 88, 0])];
            self.layer(streak, None)?;
        }
        self.route(first);
        Ok(())
    }

    pub fn discharge_water(&mut self) -> Result<()> {
        // 811F6460 invokes common types 4 and 5 with scale 2.5.
        let first = self.count;
        let mut ring = Layer::legacy(93, 25);
        ring.rotation_range[1] = core::f32::consts::PI;
        ring.rotation
            .push((25, [0., 25. * 128. * core::f32::consts::TAU / 65536., 0.]));
        ring.scale = [(0, 1.14), (7, 2.), (25, 3.5)]
            .map(|(t, v)| (t, [2.5 * v, 2.5, 2.5 * v]))
            .to_vec();
        ring.color = vec![(0, [255; 4]), (13, [255; 4]), (25, [255, 255, 255, 25])];
        self.layer(ring, None)?;
        self.surface_spray(2.5)?;
        self.route(first);
        self.blade_strip(501)
    }

    // 8102AC68/8102AD08 draws object 151 (21140 / 140) on the left hand.
    // The clip lifetime is controlled by the weapon, including hit stop.
    pub fn switch_axe_cartridge(&mut self) -> Result<()> {
        let first = self.count;
        let mut model = Layer::legacy(151, 32767);
        model.blend = Blend::Alpha;
        model.scale = vec![(0, [1.3; 3])];
        model.rotation = vec![(0, [0., -core::f32::consts::FRAC_PI_6, 0.])];
        self.layer(model, None)?;
        self.route(first);
        Ok(())
    }

    pub fn blade_strip(&mut self, mesh: u16) -> Result<()> {
        let first = self.count;
        let mut trail = Layer::legacy(mesh, 2);
        trail.blend = Blend::Alpha;
        self.layer(trail, None)?;
        self.route(first);
        Ok(())
    }

    pub fn ground_impacts(&mut self) -> Result<()> {
        let white = [255; 4];
        let first = self.count;
        self.surface_spray(2.)?;
        self.route(first);
        // The dry branch of 81F82A3C throws nine pieces of mesh 43. Use
        // samples from its velocity ranges and its per-tick gravity.
        let first = self.count;
        for i in 0..9 {
            let mut piece = Layer::legacy(43, 30);
            piece.blend = Blend::Alpha;
            let vx = (6. + 0.6 * ((3 * i + 2) % 10) as f32)
                * if matches!(i % 4, 1 | 2) { -1. } else { 1. };
            let vz = (6. + 0.6 * ((7 * i + 1) % 10) as f32)
                * if matches!(i % 4, 1 | 3) { -1. } else { 1. };
            let vy = 10. + 0.8 * ((7 * i) % 10) as f32;
            piece.movement = (0..=30)
                .map(|t| {
                    let n = t as f32;
                    (t, [vx * n, vy * n * (1. - (n - 1.) / 15.), vz * n])
                })
                .collect();
            let turn = core::f32::consts::TAU * 30. / 65536.;
            piece.rotation = vec![(0, [0.; 3]), (30, [512. * turn, 576. * turn, 768. * turn])];
            piece.rotation_range = [core::f32::consts::PI; 3];
            self.layer(piece, None)?;
        }
        self.route(first);
        // Type 10 starts three flashes along the strike, with rising clouds
        // and a second debris pass once each flash finishes.
        let first = self.count;
        for (index, (distance, delay)) in [(0., 0), (50., 1), (100., 2)].into_iter().enumerate() {
            let mut flash = Layer::legacy(0, 4);
            flash.position[2] = distance;
            flash.delay = delay;
            flash.facing = Facing::Camera;
            flash.rotation[0].1[2] = core::f32::consts::FRAC_PI_2;
            flash.scale = vec![(0, [0.1, 1.5, 1.]), (2, [5., 3., 1.]), (4, [3., 5., 1.])];
            flash.color = vec![(0, [255, 255, 255, 128]), (4, [255, 255, 255, 16])];
            self.layer(flash, None)?;
            let mut cloud = Layer::legacy([72, 74, 72][index], 30);
            cloud.blend = Blend::Alpha;
            cloud.delay = [2, 4, 6][index];
            cloud.position[2] = distance;
            cloud.facing = Facing::Camera;
            cloud.scale = vec![(0, [1.; 3]), (5, [6., 6., 1.]), (30, [10., 10., 1.])];
            cloud.color = vec![(0, white), (15, white), (30, [255, 255, 255, 0])];
            cloud.movement.push((30, [0., 15., 0.]));
            cloud.rotation_range[2] = core::f32::consts::PI;
            self.layer(cloud, None)?;
            let mut debris = Layer::legacy(141, 30);
            debris.blend = Blend::Alpha;
            debris.position[2] = distance;
            debris.delay = 5 + delay + [0, 1, 2][index];
            debris.rotation_range[2] = core::f32::consts::PI;
            debris.scale = vec![(0, [0.1; 3]), (5, [0.4; 3]), (30, [0.7; 3])];
            debris.movement = (0..=30)
                .map(|t| {
                    let n = t as f32;
                    (t, [0., 15. * n - 1.5 * n * (n - 1.), 0.])
                })
                .collect();
            self.layer(debris, None)?;
        }
        self.route(first);
        Ok(())
    }

    pub fn dual_blades(&mut self, params: &[u8]) -> Result<()> {
        // After 4021:1, keep the common routes in the order used by fx.rs.
        // 81F867C0 selects one of three sparks and draws 76 at the blade.
        for mesh in 32..=34 {
            let first = self.count;
            let mut spark = Layer::legacy(mesh, 4);
            spark.position = [0., 0., 50.];
            spark.facing = Facing::Camera;
            spark.scale = vec![
                (0, [0.5, 0.5, 1.]),
                (1, [0.5, 0.5, 1.]),
                (2, [1.; 3]),
                (4, [0.2, 0.2, 1.]),
            ];
            self.layer(spark, None)?;
            let mut rays = Layer::legacy(76, 12);
            rays.color.push((12, [255, 255, 255, 0]));
            self.layer(rays, None)?;
            self.route(first);
        }

        // 81F8BF88 type 18 requests four simultaneous flashes at node 14.
        let first = self.count;
        let mut flash = Layer::legacy(CHARGE_AURA_MESH, 14);
        flash.count = 4;
        flash.blend = Blend::Alpha;
        flash.facing = Facing::Camera;
        flash.color = vec![(0, [255, 255, 255, 96]), (14, [255, 255, 255, 0])];
        self.layer(flash, None)?;
        self.route(first);

        // 81F89FAC type 1: the timed white trail at the sharpening slash.
        let first = self.count;
        let slash = Layer::legacy(501, 8);
        self.layer(slash, None)?;
        self.route(first);

        // The original shader colours are ARGB. The extreme variants come
        // from the dual-blades parameter block at offsets 172 and 176.
        let rgba = |offset| {
            let [alpha, red, green, blue] = word(params, offset).to_be_bytes();
            [red, green, blue, alpha]
        };
        let modes = [
            ([255, 51, 51, 51], [255, 51, 51, 255]),
            ([255, 100, 20, 51], [255, 100, 20, 255]),
            (rgba(172), rgba(176)),
        ];
        for (glow_color, _) in modes {
            let first = self.count;
            let mut glow = Layer::legacy(59, 8);
            // Both blades refresh before the previous pair fades. Composite their
            // coverage before GU's bloom pass, as for the sustained charge aura.
            glow.blend = Blend::Alpha;
            glow.color = vec![(0, glow_color), (6, glow_color), (8, [0; 4])];
            self.layer(glow, None)?;
            self.route(first);
        }
        for hand in [502, 503] {
            for (_, color) in modes {
                let first = self.count;
                let mut trail = Layer::legacy(hand, 6);
                trail.blend = Blend::Alpha;
                trail.color = vec![(0, color)];
                self.layer(trail, None)?;
                self.route(first);
            }
        }
        Ok(())
    }

    pub fn dual_blade_swing(&mut self) -> Result<()> {
        // 817BCEE4 sends types 1/2 to 81F89FAC for up to 27 logic ticks.
        for hand in [504, 505] {
            let first = self.count;
            self.layer(Layer::legacy(hand, 27), None)?;
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
            name: format!(r"effect\{}\particles", self.namespace),
            kind: PEL,
            data: pel,
        });
        (self.assets, self.routes)
    }
}
