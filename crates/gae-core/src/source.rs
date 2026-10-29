//! Where events come from. A `Source` runs on its own worker thread, polled at its own interval;
//! it may block (read a socket, call an API) without touching the frame.
//!
//! Ready-made sources: `JsonlSource` (tail a JSON-lines file through a mapper) and `Replay`
//! (a scripted timeline, for `--demo` modes and tests).

use crate::model::Event;
use crate::tail::FileTail;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub trait Source: Send + 'static {
    /// Shown in the status strip ("eve.json", "demo").
    fn name(&self) -> String;
    /// Events since the last poll. An `Err` is shown in the UI; the source is polled again later.
    fn poll(&mut self) -> Result<Vec<Event>, String>;
    fn interval(&self) -> Duration {
        Duration::from_millis(400)
    }
}

/// Tails a JSON-lines file and turns each line into events with `map`. Lines `map` cannot read
/// are counted and reported, never fatal.
pub struct JsonlSource<F>
where
    F: FnMut(&str) -> Vec<Event> + Send + 'static,
{
    tail: FileTail,
    map: F,
    bad_lines: u64,
    reported: u64,
}

impl<F> JsonlSource<F>
where
    F: FnMut(&str) -> Vec<Event> + Send + 'static,
{
    pub fn new(path: impl Into<PathBuf>, from_start: bool, map: F) -> JsonlSource<F> {
        JsonlSource {
            tail: FileTail::new(path, from_start),
            map,
            bad_lines: 0,
            reported: 0,
        }
    }

    /// Lines that produced nothing, so far.
    pub fn bad_lines(&self) -> u64 {
        self.bad_lines
    }
}

impl<F> Source for JsonlSource<F>
where
    F: FnMut(&str) -> Vec<Event> + Send + 'static,
{
    fn name(&self) -> String {
        self.tail
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "jsonl".into())
    }

    fn poll(&mut self) -> Result<Vec<Event>, String> {
        let lines = self
            .tail
            .read_lines()
            .map_err(|e| format!("{}: {e}", self.tail.path().display()))?;
        let mut out = Vec::new();
        for line in lines {
            let evs = (self.map)(&line);
            if evs.is_empty() {
                self.bad_lines += 1;
            }
            out.extend(evs);
        }
        if self.bad_lines > self.reported + 100 {
            self.reported = self.bad_lines;
            return Err(format!("{}: {} lines skipped so far", self.name(), self.bad_lines));
        }
        Ok(out)
    }
}

/// A scripted timeline: `(delay after the previous step, events)`. Event times are rewritten to
/// "now" as each step plays, so a replay always looks live. Loops when `looping` is set.
pub struct Replay {
    name: String,
    steps: Vec<(Duration, Vec<Event>)>,
    next: usize,
    due: Instant,
    looping: bool,
    /// Ids and unit names get this suffix on every loop after the first, so repeats are new items.
    round: u32,
}

impl Replay {
    pub fn new(name: &str, steps: Vec<(Duration, Vec<Event>)>, looping: bool) -> Replay {
        let first = steps.first().map(|s| s.0).unwrap_or_default();
        Replay {
            name: name.to_string(),
            steps,
            next: 0,
            due: Instant::now() + first,
            looping,
            round: 0,
        }
    }

    pub fn finished(&self) -> bool {
        !self.looping && self.next >= self.steps.len()
    }
}

fn restamp(ev: &mut Event, now: i64, round: u32) {
    let tag = |s: &mut String| {
        if round > 0 && !s.is_empty() {
            s.push_str(&format!("~{round}"));
        }
    };
    match ev {
        Event::UnitUp { at, .. }
        | Event::UnitDown { at, .. }
        | Event::ActionStart { at, .. }
        | Event::ActionEnd { at, .. }
        | Event::Artifact { at, .. }
        | Event::Metric { at, .. }
        | Event::Log { at, .. } => *at = now,
        Event::Signal { id, at, .. } | Event::Resolve { id, at, .. } => {
            *at = now;
            tag(id);
        }
    }
    if let Event::ActionStart { action, .. } | Event::ActionEnd { action, .. } = ev {
        tag(action);
    }
}

impl Source for Replay {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(100)
    }

    fn poll(&mut self) -> Result<Vec<Event>, String> {
        let mut out = Vec::new();
        let now = Instant::now();
        while now >= self.due && !self.steps.is_empty() {
            if self.next >= self.steps.len() {
                if !self.looping {
                    break;
                }
                self.next = 0;
                self.round += 1;
            }
            let stamp = crate::model::now_millis();
            for ev in &self.steps[self.next].1 {
                let mut ev = ev.clone();
                restamp(&mut ev, stamp, self.round);
                out.push(ev);
            }
            self.next += 1;
            let gap = self
                .steps
                .get(self.next)
                .or_else(|| self.looping.then(|| &self.steps[0]))
                .map(|s| s.0)
                .unwrap_or_default();
            self.due += gap.max(Duration::from_millis(1));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_plays_in_order_and_retags_on_loop() {
        let sig = Event::Signal {
            id: "s".into(),
            unit: "u".into(),
            severity: crate::model::Severity::Low,
            title: "t".into(),
            detail: String::new(),
            kind: String::new(),
            at: 0,
        };
        let mut r = Replay::new("demo", vec![(Duration::ZERO, vec![sig.clone()])], true);
        let a = r.poll().unwrap();
        assert!(matches!(&a[0], Event::Signal { id, at, .. } if id == "s" && *at > 0));
        std::thread::sleep(Duration::from_millis(5));
        let b = r.poll().unwrap();
        assert!(
            b.iter()
                .any(|e| matches!(e, Event::Signal { id, .. } if id.starts_with("s~")))
        );
        let mut once = Replay::new("x", vec![(Duration::ZERO, vec![sig])], false);
        assert_eq!(once.poll().unwrap().len(), 1);
        assert!(once.finished());
        assert!(once.poll().unwrap().is_empty());
    }

    #[test]
    fn jsonl_source_maps_and_counts_bad_lines() {
        let p = std::env::temp_dir().join(format!("gae-jsonl-{}", std::process::id()));
        std::fs::write(&p, "{\"type\":\"unit_down\",\"unit\":\"a\",\"at\":1}\nnot json\n").unwrap();
        let mut s = JsonlSource::new(&p, true, |l| serde_json::from_str::<Event>(l).into_iter().collect());
        let evs = s.poll().unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(s.bad_lines(), 1);
        let _ = std::fs::remove_file(&p);
    }
}
