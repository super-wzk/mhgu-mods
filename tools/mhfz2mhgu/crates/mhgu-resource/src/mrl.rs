//! Build GU 0x22 materials from static diffuse templates in a native MRL.
use crate::{arc::TEXTURE, binary::Fields};
use anyhow::{Context, Result, ensure};

const ALBEDO: u32 = 0xcd06f371;
const SPECULAR: u32 = 0x0ed1b385;
fn word(b: &[u8], p: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        b.get(p..p + 4).context("short MRL field")?.try_into()?,
    ))
}
pub struct Material {
    pub name: String,
    pub albedo: usize,
    pub cutout: bool,
}
/// Retarget a stock GU model-particle material to a Frontier atlas.
pub fn effect(template: &[u8], name: &str, atlas: &str, wrap: bool) -> Result<Vec<u8>> {
    ensure!(
        template.starts_with(b"MRL\0\x22\0\0\0")
            && word(template, 8)? == 1
            && word(template, 12)? == 1,
        "expected single-atlas effect material"
    );
    let texture = word(template, 20)? as usize;
    let material = word(template, 24)? as usize;
    ensure!(
        word(template, texture)? == TEXTURE && atlas.len() < 64,
        "invalid effect texture"
    );
    let mut out = template.to_vec();
    let path = out
        .get_mut(texture + 12..texture + 76)
        .context("short effect material path")?;
    path.fill(0);
    path[..atlas.len()].copy_from_slice(atlas.as_bytes());
    out.get_mut(material + 4..material + 8)
        .context("short material hash")?
        .copy_from_slice(&(!crc32fast::hash(name.as_bytes())).to_le_bytes());
    let block = word(template, material + 52)? as usize;
    for i in 0..(word(template, material + 24)? & 0xfff) as usize {
        let p = block + 12 * i;
        // Preserve the precompiled cm153_000 shader permutation. It reads
        // CBPrimEflEmu when paired with the stock IANonSkinBC vertex format.
        if word(template, p)? & 15 == 2 && word(template, p + 8)? == 0xcaa79222 {
            // AppShaderPackage's SSWrapLinear and SSClampLinear descriptors.
            out.patch(p + 4, if wrap { 0x7100317eu32 } else { 0x11dae184 });
        }
    }
    Ok(out)
}
/// Assemble the stock model-particle shader for every draw strip of one FX mesh.
pub fn effect_set(
    template: &[u8],
    names: &[String],
    paths: &[String],
    wrap: bool,
) -> Result<Vec<u8>> {
    ensure!(
        !names.is_empty() && names.len() == paths.len(),
        "missing FX material binding"
    );
    if names.len() == 1 {
        return effect(template, &names[0], &paths[0], wrap);
    }
    let count = names.len();
    let rows = 28 + 76 * count;
    let mut out = template.get(..28).context("short effect MRL")?.to_vec();
    out.resize(rows + 60 * count, 0);
    out.patch(8, u32::try_from(count)?);
    out.patch(12, u32::try_from(count)?);
    out.patch(20, 28u32);
    out.patch(24, u32::try_from(rows)?);
    for (i, (name, path)) in names.iter().zip(paths).enumerate() {
        let single = effect(template, name, path, wrap)?;
        let texture = word(&single, 20)? as usize;
        let source_row = word(&single, 24)? as usize;
        let source_block = word(&single, source_row + 52)? as usize;
        let size = word(&single, source_row + 8)? as usize;
        let row = rows + 60 * i;
        out[28 + 76 * i..28 + 76 * (i + 1)].copy_from_slice(&single[texture..texture + 76]);
        out[row..row + 60].copy_from_slice(&single[source_row..source_row + 60]);
        out.resize(out.len().next_multiple_of(16), 0);
        let block = out.len();
        out.extend_from_slice(
            single
                .get(source_block..source_block + size)
                .context("short FX material block")?,
        );
        out.patch(row + 52, u32::try_from(block)?);
        // The original MRL has one bound texture. Keep unbound samplers at zero.
        for param in 0..(word(&out, row + 24)? & 0xfff) as usize {
            let field = block + param * 12;
            if word(&out, field)? & 15 == 3 && word(&out, field + 4)? == 1 {
                out.patch(field + 4, u32::try_from(i + 1)?);
            }
        }
    }
    Ok(out)
}

struct Template<'a> {
    row: &'a [u8],
    block: &'a [u8],
    slots: Vec<(usize, u32)>,
}

fn templates(src: &[u8]) -> Result<[Option<Template<'_>>; 2]> {
    ensure!(src.starts_with(b"MRL\0\x22\0\0\0"), "expected GU MRL v0x22");
    let mut variants = [None, None];
    let count = word(src, 8)? as usize;
    let start = word(src, 24)? as usize;
    for row in src
        .get(start..start + 60 * count)
        .context("invalid MRL material table")?
        .as_chunks::<60>()
        .0
        .iter()
    {
        if word(row, 48)? != 0 {
            continue;
        }
        let start = word(row, 52)? as usize;
        let block = src
            .get(start..start + word(row, 8)? as usize)
            .context("invalid MRL parameter block")?;
        let n = (word(row, 24)? & 0xfff) as usize;
        let mut slots = vec![];
        for i in 0..n {
            if word(block, 12 * i)? & 15 == 3 {
                slots.push((12 * i + 4, word(block, 12 * i + 8)?));
            }
        }
        if slots.iter().any(|&(_, h)| h == ALBEDO)
            && slots.iter().all(|&(_, h)| matches!(h, ALBEDO | SPECULAR))
        {
            let cut = ((word(row, 28)? >> 24) & 1) as usize;
            variants[cut].get_or_insert(Template { row, block, slots });
        }
    }
    Ok(variants)
}

pub fn encode(
    template: &[u8],
    paths: &[String],
    materials: &[Material],
    specular: usize,
) -> Result<Vec<u8>> {
    let choices = templates(template)?;
    let mat_start = 28 + 76 * paths.len();
    ensure!(specular < paths.len(), "missing specular texture");
    let mut out = b"MRL\0".to_vec();
    for n in [
        0x22,
        materials.len().try_into()?,
        paths.len().try_into()?,
        word(template, 16)?,
        28,
        mat_start.try_into()?,
    ] {
        out.u32(n);
    }
    for p in paths {
        ensure!(
            p.len() < 64 && p.is_ascii() && !p.contains('\0'),
            "invalid MRL texture path"
        );
        out.u32(TEXTURE);
        out.u32(0);
        out.u32(0);
        out.extend(p.bytes());
        out.resize(out.len() + 64 - p.len(), 0);
    }
    out.resize(mat_start + 60 * materials.len(), 0);
    for (i, m) in materials.iter().enumerate() {
        ensure!(m.albedo < paths.len(), "missing albedo for {}", m.name);
        let t = choices[m.cutout as usize].as_ref().with_context(|| {
            format!(
                "no static {} albedo template (use em030_00.arc)",
                if m.cutout { "alpha-tested" } else { "opaque" }
            )
        })?;
        let row = mat_start + i * 60;
        out[row..row + 60].copy_from_slice(t.row);
        out.patch(row + 4, !crc32fast::hash(m.name.as_bytes()));
        out.resize(out.len().next_multiple_of(16), 0);
        let pos = out.len();
        out.patch(row + 52, pos.try_into()?);
        out.patch(row + 56, 0);
        out.extend(t.block);
        for &(off, h) in &t.slots {
            let idx = if h == ALBEDO { m.albedo } else { specular };
            out.patch(pos + off, (idx + 1).try_into()?);
        }
    }
    Ok(out)
}
