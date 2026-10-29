//! Where this environment's events come from. GAE: replace `map_line` with your format's
//! mapper and `demo` with a script that shows every kind and severity in under a minute.

use gae_engine::core::model::{Event, Severity};
use gae_engine::core::source::{JsonlSource, Replay};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// One input line to events. Returning nothing counts the line as skipped (reported, not fatal).
/// The template reads GAE's own schema; a real environment parses its tool's format here.
pub fn map_line(line: &str) -> Vec<Event> {
    serde_json::from_str::<Event>(line).into_iter().collect()
}

pub fn file(path: PathBuf, from_start: bool) -> JsonlSource<fn(&str) -> Vec<Event>> {
    JsonlSource::new(path, from_start, map_line as fn(&str) -> Vec<Event>)
}

fn up(unit: &str, parent: &str, kind: &str) -> Event {
    Event::UnitUp {
        unit: unit.into(),
        parent: Some(parent.into()),
        kind: kind.into(),
        label: String::new(),
        meta: BTreeMap::new(),
        at: 0,
    }
}

fn signal(id: &str, unit: &str, severity: Severity, title: &str) -> Event {
    Event::Signal {
        id: id.into(),
        unit: unit.into(),
        severity,
        title: title.into(),
        detail: String::new(),
        kind: String::new(),
        at: 0,
    }
}

/// A looping script: groups and workers appear, work, and raise signals of every severity.
pub fn demo() -> Replay {
    let s = Duration::from_secs;
    let mut steps = vec![(s(0), vec![up("alpha", "core", "group"), up("beta", "core", "group")])];
    for i in 0..6 {
        let group = if i % 2 == 0 { "alpha" } else { "beta" };
        let w = format!("worker-{i}");
        steps.push((Duration::from_millis(600), vec![up(&w, group, "worker")]));
        steps.push((
            Duration::from_millis(400),
            vec![Event::ActionStart {
                unit: w.clone(),
                action: format!("job-{i}"),
                kind: "job".into(),
                summary: format!("job {i}"),
                at: 0,
            }],
        ));
    }
    for (i, sev) in Severity::ALL.iter().enumerate() {
        let w = format!("worker-{i}");
        steps.push((
            s(3),
            vec![
                Event::ActionEnd {
                    unit: w.clone(),
                    action: format!("job-{i}"),
                    ok: *sev < Severity::High,
                    output: String::new(),
                    at: 0,
                },
                signal(&format!("sig-{i}"), &w, *sev, &format!("{} thing on {w}", sev.label())),
            ],
        ));
    }
    steps.push((s(20), vec![]));
    Replay::new("demo", steps, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gae_engine::core::World;
    use gae_engine::core::source::Source;

    #[test]
    fn map_line_reads_the_schema_and_skips_noise() {
        assert_eq!(map_line(r#"{"type":"unit_down","unit":"a"}"#).len(), 1);
        assert!(map_line("garbage").is_empty());
    }

    #[test]
    fn demo_builds_a_world_with_signals() {
        let mut r = demo();
        let mut w = World::new("base");
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while std::time::Instant::now() < deadline {
            for e in r.poll().unwrap() {
                w.apply(e);
            }
        }
        assert!(w.unit("alpha").is_some());
    }
}
