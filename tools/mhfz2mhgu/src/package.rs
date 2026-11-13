use crate::{Monster, model, motion, read};
use anyhow::{Context, Result, ensure};
use mhf_resource::{fmod::Fmod, image, pac};
use mhgu_resource::{
    arc::{self, Asset},
    mrl, tex,
};
use std::{collections::BTreeMap, path::Path};

fn template(path: &Path) -> Result<Vec<u8>> {
    let data = read(path)?;
    if data.starts_with(b"MRL\0") {
        return Ok(data);
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("invalid template filename")?;
    let entries = arc::entries(&data)?;
    let m = entries
        .iter()
        .find(|e| {
            e.kind == arc::MATERIAL
                && e.name
                    .rsplit_once("\\mod\\")
                    .is_some_and(|(_, name)| name == stem)
        })
        .context("could not find the monster model MRL in the template ARC")?;
    m.unpack()
}

pub fn build(
    monster: &Monster,
    template_path: &Path,
    name: &str,
    texture_member: usize,
    motion_member: usize,
    group: usize,
) -> Result<Vec<u8>> {
    let (species, variant) = name
        .split_once('_')
        .context("expected a resource name like em150_00")?;
    ensure!(
        species.len() == 5
            && species.starts_with("em")
            && species[2..].bytes().all(|c| c.is_ascii_digit())
            && variant.len() == 2
            && variant.bytes().all(|c| c.is_ascii_digit()),
        "expected emNNN_VV"
    );
    let base = format!("enemy\\{species}\\{name}");
    let model_path = format!("{base}\\mod\\{name}");
    let (pac, pair, rig) = monster.open()?;
    let source = pac::member(&pair, 0)?;
    let f = Fmod::parse(&source)?;
    let images = pac::member(&pac, texture_member)?;
    let textures: Vec<_> = f.textures().collect();
    let mut assets = vec![];
    let mut paths = vec![];
    let mut by_id = BTreeMap::new();
    let mut materials = vec![];
    for (i, mat) in f.materials().enumerate() {
        let [ordinal] = mat.texture_indices.as_slice() else {
            anyhow::bail!(
                "material {i}: expected one diffuse texture, found {}",
                mat.texture_indices.len()
            );
        };
        let binding = textures
            .get(*ordinal as usize)
            .with_context(|| format!("material {i}: missing texture {ordinal}"))?;
        let id = binding.image_id;
        let (idx, cutout, w, h) = if let Some(&record) = by_id.get(&id) {
            record
        } else {
            let img = image::decode(&pac::member(&images, id as usize)?)
                .with_context(|| format!("decode PAC image {id} as PNG"))?;
            let cutout = img.rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
            let path = format!("{model_path}_BM{id:03}");
            let record = (paths.len(), cutout, img.width, img.height);
            assets.push(Asset {
                name: path.clone(),
                kind: arc::TEXTURE,
                data: tex::rgba8(img.width, img.height, &img.rgba)?,
            });
            paths.push(path);
            by_id.insert(id, record);
            record
        };
        ensure!(
            (w, h) == (binding.width, binding.height),
            "material {i}: FMOD dimensions differ from image {id}"
        );
        materials.push(mrl::Material {
            name: model::material_name(i),
            albedo: idx,
            cutout,
        });
    }
    let specular = paths.len();
    let spec = format!("{model_path}_spec");
    paths.push(spec.clone());
    assets.push(Asset {
        name: spec,
        kind: arc::TEXTURE,
        data: tex::rgba8(1, 1, &[0, 0, 0, 255])?,
    });
    assets.push(Asset {
        name: model_path.clone(),
        kind: arc::MATERIAL,
        data: mrl::encode(&template(template_path)?, &paths, &materials, specular)?,
    });
    assets.push(Asset {
        name: model_path,
        kind: arc::MODEL,
        data: model::convert(&source, &rig)?,
    });
    assets.push(Asset {
        name: format!("{base}\\mot\\{name}_{group}"),
        kind: arc::MOTION,
        data: motion::convert(
            &pac::member(&pac, motion_member)?,
            &rig,
            group,
            None,
            None,
            None,
        )?,
    });
    arc::encode(&assets)
}
