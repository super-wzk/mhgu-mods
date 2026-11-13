//! Preserve a donor monster's logical joints and append a skinned MOD skeleton.
use crate::binary::Fields;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};

fn w(b: &[u8], p: usize) -> Result<usize> {
    Ok(u32::from_le_bytes(b.get(p..p + 4).context("short MOD field")?.try_into()?) as usize)
}
fn h(b: &[u8], p: usize) -> Result<usize> {
    Ok(u16::from_le_bytes(b.get(p..p + 2).context("short MOD count")?.try_into()?) as usize)
}
fn floats<const N: usize>(b: &[u8], p: usize) -> Result<[f32; N]> {
    let r = b.get(p..p + 4 * N).context("short MOD matrix")?;
    Ok(std::array::from_fn(|i| {
        f32::from_le_bytes(r[4 * i..4 * i + 4].try_into().unwrap())
    }))
}
fn cube(b: &[u8]) -> Result<Mat4> {
    let lo = Vec3::from_array(floats(b, 80)?);
    let hi = Vec3::from_array(floats(b, 96)?);
    let side = (hi - lo).max_element();
    ensure!(side > 0., "empty bind cube");
    Ok(Mat4::from_scale_rotation_translation(
        Vec3::splat(side),
        glam::Quat::IDENTITY,
        lo,
    ))
}
pub fn graft(donor: &[u8], model: &[u8], hidden: &[u16]) -> Result<(Vec<u8>, Vec<u8>)> {
    for b in [donor, model] {
        ensure!(b.starts_with(b"MOD\0\xd4\0"), "expected MOD 0xd4");
    }
    let (dn, fn_) = (h(donor, 6)?, h(model, 6)?);
    ensure!(dn + fn_ <= 255, "grafted skeleton exceeds MOD joint IDs");
    let (db, fb) = (w(donor, 36)?, w(model, 36)?);
    let table = donor
        .get(db + 152 * dn..db + 152 * dn + 256)
        .context("short donor skeleton")?;
    let mut remap = table.to_vec();
    let ids: Vec<u8> = (0..255u8)
        .filter(|&i| remap[i as usize] == 255)
        .take(fn_)
        .collect();
    ensure!(ids.len() == fn_, "no free logical bone IDs");
    let fg = w(model, 40)?;
    ensure!(
        fb + 152 * fn_ + 256 == fg && fg <= model.len(),
        "unexpected bind layout"
    );
    let mut out = model[..fb].to_vec();
    out[6..8].copy_from_slice(&((dn + fn_) as u16).to_le_bytes());
    out.extend(&donor[db..db + 24 * dn]);
    for (i, &id) in ids.iter().enumerate() {
        let mut r = model[fb + i * 24..fb + (i + 1) * 24].to_vec();
        r[0] = id;
        for v in &mut r[1..3] {
            if *v != 255 {
                *v += dn as u8;
            }
        }
        remap[id as usize] = (dn + i) as u8;
        out.extend(r);
    }
    out.extend(&donor[db + 24 * dn..db + 88 * dn]);
    out.extend(&model[fb + 24 * fn_..fb + 88 * fn_]);
    let scale = cube(donor)?.inverse() * cube(model)?;
    for i in 0..dn {
        let m = Mat4::from_cols_array(&floats(donor, db + 88 * dn + 64 * i)?);
        out.floats((m * scale).to_cols_array());
    }
    out.extend(&model[fb + 88 * fn_..fb + 152 * fn_]);
    out.extend(remap);
    let shift = out.len() - fg;
    out.extend(&model[fg..]);
    for field in (40..=60).step_by(4) {
        out.patch(field, (w(model, field)? + shift).try_into()?);
    }
    let vb = w(&out, 52)?;
    let ib = w(&out, 56)?;
    for v in out[vb..ib].as_chunks_mut::<28>().0 {
        for b in &mut v[16..20] {
            *b += dn as u8;
        }
    }
    let mesh = w(&out, 48)?;
    let nm = h(&out, 8)?;
    let env = mesh + 48 * nm;
    for i in 0..w(&out, 128)? {
        let p = env + 144 * i;
        let bone = w(&out, p)?;
        out.patch(p, (bone + dn) as u32);
    }
    // Group 1 in standard em080 is the detached-tail model on a separate root.
    // In a one-action encounter force the intact groups into the donor's always-visible body group.
    for i in 0..nm {
        let p = mesh + 48 * i;
        let group = w(&out, p + 4)? as u32;
        if hidden.contains(&((group & 0xfff) as u16)) {
            out[p..p + 2].fill(0);
            let flags = w(&out, p + 8)? as u32;
            out.patch(p + 8, flags & !1);
        }
        out.patch(p + 4, group & !0xfff);
    }
    Ok((out, ids))
}
