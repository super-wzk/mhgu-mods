use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use core::fmt::Write as _;
use mhgu_sdk::monster::species::{HuntCounts, SpeciesId, valid_key};

const MAX_COUNT: u16 = 9999;
pub(crate) type Totals = BTreeMap<String, HuntCounts>;

struct Definition {
    key: String,
    template: SpeciesId,
}

pub(crate) struct Registry {
    species: BTreeMap<SpeciesId, Definition>,
    quest: BTreeMap<SpeciesId, HuntCounts>,
    settled: bool,
    character: Option<Totals>,
    dirty: bool,
}

impl Registry {
    pub const fn new() -> Self {
        Self {
            species: BTreeMap::new(),
            quest: BTreeMap::new(),
            settled: false,
            character: None,
            dirty: false,
        }
    }
    // The service has already validated the packed ID and key at its ABI boundary.
    pub fn register(&mut self, id: SpeciesId, key: &str, template: SpeciesId) -> bool {
        if let Some(existing) = self.species.get(&id) {
            return existing.key == key && existing.template == template;
        }
        if self.species.values().any(|s| s.key == key) {
            return false;
        }
        self.species.insert(
            id,
            Definition {
                key: key.to_string(),
                template,
            },
        );
        true
    }
    pub fn template(&self, id: SpeciesId) -> Option<SpeciesId> {
        self.species.get(&id).map(|entry| entry.template)
    }
    pub fn contains(&self, id: SpeciesId) -> bool {
        self.species.contains_key(&id)
    }
    pub fn counts(&self, id: SpeciesId) -> Option<HuntCounts> {
        let key = &self.species.get(&id)?.key;
        let totals = self.character.as_ref()?;
        Some(totals.get(key).copied().unwrap_or_default())
    }
    pub fn begin_quest(&mut self) {
        self.quest.clear();
        self.settled = false;
    }
    pub fn has_hunted(&self, id: SpeciesId) -> bool {
        self.quest.contains_key(&id)
    }
    /// A registered species still participates in objective handling after settlement,
    /// but its totals stop changing. An unknown species must never reach either path.
    pub fn hunt(&mut self, id: SpeciesId, captured: bool) -> bool {
        if !self.contains(id) {
            return false;
        }
        if !self.settled {
            let counts = self.quest.entry(id).or_default();
            let bucket = if captured {
                &mut counts.captured
            } else {
                &mut counts.slain
            };
            *bucket = (*bucket + 1).min(MAX_COUNT);
        }
        true
    }
    pub fn settle(&mut self) {
        if self.settled {
            return;
        }
        self.settled = true;
        if let Some(totals) = self.character.as_mut() {
            for (id, delta) in &self.quest {
                let key = &self.species[id].key;
                let counts = totals.entry(key.clone()).or_default();
                counts.slain = (counts.slain + delta.slain).min(MAX_COUNT);
                counts.captured = (counts.captured + delta.captured).min(MAX_COUNT);
                self.dirty = true;
            }
        }
        // Keep the quest query valid through the results screen, but never transfer twice.
    }
    pub fn load(&mut self, totals: Option<Totals>) {
        self.character = totals;
        self.dirty = false;
        self.begin_quest();
    }
    pub fn snapshot(&self, generation: u32) -> Option<String> {
        if !self.dirty {
            return None;
        }
        Some(encode(self.character.as_ref()?, generation))
    }
    pub fn saved(&mut self) {
        self.dirty = false;
    }
}

const COLUMNS: &str = "species\tslain\tcaptured";
const END: &str = "# end";

fn encode(totals: &Totals, generation: u32) -> String {
    let mut out = alloc::format!("generation\t{generation}\n{COLUMNS}\n");
    for (key, c) in totals {
        writeln!(out, "{key}\t{}\t{}", c.slain, c.captured).unwrap();
    }
    // A short write must not look like a complete file, even at a row boundary.
    out.push_str(END);
    out.push('\n');
    out
}

pub(crate) fn decode(bytes: &[u8]) -> Option<(u32, Totals)> {
    let mut lines = core::str::from_utf8(bytes).ok()?.lines();
    let generation = lines.next()?.strip_prefix("generation\t")?.parse().ok()?;
    if generation == 0 {
        return None;
    }
    if lines.next()? != COLUMNS || lines.next_back()? != END {
        return None;
    }
    let mut totals = Totals::new();
    for row in lines {
        let mut fields = row.split('\t');
        let key = fields.next()?;
        let c = HuntCounts {
            slain: fields.next()?.parse().ok()?,
            captured: fields.next()?.parse().ok()?,
        };
        if fields.next().is_some()
            || !valid_key(key)
            || c.slain > MAX_COUNT
            || c.captured > MAX_COUNT
            || totals.insert(key.to_string(), c).is_some()
        {
            return None;
        }
    }
    Some((generation, totals))
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEMPLATE: SpeciesId = SpeciesId::new(1).unwrap();
    const A: SpeciesId = SpeciesId::new(107).unwrap();
    const B: SpeciesId = SpeciesId::new(108).unwrap();
    #[test]
    fn quests_and_slots_keep_distinct_records_across_install_order() {
        let mut r = Registry::new();
        assert!(
            r.register(A, "frontier:espinas", TEMPLATE) && r.register(B, "other:wyvern", TEMPLATE)
        );
        assert!(r.register(A, "frontier:espinas", TEMPLATE));
        assert_eq!(r.template(A), Some(TEMPLATE));
        assert!(!r.register(A, "frontier:espinas", SpeciesId::new(2).unwrap()));
        assert!(
            !r.register(B, "frontier:espinas", TEMPLATE)
                && !r.register(A, "other:wyvern", TEMPLATE)
        );
        r.load(Some(Totals::new()));
        r.hunt(A, true);
        r.begin_quest(); // abandoned: no totals
        r.hunt(A, false);
        r.hunt(B, true);
        r.settle();
        r.settle();
        assert!(r.has_hunted(A));
        let file = r.snapshot(1).unwrap();
        r.saved();
        assert!(r.snapshot(2).is_none());
        assert!(r.contains(A) && !r.contains(SpeciesId::new(1).unwrap()));
        assert!(file.contains("frontier:espinas\t1\t0\n"));
        let mut loaded = Registry::new();
        loaded.register(B, "other:wyvern", TEMPLATE);
        loaded.register(A, "frontier:espinas", TEMPLATE);
        loaded.load(Some(decode(file.as_bytes()).unwrap().1));
        assert_eq!(
            loaded.counts(A),
            Some(HuntCounts {
                slain: 1,
                captured: 0
            })
        );
        assert_eq!(
            loaded.counts(B),
            Some(HuntCounts {
                slain: 0,
                captured: 1
            })
        );
        loaded.load(Some(Totals::new()));
        assert_eq!(loaded.counts(A), Some(HuntCounts::default()));
        assert!(!loaded.has_hunted(B));
    }
    #[test]
    fn partial_or_ambiguous_text_never_replaces_a_valid_generation() {
        let text = "generation\t2\nspecies\tslain\tcaptured\nx:a\t4\t6\n# end\n";
        assert_eq!(
            decode(text.replace('\n', "\r\n").as_bytes()).unwrap().1["x:a"].captured,
            6
        );
        for bad in [
            text.replace("# end", ""),
            text.replace("6\n", "6\nx:a\t1\t0\n"),
            text.replace("4\t6", "10000\t6"),
            text.replace("4\t6", "4\t6\t7"),
        ] {
            assert!(decode(bad.as_bytes()).is_none());
        }
    }
}
