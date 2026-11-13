//! Evaluate the ordinary Espinas battle decision (state 0:3), up to the next
//! return to the main script. Requests are Frontier action IDs, not LMT slots.
use crate::{
    ai::{self, Entry, Graph},
    elf::Elf,
};
use anyhow::{Context, Result, bail};

pub struct Combat<'a> {
    graph: Graph<'a>,
    pub distances: [f32; 4],
}

pub struct Target {
    pub enraged: bool,
    pub airborne: bool,
    /// Cached player distance; None corresponds to player index -1.
    pub distance: Option<f32>,
    /// Absolute horizontal angle, in the game's 65536 units per revolution.
    pub angle: u16,
    pub random: u16,
}

impl<'a> Combat<'a> {
    pub fn espinas(src: &'a [u8]) -> Result<Self> {
        let graph = ai::scan(src, 80, 0, Entry(0, 3))?;
        for (k, s) in &graph.scripts {
            if let Some(issue) = &s.issue {
                bail!("FZ {k}: {issue}");
            }
        }
        // Ordinary 81CC4476 -> 8169C46A(profile 0); the alternate form uses
        // 81CC43D8 and a different table. Both use actor+2560 for the result.
        let elf = Elf::read(src)?;
        let table = elf.ptr(0x8257_f99c, 80 * 10)?;
        let mut distances = [0.; 4];
        for (i, d) in distances.iter_mut().enumerate() {
            *d = f32::from_bits(elf.ptr(table, i)?);
        }
        Ok(Self { graph, distances })
    }

    pub fn requests(&self, target: &Target) -> Result<Vec<(u8, u8)>> {
        let mut out = Vec::new();
        self.run(self.graph.root, target, &mut out)?;
        Ok(out)
    }

    fn run(&self, key: Entry, target: &Target, out: &mut Vec<(u8, u8)>) -> Result<()> {
        let script = self.graph.scripts.get(&key).context("unbound FZ call")?;
        let ops = &script.ops;
        let mut pc = 0;
        while let Some(&op) = ops.get(pc) {
            pc += 1;
            match op {
                // Select the current player. The caller supplies that player's geometry.
                [0x13] => {}
                [0x05, g, i, 0] => out.push((*g, *i)),
                [0x81, i] if key.0 == 0 => self.run(Entry(1, *i), target, out)?,
                [0x82, g, i] if key.0 == 1 => {
                    self.run(Entry(15 + u16::from(*g), *i), target, out)?
                }
                [0xff, end]
                    if *end
                        == match key.0 {
                            0 => 0,
                            1 => 1,
                            _ => 2,
                        } =>
                {
                    return Ok(());
                }
                [0x35, 0] | [0x09, 0] | [0x14, 0, _] => {
                    let yes = match op[0] {
                        0x35 => target.enraged,
                        0x09 => target.airborne,
                        _ => {
                            target.distance.is_some()
                                && target.angle >= (f32::from(op[2]) * 65536. / 360.).round() as u16
                        }
                    };
                    if !yes {
                        pc = marker(ops, pc, op[0], |m| matches!(m[1], 1 | 2))? + 1;
                    }
                }
                [kind @ (0x35 | 0x09 | 0x14), 1] => pc = marker(ops, pc, *kind, |m| m[1] == 2)? + 1,
                [0x35 | 0x09 | 0x14, 2] | [0x80 | 0x83, 255] => {}
                [0x83, 0, n] => {
                    let ds = self
                        .distances
                        .get(..usize::from(*n))
                        .context("distance case count")?;
                    let lane = target
                        .distance
                        .and_then(|d| ds.iter().position(|&k| k >= 0. && d <= k))
                        .unwrap_or(ds.len());
                    pc = marker(ops, pc, 0x83, |m| usize::from(m[1]) == lane + 1)? + 1;
                }
                [0x80, 0, n] => {
                    let mut total = 0u8;
                    for i in 1..=*n {
                        let k = marker(ops, pc, 0x80, |m| m[1] == i)?;
                        let weight = ops[k][2];
                        pc = k + 1;
                        total = total.wrapping_add(weight);
                        if weight == 255 || (weight != 0 && (target.random & 31) < u16::from(total))
                        {
                            break;
                        }
                    }
                }
                [kind @ (0x80 | 0x83), 1..=31, ..] => {
                    pc = marker(ops, pc, *kind, |m| m[1] == 255)? + 1
                }
                _ => bail!(
                    "FZ {key} at {:#010x}: unsupported battle instruction {op:02x?}",
                    script.address + ops[..pc - 1].iter().map(|x| x.len() as u32).sum::<u32>()
                ),
            }
        }
        bail!("missing return from FZ {key}")
    }
}

/// Look only at markers belonging to the current block, not nested blocks.
fn marker(ops: &[&[u8]], start: usize, kind: u8, matches: impl Fn(&[u8]) -> bool) -> Result<usize> {
    let end = if matches!(kind, 0x80 | 0x83) { 255 } else { 2 };
    let mut depth = 0;
    for (i, &op) in ops.iter().enumerate().skip(start) {
        if op[0] != kind {
            continue;
        }
        if op[1] == 0 {
            depth += 1;
        } else if depth == 0 {
            if matches(op) {
                return Ok(i);
            }
            if op[1] == end {
                break;
            }
        } else if op[1] == end {
            depth -= 1;
        }
    }
    bail!("missing marker in FZ {kind:02x} block")
}
