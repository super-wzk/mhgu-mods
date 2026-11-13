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
