//! Shared player FX from Frontier effect.bin. Keep emission records and curves
//! in their native order; several effects use multiple definitions with one ID.
mod native;
mod sheet;
use crate::{model, read, save};
use anyhow::{Context, Result, ensure};
use mhf_resource::{
    fmod::{ADDRESS_WORD, FILTER_WORD, Fmod},
    image, pac,
};
use mhgu_resource::{
    arc::{self, Asset},
    tex,
};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    fs,
    path::Path,
};

const BANKS: &[(u16, &[u16])] = &[(4006, &[0, 1, 2, 3, 4, 5, 6]), (4022, &[5, 21])];
const CHARGE_AURA_MESH: u16 = 66;

fn half(data: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        data.get(at..at + 2)
            .context("short effect record")?
            .try_into()?,
    ))
}

// The first generator in w00_000 uses mask 1. Use its EFL proof for our
// single-generator lists, including GU's attachment and lifetime defaults.
fn particle_proof(weapon: &[arc::Entry<'_>]) -> Result<[u8; 160]> {
    let pel = weapon
        .iter()
        .find(|e| e.kind == 0x5a525c16 && e.name == r"effect\pel\pl\w00_000")
        .context("missing great sword proof list")?
        .unpack()?;
    ensure!(
        pel.starts_with(b"PEL\0\x0f\0\0\0"),
        "unexpected native PEL version"
    );
    let mut tail = &pel[8..];
    for _ in 0..3 {
        let count = u32::from_le_bytes(tail.get(..4).context("short PEL group")?.try_into()?);
        tail = &tail[4..];
        for _ in 0..count {
            let (len, rest) = tail.split_first().context("short PEL name")?;
            let (name, rest) = rest
                .split_at_checked(usize::from(*len))
                .context("short PEL name")?;
            let (len, rest) = rest.split_first().context("short PEL script")?;
            let (_, rest) = rest
                .split_at_checked(usize::from(*len))
                .context("short PEL script")?;
            let (body, next) = rest.split_at_checked(160).context("short PEL record")?;
            tail = next;
            if name == br"effect\pl\w00\w00_000"
                && body[61] == 0
                && body[72..76] == 1u32.to_le_bytes()
            {
                return Ok(body.try_into()?);
            }
        }
    }
    anyhow::bail!("missing great sword EFL proof for generator 0")
}

fn charge_aura_texture(source: &image::Image) -> Result<Vec<u8>> {
    // The sprite has opaque near-black texels. Smoothly fade their coverage
    // between RGB intensities 12 and 96 while preserving the bright core.
    let mut rgba = source.rgba.clone();
    for [red, green, blue, alpha] in rgba.as_chunks_mut::<4>().0 {
        let intensity = f32::from((*red).max(*green).max(*blue));
        let t = ((intensity - 12.) / (96. - 12.)).clamp(0., 1.);
        let coverage = t * t * (3. - 2. * t);
        *alpha = (f32::from(*alpha) * coverage).round() as u8;
    }
    tex::rgba8_unmipped(source.width, source.height, &rgba)
}

pub fn extract(dat: &Path, out: &Path, gu_weapon: &Path) -> Result<()> {
    let bundle = pac::decode(read(&dat.join("effect.bin"))?)?;
    let banks = pac::member(&bundle, 18)?;
    let directory = pac::member(&banks, 0)?;
    let gu = read(gu_weapon)?;
    let weapon = arc::entries(&gu)?;
    let mut playback = native::Compiler::new(&weapon, particle_proof(&weapon)?)?;
    fs::create_dir_all(out)?;
    for &(id, selectors) in BANKS {
        let slot = directory
            .get(4..)
            .context("short effect index")?
            .chunks_exact(4)
            .take(usize::from(half(&directory, 2)?))
            .position(|r| r[..2] == 1u16.to_le_bytes() && r[2..] == id.to_le_bytes())
            .with_context(|| format!("missing effect bank {id}"))?;
        let data = pac::member(&banks, slot + 1)?;
        playback.bank(&data, id, selectors)?;
        save(&out.join(format!("{id}.bank")), data)?;
    }
    playback.charge()?;
    let pair = pac::member(&bundle, 0)?;
    let bytes = pac::member(&pair, 0)?;
    let model = Fmod::parse(&bytes)?;
    let objects: Vec<_> = model.objects().collect();
    let mats: Vec<_> = model.materials().collect();
    let textures: Vec<_> = model.textures().collect();
    let images = pac::member(&bundle, 1)?;
    let mut atlases = BTreeMap::new();
    let mut assets = vec![];
    let mrl = weapon
        .iter()
        .find(|e| e.kind == arc::MATERIAL && e.name == r"effect\base\cm153_000")
        .context("missing GU effect material")?
        .unpack()?;
    let mut bindings = String::from("mesh\tmaterial\ttexture_slot\tatlas\n");
    for &id in &playback.meshes {
        let obj = *objects.get(id).context("missing effect mesh")?;
        let mut mesh = model::static_mesh(obj).with_context(|| format!("effect mesh {id}"))?;
        // The model converter uses OBJ-style flipped V for skinned assets.
        // Effect atlases retain the original top-down PNG orientation: flipping
        // V selects a different sprite, not a vertically mirrored copy.
        let original_uv = &obj.uvs().context("missing FX UV")?.values;
        for (vertex, uv) in mesh.vertices.iter_mut().zip(original_uv) {
            vertex.uv = glam::Vec2::from_array(*uv);
        }
        let (index, strip) = mesh.strips.pop_first().context("missing FX draw strip")?;
        ensure!(
            mesh.strips.is_empty(),
            "unsupported layered material on FX mesh {id}"
        );
        mesh.strips.insert(0, strip);
        let mat = mats.get(index as usize).context("missing FX material")?;
        let render = model
            .rendering_block(id)?
            .context("missing FX render state")?;
        ensure!(
            render.words[FILTER_WORD] == 0,
            "unsupported FX texture filter"
        );
        let wrap = match render.words[ADDRESS_WORD] {
            0 => true,
            1 => false,
            mode => anyhow::bail!("unsupported FX texture addressing {mode}"),
        };
        let [ordinal] = mat.texture_indices.as_slice() else {
            anyhow::bail!("expected one effect atlas");
        };
        let img = textures
            .get(*ordinal as usize)
            .context("missing FX texture binding")?
            .image_id;
        let path = format!(r"effect\frontier_gs\m{id}");
        let atlas = format!(r"effect\frontier_gs\t{img}");
        let png = match atlases.entry(img) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let png = image::decode(&pac::member(&images, img as usize)?)?;
                assets.push(Asset {
                    name: atlas.clone(),
                    kind: arc::TEXTURE,
                    data: tex::rgba8_unmipped(png.width, png.height, &png.rgba)?,
                });
                entry.insert(png)
            }
        };
        let material_atlas = if id == usize::from(CHARGE_AURA_MESH) {
            let masked_atlas = format!("{path}_alpha");
            assets.push(Asset {
                name: masked_atlas.clone(),
                kind: arc::TEXTURE,
                data: charge_aura_texture(png)?,
            });
            masked_atlas
        } else {
            atlas
        };
        let name = model::material_name(index as usize);
        assets.push(Asset {
            name: path.clone(),
            kind: arc::MATERIAL,
            data: mhgu_resource::mrl::effect(&mrl, &name, &material_atlas, wrap)?,
        });
        bindings.push_str(&format!("{id}\t{index}\t0\t{img}\n"));
        // FMOD stores colours as floating-point byte values (0..255).
        let colors = obj.colors().map(|c| {
            c.values
                .iter()
                .map(|rgba| rgba.map(|x| x / 255.))
                .collect::<Vec<_>>()
        });
        let names = [name];
        assets.push(Asset {
            name: path,
            kind: arc::MODEL,
            data: mhgu_resource::model::static_colored(&mesh, &names, colors.as_deref())?,
        });
        for sheet in playback.sheets.iter().filter(|s| usize::from(s.mesh) == id) {
            let path = sheet.path();
            assets.push(Asset {
                name: path.clone(),
                kind: arc::TEXTURE,
                data: sheet.texture(&mut mesh, original_uv, png, wrap)?,
            });
            assets.push(Asset {
                name: path.clone(),
                kind: arc::MATERIAL,
                data: mhgu_resource::mrl::effect(&mrl, &names[0], &path, false)?,
            });
            assets.push(Asset {
                name: path,
                kind: arc::MODEL,
                data: mhgu_resource::model::static_colored(&mesh, &names, colors.as_deref())?,
            });
        }
    }
    let (effects, routes) = playback.finish();
    assets.extend(effects);
    save(&out.join("routes.bin"), routes)?;
    fs::write(out.join("bindings.tsv"), bindings)?;
    save(&out.join("geometry.arc"), arc::encode(&assets)?)
}
