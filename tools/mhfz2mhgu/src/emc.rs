use anyhow::{Context, Result, bail, ensure};
use mhf_resource::ai::{Entry, Graph};
use mhgu_resource::emc::{self, Scripts};
use std::collections::BTreeMap;

pub fn convert(graph: &Graph<'_>) -> Result<Vec<u8>> {
    let mut scripts = Scripts::new();
    for (&key, s) in &graph.scripts {
        if let Some(issue) = &s.issue {
            bail!("FZ {key}: {issue}");
        }
        ensure!(
            key.0 <= 1,
            "no verified GU group mapping for FZ group {}",
            key.0
        );
        let mut code = vec![];
        let mut va = s.address;
        for &op in &s.ops {
            match op {
                [0x05, g, id, 0] => code.extend(emc::action(*g, *id)),
                [0x81, i] if key.0 == 0 => code.extend(emc::call_action(*i)),
                [0xff, end] if u16::from(*end) == key.0 => code.push(emc::END),
                _ => bail!("FZ {key} at {va:#010x}: no complete GU mapping for {op:02x?}"),
            }
            va += op.len() as u32;
        }
        scripts.insert((key.0, key.1), code);
    }
    emc::encode(&scripts)
}

// A deliberately isolated action request, useful before the rest of an AI is translated.
pub fn sample(graph: &Graph<'_>) -> Result<(Vec<u8>, String)> {
    let root = &graph.scripts[&graph.root];
    let action = |k: Entry| -> Option<(u8, u8)> {
        graph.scripts.get(&k)?.ops.iter().find_map(|op| match **op {
            [0x05, g, id, 0] => Some((g, id)),
            _ => None,
        })
    };
    let mut scripts = Scripts::new();
    let called_action = root.ops.iter().find_map(|op| {
        let [0x81, slot] = **op else { return None };
        action(Entry(1, slot)).map(|request| (slot, request))
    });
    let (destination, (group, id), route) = match called_action {
        Some((slot, request)) => {
            let mut call = emc::call_action(slot).to_vec();
            call.push(emc::END);
            scripts.insert((0, 0), call);
            ((1, slot), request, format!("{} -> 1:{slot}", graph.root))
        }
        None => {
            let request = action(graph.root).with_context(|| {
                format!(
                    "FZ {} has no decoded action or call to a subscript with a decoded action",
                    graph.root
                )
            })?;
            ((0, 0), request, graph.root.to_string())
        }
    };
    let mut request = emc::action(group, id).to_vec();
    request.push(emc::END);
    scripts.insert(destination, request);
    Ok((
        emc::encode(&scripts)?,
        format!("FZ {route} -> action ({group}, {id}); GU entry 0:0"),
    ))
}

pub fn report(g: &Graph<'_>) {
    let mut groups = BTreeMap::<u16, Vec<String>>::new();
    for (&entry, s) in &g.scripts {
        let label = if s.issue.is_some() {
            format!("{}?", entry.1)
        } else {
            entry.1.to_string()
        };
        groups.entry(entry.0).or_default().push(label);
    }
    for (group, slots) in groups {
        println!("FZ group {group}: [{}]", slots.join(", "));
    }
    for (k, s) in &g.scripts {
        if let Some(issue) = &s.issue {
            println!("  {k}: {issue}");
        }
    }
}
