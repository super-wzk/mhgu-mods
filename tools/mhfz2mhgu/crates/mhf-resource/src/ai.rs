//! Locate the Vita selector's bank table, then follow references from a native script entry.
//! The descriptor is an array of pointers, not a counted directory.
use crate::elf::Elf;
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    str::FromStr,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry(pub u16, pub u8);
impl fmt::Display for Entry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.0, self.1)
    }
}
impl FromStr for Entry {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let (g, i) = s.split_once(':').context("expected GROUP:SLOT")?;
        Ok(Self(g.parse()?, i.parse()?))
    }
}

impl Elf<'_> {
    fn banks(&self) -> Result<u32> {
        // cmp r1,#0x83; load config; bgt native-provider; movw/movt r2,table;
        // ldr r0,[r2,r0,lsl #2]; movs r2,#0; ldr r0,[r0,r1,lsl #2].
        let imm = |p: &[u8]| {
            let a = u16::from_le_bytes([p[0], p[1]]);
            let b = u16::from_le_bytes([p[2], p[3]]);
            u32::from(((a & 15) << 12) | ((a & 0x400) << 1) | ((b & 0x7000) >> 4) | (b & 255))
        };
        let mut found = BTreeSet::new();
        for code in self.executable_segments() {
            for p in code.windows(26).step_by(2) {
                if p[..6] == [0x83, 0x29, 0x18, 0x68, 0x40, 0x6b]
                    && p[7] == 0xdc
                    && u16::from_le_bytes([p[8], p[9]]) & 0xfbf0 == 0xf240
                    && p[11] & 0x8f == 2
                    && u16::from_le_bytes([p[12], p[13]]) & 0xfbf0 == 0xf2c0
                    && p[15] & 0x8f == 2
                    && p[16..] == [0x52, 0xf8, 0x20, 0x00, 0x00, 0x22, 0x50, 0xf8, 0x21, 0x00]
                {
                    found.insert(imm(&p[8..]) | (imm(&p[12..]) << 16));
                }
            }
        }
        ensure!(
            found.len() == 1,
            "expected one Vita monster bank selector, found {}",
            found.len()
        );
        Ok(found.pop_first().unwrap())
    }
}

#[derive(Debug)]
pub struct Script<'a> {
    pub address: u32,
    pub ops: Vec<&'a [u8]>,
    pub issue: Option<String>,
}
pub struct Graph<'a> {
    pub root: Entry,
    pub scripts: BTreeMap<Entry, Script<'a>>,
}
fn target(op: &[u8]) -> Option<Entry> {
    match op {
        [0x07, i] => Some(Entry(0, *i)),
        [0x81, i] => Some(Entry(1, *i)),
        [0x16, i] => Some(Entry(9, *i)),
        [0x82, g, i] => Some(Entry(15 + u16::from(*g), *i)),
        _ => None,
    }
}

// Logical byte width and, for conditional opcodes, the selector that closes the block.
// Compound headers exclude the first case marker; the native skip helper consumes both.
fn layout(bytes: &[u8]) -> Result<(usize, Option<u8>)> {
    let op = bytes[0];
    let selector = || bytes.get(1).copied().context("missing selector");
    Ok(match op {
        0x00
        | 0x04
        | 0x10..=0x13
        | 0x18..=0x19
        | 0x1e
        | 0x25
        | 0x2d
        | 0x30..=0x31
        | 0x4b..=0x4d
        | 0x52..=0x53
        | 0x58
        | 0x5f
        | 0x61
        | 0x65
        | 0x68
        | 0x7b
        | 0x7e
        | 0x84..=0x86
        | 0x92..=0x93 => (1, None),
        0x07
        | 0x0a
        | 0x0d
        | 0x16
        | 0x26
        | 0x2e
        | 0x40..=0x41
        | 0x48..=0x49
        | 0x4e..=0x50
        | 0x5b
        | 0x62
        | 0x81
        | 0x90..=0x91
        | 0x99
        | 0xff => (2, None),
        0x0c | 0x1a | 0x82 => (3, None),
        0x05 | 0x3f => (4, None),
        0x06 => (if selector()? == 3 { 5 } else { 4 }, None),
        0x17 => (5, None),
        0x0f => (6, None),
        0x24 => (
            match selector()? {
                0 => 3,
                1 => 2,
                _ => 1,
            },
            None,
        ),

        0x01..=0x03
        | 0x08..=0x09
        | 0x1f
        | 0x21
        | 0x28..=0x2a
        | 0x2f
        | 0x35
        | 0x37..=0x3c
        | 0x44..=0x45
        | 0x47
        | 0x4a
        | 0x51
        | 0x54..=0x55
        | 0x59
        | 0x5c..=0x5d
        | 0x60
        | 0x66..=0x67
        | 0x72
        | 0x77
        | 0x7c
        | 0x7f => (2, Some(2)),
        0x0b
        | 0x14
        | 0x1b
        | 0x22
        | 0x32
        | 0x36
        | 0x42
        | 0x5a
        | 0x5e
        | 0x63..=0x64
        | 0x69
        | 0x71
        | 0x74 => (if selector()? == 0 { 3 } else { 2 }, Some(2)),
        0x0e | 0x2b | 0x34 | 0x3d | 0x46 | 0x56 | 0x78 | 0x9a..=0x9c => {
            (if selector()? == 0 { 4 } else { 2 }, Some(2))
        }
        0x15 | 0x3e | 0x57 => (
            match selector()? {
                0 => 3,
                1 => 4,
                _ => 2,
            },
            Some(3),
        ),
        0x1c..=0x1d
        | 0x20
        | 0x23
        | 0x27
        | 0x2c
        | 0x33
        | 0x70
        | 0x73
        | 0x75..=0x76
        | 0x7a
        | 0x7d
        | 0x94 => (if selector()? <= 1 { 3 } else { 2 }, Some(3)),
        0x79 => (
            match selector()? {
                0 => 4,
                1 => 3,
                _ => 2,
            },
            Some(3),
        ),
        0x80 => (if selector()? == 0xff { 2 } else { 3 }, Some(0xff)),
        0x83 => (if selector()? == 0 { 3 } else { 2 }, Some(0xff)),
        _ => bail!("unrecognized FZ opcode {op:#04x}"),
    })
}

fn collect<'a>(bytes: &'a [u8], entry: Entry, s: &mut Script<'a>) -> Result<()> {
    let mut stack = vec![];
    let mut pos = 0;
    while pos < bytes.len() {
        let rest = &bytes[pos..];
        let va = u64::from(s.address) + pos as u64;
        let op = rest[0];
        let (n, close_selector) = layout(rest).with_context(|| format!("at {va:#010x}"))?;
        let ins = rest
            .get(..n)
            .with_context(|| format!("truncated opcode at {va:#010x}"))?;
        if let Some(close) = close_selector {
            let v = ins[1];
            let valid = match op {
                0x80 => v <= 32 || v == 255,
                0x83 => v <= 5 || v == 255,
                _ => v <= close,
            };
            ensure!(valid, "unknown selector {op:02x}/{v:02x} at {va:#010x}");
            if v == 0 {
                stack.push(op)
            } else {
                ensure!(
                    stack.last() == Some(&op),
                    "unmatched marker {op:02x}/{v:02x} at {va:#010x}"
                );
                if v == close {
                    stack.pop();
                }
            }
        }
        s.ops.push(ins);
        pos += n;
        let end = matches!(op, 0x00 | 0x04 | 0x07)
            || matches!(ins, [0xff, 0..=3 | 0xf5..=0xff])
            || (entry.0 == 1 && op == 0x81)
            || (entry.0 >= 15 && op == 0x82);
        if end && stack.is_empty() {
            return Ok(());
        }
    }
    bail!("ELF segment ends before script {entry} terminates")
}

pub fn scan(src: &[u8], monster: u8, bank: u8, entry: Entry) -> Result<Graph<'_>> {
    let elf = Elf::read(src)?;
    let banks = elf.banks()?;
    ensure!(
        monster <= 0x83,
        "Vita monsters above 131 use native descriptor providers, not this ELF bank"
    );
    let set = elf.ptr(banks, bank.into())?;
    ensure!(set != 0, "FZ bank {bank} is empty");
    let descriptor = elf.ptr(set, monster.into())?;
    ensure!(
        descriptor != 0,
        "no descriptor for monster {monster} in bank {bank}"
    );
    let mut scripts = BTreeMap::new();
    let mut todo = BTreeSet::from([entry]);
    while let Some(k) = todo.pop_first() {
        if scripts.contains_key(&k) {
            continue;
        }
        let mut script = Script {
            address: 0,
            ops: vec![],
            issue: None,
        };
        let result = (|| -> Result<()> {
            let tab = elf.ptr(descriptor, k.0.into())?;
            ensure!(tab != 0, "null source table");
            let addr = elf.ptr(tab, k.1.into())?;
            ensure!(addr != 0, "null source slot (unbound in the ELF)");
            script.address = addr;
            collect(elf.tail(addr)?, k, &mut script)
        })();
        script.issue = result.err().map(|e| format!("{e:#}"));
        todo.extend(script.ops.iter().filter_map(|x| target(x)));
        scripts.insert(k, script);
    }
    Ok(Graph {
        root: entry,
        scripts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_operands_and_conditional_returns_on_instruction_boundaries() {
        let bytes = [
            0x35, 0, 5, 0x81, 0xff, 0, 0xff, 0, 0x35, 2, 0x81, 1, 0xff, 0, 0x81, 2,
        ];
        let mut s = Script {
            address: 0x1234,
            ops: vec![],
            issue: None,
        };
        collect(&bytes, Entry(0, 0), &mut s).unwrap();
        assert_eq!(s.ops.iter().map(|p| p.len()).sum::<usize>(), 14);
        assert_eq!(
            s.ops.iter().filter_map(|p| target(p)).collect::<Vec<_>>(),
            [Entry(1, 1)]
        );
    }
}
