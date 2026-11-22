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
const SNS_STRIPS: &[u16] = &[501, 504, 505, 506, 507];
const CHARGE_AURA_MESH: u16 = 66;

#[derive(Clone, Copy)]
enum WeaponEffect {
    SwordShield,
    Gunlance,
    SwitchAxe,
    GreatSword,
    DualBlades,
    LongSword,
}

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
    convert(dat, out, gu_weapon, WeaponEffect::GreatSword)
}

pub fn long_sword(dat: &Path, out: &Path, gu_weapon: &Path) -> Result<()> {
    convert(dat, out, gu_weapon, WeaponEffect::LongSword)
}

pub fn gunlance(dat: &Path, out: &Path, gu_weapon: &Path) -> Result<()> {
    convert(dat, out, gu_weapon, WeaponEffect::Gunlance)
}

pub fn sword_shield(dat: &Path, out: &Path, gu_weapon: &Path) -> Result<()> {
    convert(dat, out, gu_weapon, WeaponEffect::SwordShield)
}

pub fn switch_axe(dat: &Path, out: &Path, gu_weapon: &Path) -> Result<()> {
    convert(dat, out, gu_weapon, WeaponEffect::SwitchAxe)
}

pub fn dual_blades(dat: &Path, out: &Path, gu_weapon: &Path) -> Result<()> {
    convert(dat, out, gu_weapon, WeaponEffect::DualBlades)
}

const SWITCH_AXE_PHIAL: &[u16] = &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31];

fn convert(dat: &Path, out: &Path, gu_weapon: &Path, weapon_effect: WeaponEffect) -> Result<()> {
    let (namespace, effects): (_, &[(u16, &[u16])]) = match weapon_effect {
        WeaponEffect::SwordShield => ("frontier_sns", &[
            (3013, &[10]), (4020, &[0, 4, 5, 9, 11, 12, 13, 17, 18]),
            (4035, &[2, 3, 7]), (4036, &[2, 7]),
        ]),
        WeaponEffect::Gunlance => (
            "frontier_gl",
            &[
                (4027, &[0, 1, 2, 3, 4, 5, 6, 7, 11, 13, 14, 15, 16, 17, 18, 22, 23, 24, 25]),
                (4035, &[2, 4, 11, 16]),
                (4036, &[2, 4, 11, 16]),
            ],
        ),
        WeaponEffect::GreatSword => ("frontier_gs", BANKS),
        WeaponEffect::SwitchAxe => ("frontier_sa", &[
            (4013, &[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27]),
            (4014, &[0,1,2,3,4,5,6,7,8,10,11,12,13,16,17,18,19,20,21,22,23,24,25,26]),
            (4015, &[0,1,2,3,4,13]),
            (4035, &[0,5,6,7,8,10,11,12,13,14,15,16,17]),
            (4036, &[0,5,6,7,8,10,11,12,13,14,15,16,17]),
            (4019, SWITCH_AXE_PHIAL),
            (4018, SWITCH_AXE_PHIAL),
            (4016, SWITCH_AXE_PHIAL),
            (4017, SWITCH_AXE_PHIAL),
        ]),
        WeaponEffect::LongSword => (
            "frontier_ls",
            &[
                (4007, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]),
                (
                    4023,
                    &[
                        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
                    ],
                ),
                (4035, &[2, 6]),
                (4036, &[2, 6]),
            ],
        ),
        WeaponEffect::DualBlades => ("frontier_db", &[(4021, &[1])]),
    };
    let bundle = pac::decode(read(&dat.join("effect.bin"))?)?;
    let banks = pac::member(&bundle, 18)?;
    let directory = pac::member(&banks, 0)?;
    let bank_entries = directory
        .get(4..)
        .context("short effect index")?
        .chunks_exact(4)
        .take(usize::from(half(&directory, 2)?));
    let load_bank = |id: u16| -> Result<Vec<u8>> {
        let slot = bank_entries
            .clone()
            .position(|r| r[..2] == 1u16.to_le_bytes() && r[2..] == id.to_le_bytes())
            .with_context(|| format!("missing effect bank {id}"))?;
        pac::member(&banks, slot + 1)
    };
    let gu = read(gu_weapon)?;
    let weapon = arc::entries(&gu)?;
    let mut playback = native::Compiler::new(&weapon, particle_proof(&weapon)?, namespace)?;
    fs::create_dir_all(out)?;
    let mhfdat = pac::decode(read(&dat.join("mhfdat.bin"))?)?;
    for &(id, selectors) in effects {
        let data = load_bank(id)?;
        if id == 3013 && matches!(weapon_effect, WeaponEffect::SwordShield) {
            let directory = u32::from_le_bytes(mhfdat.get(1836..1840).context("missing parameter directory")?.try_into()?) as usize;
            let at = directory + 61 * 32;
            let speed = f32::from_le_bytes(mhfdat.get(at..at+4).context("missing vacuum speed")?.try_into()?);
            playback.moving_bank(&data,id,selectors,speed)?;
        } else {
            playback.bank(&data, id, selectors)?;
        }
        if id == 4022 {
            playback.charge()?;
            playback.charged_slash()?;
            playback.ground_impacts()?;
            // 817C08AA: clip 1476 also uses selector 5 from the initial bank routes.
            playback.bank(&data, id, &[10, 16, 6])?;
        }
        if id == 4023 {
            playback.discharge_water()?;
            playback.sword_flash(0)?;
            playback.sword_flash(17)?;
            playback.parry_impact()?;
        }
        save(&out.join(format!("{id}.bank")), data)?;
    }
    let dual_blades = matches!(weapon_effect, WeaponEffect::DualBlades);
    let weapon_id = match weapon_effect {
        WeaponEffect::Gunlance => {
            playback.footfalls(0.03)?;
            playback.sword_flash(0)?;
            playback.gunlance_blade()?;
            playback.gunlance_shell_feedback()?;
            9
        }
        WeaponEffect::SwordShield => {
            playback.footfalls(0.)?;
            for &id in SNS_STRIPS {
                playback.blade_strip(id)?;
            }
            playback.round_force_ring()?;
            4
        }
        WeaponEffect::SwitchAxe => {
            let mut keys = Vec::new();
            for &(bank, selectors) in effects {
                for selector in selectors {
                    keys.extend(bank.to_le_bytes());
                    keys.extend(selector.to_le_bytes());
                }
            }
            save(&out.join("selectors.bin"), keys)?;
            playback.footfalls(0.)?;
            playback.switch_axe_cartridge()?;
            12
        }
        WeaponEffect::GreatSword => {
            let weapon = load_bank(4022)?;
            let dry = load_bank(4035)?;
            let wet = load_bank(4036)?;
            playback.bank(&weapon, 4022, &[4, 9, 11, 12, 13])?;
            playback.bank(&dry, 4035, &[3, 4, 14])?;
            playback.bank(&wet, 4036, &[4, 14])?;
            // Source clips 1469 (sprint slash) and 1474 (successful guard).
            playback.bank(&weapon, 4022, &[15, 17])?;
            playback.bank(&dry, 4035, &[15])?;
            playback.bank(&wet, 4036, &[15])?;
            0
        }
        WeaponEffect::DualBlades => {
            let params = crate::hunter::dual_blades_parameters(&mhfdat)?;
            playback.dual_blades(params)?;
            playback.bank(&load_bank(4021)?, 4021, &[2, 3])?;
            for bank in [4035, 4036] {
                playback.bank(&load_bank(bank)?, bank, &[2, 5, 6, 12, 13])?;
            }
            playback.dual_blade_swing()?;
            playback.bank(&load_bank(4002)?, 4002, &[2])?;
            playback.bank(&load_bank(4021)?, 4021, &[0])?;
            for bank in [4035, 4036] {
                playback.bank(&load_bank(bank)?, bank, &[4])?;
            }
            playback.dual_blade_glows(params, true)?;
            6
        }
        WeaponEffect::LongSword => {
            for mesh in 504..=512 {
                playback.blade_strip(mesh)?;
            }
            playback.bank(&load_bank(4035)?, 4035, &[11, 13, 14])?;
            playback.footfalls(0.)?;
            playback.spirit_full()?;
            playback.bank(&load_bank(4036)?, 4036, &[11, 13, 14])?;
            7
        }
    };
    save(&out.join("motion_fx.bin"), motion_fx(&mhfdat, weapon_id)?)?;
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
    let main_strip: &[u16] = match weapon_effect {
        WeaponEffect::SwordShield => SNS_STRIPS,
        WeaponEffect::DualBlades => &[501, 504, 505],
        WeaponEffect::LongSword => &[501, 504, 505, 506, 507, 508, 509, 510, 511, 512],
        _ => &[],
    };
    if !main_strip.is_empty() {
        let span = blade_trails(&bundle, &mrl, &mut assets, namespace, dual_blades, main_strip)?;
        if !dual_blades {
            save(&out.join("trail_span.bin"), span.to_le_bytes().to_vec())?;
        }
        for &id in main_strip.iter().chain(if dual_blades { &[502, 503][..] } else { &[] }) {
            playback.meshes.remove(&usize::from(id));
        }
    }
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
        let path = format!(r"effect\{namespace}\m{id}");
        let mut names = Vec::new();
        let mut material_atlases = Vec::new();
        let mut images_used = Vec::new();
        for (index, strip) in std::mem::take(&mut mesh.strips) {
            mesh.strips.insert(u32::try_from(names.len())?, strip);
            let mat = mats.get(index as usize).context("missing FX material")?;
            let [ordinal] = mat.texture_indices.as_slice() else {
                anyhow::bail!("expected one effect atlas");
            };
            let img = textures
                .get(*ordinal as usize)
                .context("missing FX texture binding")?
                .image_id;
            let atlas = format!(r"effect\{namespace}\t{img}");
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
            let material_atlas = if id == usize::from(CHARGE_AURA_MESH)
                && !matches!(weapon_effect, WeaponEffect::GreatSword)
            {
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
            names.push(name);
            images_used.push(img);
            material_atlases.push(material_atlas);
            bindings.push_str(&format!("{id}\t{index}\t0\t{img}\n"));
        }
        assets.push(Asset {
            name: path.clone(), kind: arc::MATERIAL,
            data: mhgu_resource::mrl::effect_set(&mrl,&names,&material_atlases,wrap)?,
        });
        // FMOD stores colours as floating-point byte values (0..255).
        let colors = obj.colors().map(|c| {
            c.values
                .iter()
                .map(|rgba| rgba.map(|x| x / 255.))
                .collect::<Vec<_>>()
        });
        assets.push(Asset {
            name: path,
            kind: arc::MODEL,
            data: mhgu_resource::model::static_colored(&mesh, &names, colors.as_deref())?,
        });
        for sheet in playback.sheets.iter().filter(|s| usize::from(s.mesh) == id) {
            let [img] = images_used.as_slice() else { anyhow::bail!("animated atlas spans multiple materials on FX mesh {id}"); };
            let png = atlases.get(img).context("missing FX sheet image")?;
            let path = sheet.path(namespace);
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

// 817BE150 resolves the weapon's clip directory from mhfdat + 0x618.
// 817BD900 replays these records independently of the action's hit program.
fn motion_fx(data: &[u8], weapon_id: usize) -> Result<Vec<u8>> {
    let word = |at: usize| -> Result<usize> {
        Ok(u32::from_le_bytes(
            data.get(at..at + 4)
                .context("short motion FX directory")?
                .try_into()?,
        ) as usize)
    };
    let weapon = word(0x618)? + (weapon_id + 1) * 8;
    let clips = word(weapon + 4)?;
    let mut rows = Vec::new();
    for i in 0..usize::from(half(data, weapon)?) {
        let entry = clips + 8 * i;
        let clip = half(data, entry)?;
        let start = word(entry + 4)?;
        let size = 16 * usize::from(half(data, entry + 2)?);
        for row in data
            .get(start..start + size)
            .context("short motion FX track")?
            .chunks_exact(16)
        {
            let kind = half(row, 2)?;
            let bank = half(row, 4)?;
            let selector = half(row, 6)?;
            let keep = match (weapon_id, clip, kind) {
                (_, _, 8) => false, // 817BD8FE immediately returns.
                (0, 1465 | 1476, 5..=7) => (bank, selector) != (4022, 8), // Alpha stays zero.
                (0, 1469 | 1474, 5 | 6) => true,
                (6, 1444..=1447, 0 | 3 | 5 | 7) | (6, 1448, 2 | 5) | (6, 1450, 0) => true,
                (4 | 9, _, 1 | 3..=7) => true,
                (12, _, 1 | 5..=7) => true,
                (7, _, _) => match (kind, bank, selector, row[8], row[9]) {
                    (0, 5, _, 0, 0)
                    | (1, 0 | 3 | 4 | 7 | 10 | 20, 100, 0 | 5 | 8 | 10, 0)
                    | (3, 4023, 17, 10, 0)
                    | (5, 4035, 2 | 6 | 11 | 13 | 14, 0, 0 | 2 | 4)
                    | (6, 4035, 2, 0, 3)
                    | (6, 4023, 0..=18, 0, 0 | 2 | 3 | 4)
                    | (6, 4023, 0..=18, 10, 0 | 5) => true,
                    _ => anyhow::bail!("unsupported w07 motion FX row {clip}: {row:?}"),
                },
                _ => false,
            };
            if keep {
                rows.push((clip, row));
            }
        }
    }
    ensure!(!rows.is_empty(), "missing weapon {weapon_id} motion FX");
    rows.sort_by_key(|&(clip, _)| clip);
    let mut output = Vec::with_capacity(rows.len() * 18);
    for (clip, row) in rows {
        output.extend(clip.to_le_bytes());
        output.extend_from_slice(row);
    }
    Ok(output)
}

// FZ 81F8B4F8 uses model bank 4 (effect.bin members 8/9). Each
// successive pair of vertices is rigidly bound to one historical pose.
fn blade_trails(
    bundle: &[u8],
    mrl: &[u8],
    assets: &mut Vec<Asset>,
    namespace: &str,
    dual_blades: bool,
    main_strip: &[u16],
) -> Result<f32> {
    let pair = pac::member(bundle, 8)?;
    let bytes = pac::member(&pair, 0)?;
    let model = Fmod::parse(&bytes)?;
    let images = pac::member(bundle, 9)?;
    // The two demon trails bind separate runtime buffers backed by the same strip.
    let mut span = 0.;
    for (object, ids) in [(1, main_strip), (2, &[502, 503][..])] {
        if object == 2 && !dual_blades {
            continue;
        }
        let obj = model
            .objects()
            .nth(object)
            .context("missing FZ blade trail")?;
        let mut mesh = model::static_mesh(obj)?;
        if object == 1 {
            span = mesh
                .vertices
                .first()
                .context("empty blade strip")?
                .position
                .z;
            ensure!(span > 0., "invalid blade span");
        }
        let skin = &obj
            .weights()
            .context("missing blade history weights")?
            .vertices;
        ensure!(
            mesh.vertices.len() == 48 && skin.len() == 48,
            "unexpected blade strip size"
        );
        let uv = &obj.uvs().context("missing blade UVs")?.values;
        for (i, vert) in mesh.vertices.iter_mut().enumerate() {
            let [weight] = skin[i].influences.as_slice() else {
                anyhow::bail!("unexpected blade skin")
            };
            ensure!(
                weight.weight == 100. && weight.bone_index as usize == 25 - i / 2,
                "unexpected blade pose order"
            );
            // Remove the bind-pose spacing between adjacent bones. The
            // runtime places each vertex pair using that bone's sampled pose.
            vert.position.x = 0.;
            vert.uv = glam::Vec2::from_array(uv[i]);
        }
        let (mat, strip) = mesh.strips.pop_first().context("missing blade strip")?;
        ensure!(mesh.strips.is_empty(), "unexpected blade materials");
        mesh.strips.insert(0, strip);
        let mat = model
            .materials()
            .nth(mat as usize)
            .context("missing blade material")?;
        let [slot] = mat.texture_indices.as_slice() else {
            anyhow::bail!("unexpected blade textures")
        };
        let tex_id = model
            .textures()
            .nth(*slot as usize)
            .context("missing blade atlas")?
            .image_id;
        let atlas = format!(r"effect\{namespace}\blade_atlas{tex_id}");
        if !assets
            .iter()
            .any(|a| a.kind == arc::TEXTURE && a.name == atlas)
        {
            let png = image::decode(&pac::member(&images, tex_id as usize)?)?;
            assets.push(Asset {
                name: atlas.clone(),
                kind: arc::TEXTURE,
                data: tex::rgba8_unmipped(png.width, png.height, &png.rgba)?,
            });
        }
        let colors = obj.colors().map(|c| {
            c.values
                .iter()
                .map(|v| v.map(|x| x / 255.))
                .collect::<Vec<_>>()
        });
        let material = mhgu_resource::mrl::effect(mrl, "blade", &atlas, false)?;
        let strip =
            mhgu_resource::model::static_colored(&mesh, &["blade".into()], colors.as_deref())?;
        for id in ids {
            let name = format!(r"effect\{namespace}\m{id}");
            assets.push(Asset {
                name: name.clone(),
                kind: arc::MATERIAL,
                data: material.clone(),
            });
            assets.push(Asset {
                name,
                kind: arc::MODEL,
                data: strip.clone(),
            });
        }
    }
    Ok(span)
}
