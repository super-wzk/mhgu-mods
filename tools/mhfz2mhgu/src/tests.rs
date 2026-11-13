use super::{Monster, emc, model, motion};
use anyhow::Result;
use mhf_resource::{
    ai::{self, Entry, Graph, Script},
    pac,
};
use std::collections::BTreeMap;

fn word_at(bytes: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap())
}
fn gu_script(bytes: &[u8], group: usize, slot: usize) -> &[u8] {
    let table = word_at(bytes, 20 + group * 16) as usize;
    let n = word_at(bytes, table + slot * 16) as usize;
    let start = word_at(bytes, table + slot * 16 + 4) as usize;
    &bytes[start..start + n]
}
#[test]
fn mapped_calls_use_native_gu_returns() {
    let root = Entry(0, 0);
    let mut graph = Graph {
        root,
        scripts: BTreeMap::from([
            (
                root,
                Script {
                    address: 12,
                    ops: vec![&[0x81, 1], &[255, 0]],
                    issue: None,
                },
            ),
            (
                Entry(1, 1),
                Script {
                    address: 20,
                    ops: vec![&[5, 2, 1, 0], &[255, 1]],
                    issue: None,
                },
            ),
        ]),
    };
    let data = emc::convert(&graph).unwrap();
    assert_eq!(gu_script(&data, 0, 0), &[0x14, 1, 255]);
    assert_eq!(gu_script(&data, 1, 1), &[0, 2, 1, 255]);
    assert_eq!(gu_script(&data, 1, 0), &[255]);
    graph.scripts.get_mut(&Entry(1, 1)).unwrap().ops[0] = &[5, 2, 1, 1];
    assert!(
        emc::convert(&graph)
            .unwrap_err()
            .to_string()
            .contains("FZ 1:1 at 0x00000014")
    );
}

#[test]
#[ignore = "set MHFZ_VITA_ELF to the source Vita ELF"]
fn espinas_ai_is_discovered_without_a_manifest() -> Result<()> {
    let elf = std::fs::read(std::env::var("MHFZ_VITA_ELF")?)?;
    let graph = ai::scan(&elf, 80, 0, Entry(0, 0))?;
    assert_eq!(graph.scripts[&Entry(0, 3)].address, 0x826ba454);
    assert_eq!(graph.scripts[&Entry(1, 1)].address, 0x826bab54);
    assert!(graph.scripts.contains_key(&Entry(22, 1)));
    assert!(graph.scripts.values().all(|s| s.issue.is_none()));
    let graph = ai::scan(&elf, 80, 0, Entry(0, 3))?;
    let (data, _) = emc::sample(&graph)?;
    assert_eq!(gu_script(&data, 0, 0), &[0x14, 1, 255]);
    assert_eq!(gu_script(&data, 1, 1), &[0, 2, 1, 255]);
    assert!(
        emc::convert(&graph)
            .unwrap_err()
            .to_string()
            .contains("0x826ba454")
    );
    Ok(())
}
/// Golden values from the original standard-detail Frontier Espinas PAC (em080).
#[test]
#[ignore = "set MHFZ_EM080_PAC to your source game asset"]
fn espinas_geometry_and_motion() -> Result<()> {
    let monster = Monster {
        pac: std::env::var("MHFZ_EM080_PAC")?.into(),
        model_member: 0,
    };
    let (pac, pair, rig) = monster.open()?;
    let m = model::convert(&pac::member(&pair, 0)?, &rig)?;
    let u16_at = |b: &[u8], p| u16::from_le_bytes(b[p..p + 2].try_into().unwrap());
    let f_at = |b: &[u8], p| f32::from_le_bytes(b[p..p + 4].try_into().unwrap());
    assert_eq!(&m[..6], b"MOD\0\xd4\0");
    assert_eq!(
        (u16_at(&m, 6), u16_at(&m, 8), word_at(&m, 12)),
        (48, 22, 7705)
    );
    let vb = word_at(&m, 52) as usize;
    let side = (0..3)
        .map(|i| f_at(&m, 96 + 4 * i) - f_at(&m, 80 + 4 * i))
        .fold(0f32, f32::max);
    for (i, pos) in [16.621965, 100.31453, 150.64856].into_iter().enumerate() {
        let decoded = f_at(&m, 80 + 4 * i) + f32::from(u16_at(&m, vb + 2 * i)) * side / 32767.;
        assert!((decoded - pos).abs() <= side / 65534.);
    }
    assert_eq!(&m[vb + 16..vb + 19], &[3, 2, 29]);
    assert!((f32::from(u16_at(&m, vb + 6)) / 32767. - 0.77).abs() < 0.001);

    let l = motion::convert(&pac::member(&pac, 2)?, &rig, 0, Some(2), None, None)?;
    assert_eq!(&l[..6], b"LMT\0C\0");
    assert_eq!(u16_at(&l, 6), 100);
    let a = word_at(&l, 16) as usize;
    assert_eq!((word_at(&l, 8), word_at(&l, 12)), (0, 0));
    assert_eq!(word_at(&l, a + 8), 59);
    let table = word_at(&l, a) as usize;
    let count = word_at(&l, a + 4) as usize;
    let t = (table..table + 36 * count)
        .step_by(36)
        .find(|&t| l[t..t + 4] == [4, 1, 0, 2])
        .expect("bone 2 translation");
    let buf = word_at(&l, t + 12) as usize;
    let bounds = word_at(&l, t + 32) as usize;
    let y = f_at(&l, bounds + 20)
        + f_at(&l, bounds + 4) * f32::from(u16_at(&l, buf + 5 * 8 + 2)) / 65535.;
    // FZ keys (4594 at frame 0; 4758 at frame 10), outgoing/incoming slopes 11/13.
    assert!((y - 292.09375).abs() < 0.001);
    Ok(())
}

#[test]
#[ignore = "requires the ordinary em080 PAC, Vita ELF and native GU archives"]
fn hunt_routes_match_vita_selector() -> Result<()> {
    use std::path::PathBuf;
    let pac = PathBuf::from(std::env::var("MHFZ_EM080_PAC")?);
    let elf = PathBuf::from(std::env::var("MHFZ_VITA_ELF")?);
    let enemy = PathBuf::from(std::env::var("MHGU_ENEMY_ARCS")?);
    let raw = super::charge::hunt(
        &pac,
        &enemy.join("em001_00.arc"),
        &enemy.join("em030_00.arc"),
        &elf,
    )?;
    let entries = mhgu_resource::arc::entries(&raw)?;
    let hde = entries
        .iter()
        .find(|e| e.name.ends_with("_attackdata") && e.kind == 0x70bb64ba)
        .expect("missing hunt attack table")
        .unpack()?;
    let foot = hde.len() - 32;
    assert_eq!(&hde[foot..foot + 4], b"ESH5");
    let at = |k| 16 + 32 * word_at(&hde, foot + k) as usize;
    let header = at(20);
    let routes = at(24);
    let view = espinas_behavior::BattleView {
        thresholds: std::array::from_fn(|i| f32::from_bits(word_at(&hde, header + i * 4))),
        decisions: &hde[header + 32..routes],
        routes: &hde[routes..foot],
    };
    let src = std::fs::read(elf)?;
    let combat = mhf_resource::combat::Combat::espinas(&src)?;
    for enraged in [false, true] {
        for airborne in [false, true] {
            for distance in [
                None,
                Some(0.),
                Some(451.),
                Some(1001.),
                Some(2701.),
                Some(5001.),
            ] {
                for angle in [3640, 3641] {
                    for random in 0..32 {
                        let target = mhf_resource::combat::Target {
                            enraged,
                            airborne,
                            distance,
                            angle,
                            random,
                        };
                        let mut q = espinas_behavior::Pending::default();
                        q.select(&view, enraged, airborne, distance, angle, random);
                        let calls: Vec<_> = std::iter::from_fn(|| q.next(view.routes))
                            .map(|r| (r.0, r.1))
                            .collect();
                        assert_eq!(
                            calls,
                            combat.requests(&target)?,
                            "route mismatch: enraged={enraged}, airborne={airborne}, distance={distance:?}, angle={angle}, random={random}"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
