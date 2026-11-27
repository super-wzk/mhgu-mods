//! Consume the native Vita trace from verify_swing_frames.py.
use mhfz_sdk::action::{BranchRules, Cursor};
use std::{env, fs, path::Path};

#[test]
#[ignore = "run plugins/magnet-spike/tests/verify_swing_frames.py"]
fn w13_events() {
    static RULES: BranchRules = BranchRules {
        order: &[],
        combo_override: None,
        art_commands: 0,
    };
    let program = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/magnet-spike/target/assets/magnet_spike.fmsa"),
    )
    .unwrap()
    .leak();
    let trace = fs::read(env::var("MAGNET_EVENT_TRACE").unwrap()).unwrap();
    let word = |at: usize| u32::from_le_bytes(program[at..at + 4].try_into().unwrap()) as usize;
    let effects = program.windows(4).position(|s| s == b"MSFX").unwrap() + 8;
    let mut contacts = std::collections::BTreeMap::new();
    let mut replay: Option<Cursor> = None;
    let mut seen = 0;
    for record in trace.chunks_exact(12) {
        let [state, step, cycles, frame, blend, expected] = std::array::from_fn(|i| {
            u16::from_le_bytes(record[2 * i..2 * i + 2].try_into().unwrap())
        });
        if state == u16::MAX {
            replay = None;
            seen = 0;
            continue;
        }
        let desc = 32 + 24 * usize::from(state);
        let offset = word(desc + 4);
        let events = &program[offset..offset + 12 * word(desc)];
        let cursor = replay.get_or_insert_with(|| Cursor::new(state as u8, &[], events, &RULES));
        cursor.time.step = step as u8;
        cursor.time.cycles = cycles;
        cursor.time.frame = f32::from(frame);
        cursor.time.blend = f32::from(blend);
        while let Some((kind, value, _)) = cursor.next_motion_event() {
            let row = &events[12 * usize::from(seen)..];
            assert_eq!(
                (kind, value),
                (
                    u16::from_le_bytes(row[8..10].try_into().unwrap()),
                    u16::from_le_bytes(row[10..12].try_into().unwrap())
                )
            );
            let contact = match kind {
                3 => Some(word(12) + 40 * usize::from(value)),
                43 | 44 => Some(effects + 40 * 8),
                _ => None,
            };
            if let Some(at) = contact {
                let raw = |i| u16::from_le_bytes([program[at + i], program[at + i + 1]]);
                let (delay, duration) = mhfz_sdk::combat::contact_window(raw(0), raw(2));
                contacts.insert(
                    (state, seen),
                    [state, seen, raw(0), raw(2), delay as u16, duration as u16],
                );
            }
            seen += 1;
        }
        assert_eq!(
            seen, expected,
            "state {state}, step {step}, cycle {cycles}, frame {frame}, blend {blend}"
        );
    }
    let windows: Vec<u8> = contacts
        .into_values()
        .flatten()
        .flat_map(u16::to_le_bytes)
        .collect();
    fs::write(env::var("MAGNET_CONTACT_TRACE").unwrap(), windows).unwrap();
}
