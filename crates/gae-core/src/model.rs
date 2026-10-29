//! The event schema and the world it builds.
//!
//! Every source speaks `Event`. `World::apply` is the only place state changes; it returns the
//! `Change`s that matter to the game and the sounds, so those never re-derive what happened.
//! Every history is bounded: a world fed for a week uses the same memory as one fed for an hour.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};

/// The unit at the centre of every world. Units without a parent orbit it.
pub const CORE: &str = "core";
pub const MAX_UNITS: usize = 4_000;
pub const MAX_ACTIONS: usize = 400;
pub const MAX_ARTIFACTS: usize = 200;
pub const MAX_OPEN: usize = 2_000;
pub const MAX_CLOSED: usize = 500;
pub const MAX_LOG: usize = 500;
pub const MAX_TEXT: usize = 16 * 1024;

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn yes() -> bool {
    true
}

/// How much a signal needs the human. Ordered: `Critical > High > ... > Info`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub const ALL: [Severity; 5] = [
        Severity::Info,
        Severity::Low,
        Severity::Medium,
        Severity::High,
        Severity::Critical,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// One thing that happened. JSON: `{"type":"unit_up","unit":"web-01",...}`; `at` is epoch millis
/// and defaults to the time the event arrived.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A unit appeared or changed. An unknown parent is created on the fly under the core.
    UnitUp {
        unit: String,
        #[serde(default)]
        parent: Option<String>,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        label: String,
        #[serde(default)]
        meta: BTreeMap<String, String>,
        #[serde(default = "now_millis")]
        at: i64,
    },
    /// A unit finished or left. It stays on screen, dimmed, until evicted.
    UnitDown {
        unit: String,
        #[serde(default = "now_millis")]
        at: i64,
    },
    /// A unit started doing something (a scan, a build, a flow, a tool call).
    ActionStart {
        unit: String,
        action: String,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        summary: String,
        #[serde(default = "now_millis")]
        at: i64,
    },
    /// It ended. Without a matching start, it is recorded as an instant action.
    ActionEnd {
        unit: String,
        action: String,
        #[serde(default = "yes")]
        ok: bool,
        #[serde(default)]
        output: String,
        #[serde(default = "now_millis")]
        at: i64,
    },
    /// Something needs the human: it enters the Needs-You queue. Same `id` again while open
    /// counts as a repeat, not a new item.
    Signal {
        id: String,
        unit: String,
        #[serde(default)]
        severity: Severity,
        title: String,
        #[serde(default)]
        detail: String,
        #[serde(default)]
        kind: String,
        #[serde(default = "now_millis")]
        at: i64,
    },
    /// The source says a signal is over (the alert cleared, the build went green).
    Resolve {
        id: String,
        #[serde(default)]
        note: String,
        #[serde(default = "now_millis")]
        at: i64,
    },
    /// Something a unit produced or touched: a URL, a file, an IOC, a report.
    Artifact {
        unit: String,
        #[serde(default)]
        kind: String,
        target: String,
        #[serde(default)]
        label: String,
        #[serde(default = "now_millis")]
        at: i64,
    },
    /// A number about a unit (cpu, findings, coverage). The latest value wins.
    Metric {
        unit: String,
        name: String,
        value: f64,
        #[serde(default = "now_millis")]
        at: i64,
    },
    /// A line for the status strip and the unit's log.
    Log {
        #[serde(default)]
        unit: Option<String>,
        text: String,
        #[serde(default)]
        level: Severity,
        #[serde(default = "now_millis")]
        at: i64,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// Did something within the active window, or has an action open.
    Active,
    Idle,
    Done,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub started: i64,
    pub ended: Option<i64>,
    pub ok: Option<bool>,
    pub output: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Artifact {
    pub kind: String,
    pub target: String,
    pub label: String,
    pub at: i64,
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub id: String,
    pub parent: Option<String>,
    pub depth: u32,
    /// Arrival order, so siblings keep their places on the ring.
    pub order: u32,
    pub kind: String,
    pub label: String,
    pub meta: BTreeMap<String, String>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub ended: Option<i64>,
    /// Newest last, at most `MAX_ACTIONS`.
    pub actions: VecDeque<Action>,
    pub artifacts: VecDeque<Artifact>,
    pub metrics: BTreeMap<String, f64>,
    pub action_count: u64,
    pub fail_count: u64,
}

impl Unit {
    fn new(id: &str, parent: Option<String>, depth: u32, order: u32, at: i64) -> Unit {
        Unit {
            id: id.to_string(),
            parent,
            depth,
            order,
            kind: String::new(),
            label: String::new(),
            meta: BTreeMap::new(),
            first_seen: at,
            last_seen: at,
            ended: None,
            actions: VecDeque::new(),
            artifacts: VecDeque::new(),
            metrics: BTreeMap::new(),
            action_count: 0,
            fail_count: 0,
        }
    }

    pub fn title(&self) -> &str {
        if self.label.is_empty() { &self.id } else { &self.label }
    }

    pub fn has_open_action(&self) -> bool {
        self.actions.iter().rev().take(16).any(|a| a.ended.is_none())
    }

    pub fn last_failed(&self) -> bool {
        self.actions
            .iter()
            .rev()
            .find(|a| a.ended.is_some())
            .is_some_and(|a| a.ok == Some(false))
    }

    pub fn status(&self, now: i64, active_window_ms: i64) -> Status {
        if self.ended.is_some() {
            Status::Done
        } else if self.has_open_action() || now - self.last_seen <= active_window_ms {
            Status::Active
        } else {
            Status::Idle
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    /// The human handled it.
    Resolved,
    /// The human called it noise (a false positive, a flaky test).
    Dismissed,
    /// The source cleared it.
    Source,
    /// Pushed out by the queue bound.
    Overflow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Closure {
    pub how: How,
    pub at: i64,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Signal {
    pub id: String,
    pub unit: String,
    pub severity: Severity,
    pub title: String,
    pub detail: String,
    pub kind: String,
    pub at: i64,
    pub last_at: i64,
    /// How many times the source raised it while open.
    pub count: u32,
    /// Open past the stale limit once already (it broke the streak).
    pub stale: bool,
    pub closed: Option<Closure>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub unit: Option<String>,
    pub text: String,
    pub level: Severity,
    pub at: i64,
}

/// What an applied event meant. The game scores these and the radio says them.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    UnitAdded {
        unit: String,
        kind: String,
    },
    UnitEnded {
        unit: String,
    },
    ActionStarted {
        unit: String,
        kind: String,
    },
    ActionOk {
        unit: String,
    },
    ActionFailed {
        unit: String,
    },
    SignalRaised {
        id: String,
        severity: Severity,
    },
    SignalClosed {
        id: String,
        severity: Severity,
        how: How,
        age_ms: i64,
    },
    SignalStale {
        id: String,
        severity: Severity,
    },
}

#[derive(Debug)]
pub struct World {
    pub units: Vec<Unit>,
    index: HashMap<String, usize>,
    /// Open signals, in arrival order. `queue()` gives the triage order.
    pub open: Vec<Signal>,
    /// Closed signals, newest last, at most `MAX_CLOSED`.
    pub closed: VecDeque<Signal>,
    pub log: VecDeque<LogLine>,
    next_order: u32,
    /// Bumps whenever units are added, removed or re-parented: the layout is stale.
    pub version: u64,
}

fn clip(mut s: String) -> String {
    if s.len() > MAX_TEXT {
        let mut cut = MAX_TEXT;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

impl World {
    pub fn new(core_label: &str) -> World {
        let now = now_millis();
        let mut core = Unit::new(CORE, None, 0, 0, now);
        core.label = core_label.to_string();
        core.kind = CORE.to_string();
        let mut index = HashMap::new();
        index.insert(CORE.to_string(), 0);
        World {
            units: vec![core],
            index,
            open: Vec::new(),
            closed: VecDeque::new(),
            log: VecDeque::new(),
            next_order: 1,
            version: 1,
        }
    }

    pub fn unit(&self, id: &str) -> Option<&Unit> {
        self.index.get(id).map(|&i| &self.units[i])
    }

    fn unit_mut(&mut self, id: &str) -> Option<&mut Unit> {
        self.index.get(id).copied().map(move |i| &mut self.units[i])
    }

    pub fn children<'a>(&'a self, id: &'a str) -> impl Iterator<Item = &'a Unit> + 'a {
        self.units.iter().filter(move |u| u.parent.as_deref() == Some(id))
    }

    pub fn max_depth(&self) -> u32 {
        self.units.iter().map(|u| u.depth).max().unwrap_or(0)
    }

    /// Open signals in triage order: most severe first, then oldest first.
    pub fn queue(&self) -> Vec<&Signal> {
        let mut q: Vec<&Signal> = self.open.iter().collect();
        q.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.at.cmp(&b.at)));
        q
    }

    /// The worst open severity on a unit, if any.
    pub fn unit_alert(&self, unit: &str) -> Option<Severity> {
        self.open.iter().filter(|s| s.unit == unit).map(|s| s.severity).max()
    }

    /// Make sure `id` exists; returns its index. Unknown parents are created under the core.
    fn ensure(&mut self, id: &str, parent: Option<&str>, at: i64) -> usize {
        if let Some(&i) = self.index.get(id) {
            return i;
        }
        let parent = match parent {
            Some(p) if p != id && !p.is_empty() => p.to_string(),
            _ => CORE.to_string(),
        };
        let pi = if id == CORE { 0 } else { self.ensure(&parent, None, at) };
        let depth = self.units[pi].depth + 1;
        let order = self.next_order;
        self.next_order += 1;
        self.units.push(Unit::new(id, Some(parent), depth, order, at));
        let i = self.units.len() - 1;
        self.index.insert(id.to_string(), i);
        self.version += 1;
        self.evict();
        self.index.get(id).copied().unwrap_or(i)
    }

    /// Over the bound: drop the oldest ended leaf units (never the core, never one with an open signal).
    fn evict(&mut self) {
        if self.units.len() <= MAX_UNITS {
            return;
        }
        let excess = self.units.len() - MAX_UNITS;
        let mut candidates: Vec<(i64, String)> = self
            .units
            .iter()
            .filter(|u| u.id != CORE && u.ended.is_some())
            .filter(|u| !self.units.iter().any(|c| c.parent.as_deref() == Some(u.id.as_str())))
            .filter(|u| !self.open.iter().any(|s| s.unit == u.id))
            .map(|u| (u.ended.unwrap_or(0), u.id.clone()))
            .collect();
        candidates.sort();
        let gone: Vec<String> = candidates.into_iter().take(excess).map(|(_, id)| id).collect();
        if gone.is_empty() {
            return;
        }
        self.units.retain(|u| !gone.contains(&u.id));
        self.reindex();
    }

    fn reindex(&mut self) {
        self.index = self.units.iter().enumerate().map(|(i, u)| (u.id.clone(), i)).collect();
        self.version += 1;
    }

    /// Depths follow parents; run after a re-parent.
    fn recompute_depths(&mut self) {
        for _ in 0..64 {
            let mut changed = false;
            for i in 0..self.units.len() {
                let want = match self.units[i].parent.as_deref() {
                    None => 0,
                    Some(p) => self.index.get(p).map(|&pi| self.units[pi].depth + 1).unwrap_or(1),
                };
                if self.units[i].depth != want {
                    self.units[i].depth = want;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        self.version += 1;
    }

    fn touches_cycle(&self, id: &str, new_parent: &str) -> bool {
        let mut cur = Some(new_parent.to_string());
        let mut hops = 0;
        while let Some(c) = cur {
            if c == id {
                return true;
            }
            hops += 1;
            if hops > 256 {
                return true;
            }
            cur = self.unit(&c).and_then(|u| u.parent.clone());
        }
        false
    }

    pub fn apply(&mut self, ev: Event) -> Vec<Change> {
        let mut out = Vec::new();
        match ev {
            Event::UnitUp {
                unit,
                parent,
                kind,
                label,
                meta,
                at,
            } => {
                if unit.is_empty() {
                    return out;
                }
                let existed = self.index.contains_key(&unit);
                let i = self.ensure(&unit, parent.as_deref(), at);
                if existed
                    && let Some(p) = parent.filter(|p| !p.is_empty() && p != &unit)
                    && self.units[i].parent.as_deref() != Some(p.as_str())
                    && unit != CORE
                    && !self.touches_cycle(&unit, &p)
                {
                    self.ensure(&p, None, at);
                    if let Some(u) = self.unit_mut(&unit) {
                        u.parent = Some(p);
                    }
                    self.recompute_depths();
                }
                let Some(u) = self.unit_mut(&unit) else {
                    return out;
                };
                if !kind.is_empty() {
                    u.kind = kind;
                }
                if !label.is_empty() {
                    u.label = label;
                }
                u.meta.extend(meta);
                u.last_seen = u.last_seen.max(at);
                u.ended = None;
                if !existed {
                    out.push(Change::UnitAdded {
                        unit: u.id.clone(),
                        kind: u.kind.clone(),
                    });
                }
            }
            Event::UnitDown { unit, at } => {
                if let Some(u) = self.unit_mut(&unit)
                    && u.ended.is_none()
                    && unit != CORE
                {
                    u.ended = Some(at);
                    for a in u.actions.iter_mut().filter(|a| a.ended.is_none()) {
                        a.ended = Some(at);
                    }
                    out.push(Change::UnitEnded { unit });
                }
            }
            Event::ActionStart {
                unit,
                action,
                kind,
                summary,
                at,
            } => {
                let added = !self.index.contains_key(&unit);
                let i = self.ensure(&unit, None, at);
                let u = &mut self.units[i];
                if added {
                    out.push(Change::UnitAdded {
                        unit: u.id.clone(),
                        kind: u.kind.clone(),
                    });
                }
                u.last_seen = u.last_seen.max(at);
                u.action_count += 1;
                out.push(Change::ActionStarted {
                    unit: unit.clone(),
                    kind: kind.clone(),
                });
                u.actions.push_back(Action {
                    id: action,
                    kind,
                    summary: clip(summary),
                    started: at,
                    ended: None,
                    ok: None,
                    output: String::new(),
                });
                while u.actions.len() > MAX_ACTIONS {
                    u.actions.pop_front();
                }
            }
            Event::ActionEnd {
                unit,
                action,
                ok,
                output,
                at,
            } => {
                let i = self.ensure(&unit, None, at);
                let u = &mut self.units[i];
                u.last_seen = u.last_seen.max(at);
                match u.actions.iter_mut().rev().find(|a| a.id == action && a.ended.is_none()) {
                    Some(a) => {
                        a.ended = Some(at);
                        a.ok = Some(ok);
                        a.output = clip(output);
                    }
                    None => {
                        u.action_count += 1;
                        u.actions.push_back(Action {
                            id: action.clone(),
                            kind: String::new(),
                            summary: action,
                            started: at,
                            ended: Some(at),
                            ok: Some(ok),
                            output: clip(output),
                        });
                        while u.actions.len() > MAX_ACTIONS {
                            u.actions.pop_front();
                        }
                    }
                }
                if ok {
                    out.push(Change::ActionOk { unit });
                } else {
                    u.fail_count += 1;
                    out.push(Change::ActionFailed { unit });
                }
            }
            Event::Signal {
                id,
                unit,
                severity,
                title,
                detail,
                kind,
                at,
            } => {
                if id.is_empty() {
                    return out;
                }
                let i = self.ensure(&unit, None, at);
                self.units[i].last_seen = self.units[i].last_seen.max(at);
                if let Some(s) = self.open.iter_mut().find(|s| s.id == id) {
                    s.count += 1;
                    s.last_at = at;
                    if severity > s.severity {
                        s.severity = severity;
                    }
                    return out;
                }
                self.open.push(Signal {
                    id: id.clone(),
                    unit,
                    severity,
                    title: clip(title),
                    detail: clip(detail),
                    kind,
                    at,
                    last_at: at,
                    count: 1,
                    stale: false,
                    closed: None,
                });
                out.push(Change::SignalRaised { id, severity });
                if self.open.len() > MAX_OPEN {
                    // Drop the least severe, oldest item: the queue never grows without bound.
                    if let Some(pos) = self
                        .open
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| a.severity.cmp(&b.severity).then(a.at.cmp(&b.at)))
                        .map(|(i, _)| i)
                    {
                        let gone = self.open[pos].id.clone();
                        out.extend(self.close(&gone, How::Overflow, at, String::new()));
                    }
                }
            }
            Event::Resolve { id, note, at } => {
                out.extend(self.close(&id, How::Source, at, note));
            }
            Event::Artifact {
                unit,
                kind,
                target,
                label,
                at,
            } => {
                let i = self.ensure(&unit, None, at);
                let u = &mut self.units[i];
                u.last_seen = u.last_seen.max(at);
                if !u.artifacts.iter().any(|a| a.target == target && a.kind == kind) {
                    u.artifacts.push_back(Artifact {
                        kind,
                        target,
                        label,
                        at,
                    });
                    while u.artifacts.len() > MAX_ARTIFACTS {
                        u.artifacts.pop_front();
                    }
                }
            }
            Event::Metric { unit, name, value, at } => {
                let i = self.ensure(&unit, None, at);
                let u = &mut self.units[i];
                u.metrics.insert(name, value);
                u.last_seen = u.last_seen.max(at);
            }
            Event::Log { unit, text, level, at } => {
                self.log.push_back(LogLine {
                    unit,
                    text: clip(text),
                    level,
                    at,
                });
                while self.log.len() > MAX_LOG {
                    self.log.pop_front();
                }
            }
        }
        out
    }

    /// Close an open signal. The human calls this through the UI; sources through `Event::Resolve`.
    pub fn close(&mut self, id: &str, how: How, at: i64, note: String) -> Option<Change> {
        let pos = self.open.iter().position(|s| s.id == id)?;
        let mut s = self.open.remove(pos);
        let age_ms = (at - s.at).max(0);
        let severity = s.severity;
        s.closed = Some(Closure { how, at, note });
        self.closed.push_back(s);
        while self.closed.len() > MAX_CLOSED {
            self.closed.pop_front();
        }
        Some(Change::SignalClosed {
            id: id.to_string(),
            severity,
            how,
            age_ms,
        })
    }

    /// Signals that just went past `stale_ms` while open. Each one is reported once.
    pub fn mark_stale(&mut self, now: i64, stale_ms: i64) -> Vec<Change> {
        let mut out = Vec::new();
        for s in self.open.iter_mut().filter(|s| !s.stale && now - s.at > stale_ms) {
            s.stale = true;
            out.push(Change::SignalStale {
                id: s.id.clone(),
                severity: s.severity,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(unit: &str, parent: Option<&str>) -> Event {
        Event::UnitUp {
            unit: unit.into(),
            parent: parent.map(String::from),
            kind: "host".into(),
            label: String::new(),
            meta: BTreeMap::new(),
            at: 10,
        }
    }

    fn signal(id: &str, unit: &str, severity: Severity) -> Event {
        Event::Signal {
            id: id.into(),
            unit: unit.into(),
            severity,
            title: "t".into(),
            detail: String::new(),
            kind: String::new(),
            at: 100,
        }
    }

    #[test]
    fn units_nest_under_parents_and_unknown_parents_appear() {
        let mut w = World::new("SOC");
        let ch = w.apply(up("web-01", Some("dmz")));
        assert_eq!(w.unit("dmz").unwrap().depth, 1);
        assert_eq!(w.unit("web-01").unwrap().depth, 2);
        assert_eq!(w.unit("dmz").unwrap().parent.as_deref(), Some(CORE));
        assert!(matches!(&ch[0], Change::UnitAdded { unit, .. } if unit == "web-01"));
        assert!(
            w.apply(up("web-01", Some("dmz"))).is_empty(),
            "a repeat is not a new unit"
        );
    }

    #[test]
    fn reparent_moves_depth_and_refuses_cycles() {
        let mut w = World::new("c");
        w.apply(up("a", None));
        w.apply(up("b", Some("a")));
        w.apply(up("c2", Some("b")));
        assert_eq!(w.unit("c2").unwrap().depth, 3);
        w.apply(up("b", Some("x")));
        assert_eq!(w.unit("b").unwrap().depth, 2);
        assert_eq!(w.unit("c2").unwrap().depth, 3);
        // x is c2's grandparent now: hanging x under c2 would make a loop.
        w.apply(up("x", Some("c2")));
        assert_eq!(w.unit("x").unwrap().parent.as_deref(), Some(CORE), "cycle refused");
    }

    #[test]
    fn signals_queue_by_severity_repeat_and_close() {
        let mut w = World::new("c");
        assert_eq!(w.apply(signal("s1", "h1", Severity::Low)).len(), 1);
        w.apply(signal("s2", "h2", Severity::Critical));
        assert!(w.apply(signal("s1", "h1", Severity::High)).is_empty(), "repeat");
        let q = w.queue();
        assert_eq!(q[0].id, "s2", "severity first");
        assert_eq!(q[1].id, "s1");
        assert_eq!(q[1].severity, Severity::High, "a repeat can raise the severity");
        assert_eq!(q[1].count, 2);
        assert_eq!(w.unit_alert("h2"), Some(Severity::Critical));
        let c = w.close("s2", How::Resolved, 160, String::new()).unwrap();
        assert_eq!(
            c,
            Change::SignalClosed {
                id: "s2".into(),
                severity: Severity::Critical,
                how: How::Resolved,
                age_ms: 60
            }
        );
        assert!(w.close("s2", How::Resolved, 170, String::new()).is_none());
        assert_eq!(w.closed.len(), 1);
    }

    #[test]
    fn critical_beats_high_in_queue() {
        let mut w = World::new("c");
        w.apply(signal("a", "h", Severity::High));
        w.apply(signal("b", "h", Severity::Critical));
        assert_eq!(w.queue()[0].id, "b");
    }

    #[test]
    fn actions_pair_up_and_instant_ends_record() {
        let mut w = World::new("c");
        w.apply(Event::ActionStart {
            unit: "u".into(),
            action: "a1".into(),
            kind: "scan".into(),
            summary: "nmap".into(),
            at: 1,
        });
        assert!(w.unit("u").unwrap().has_open_action());
        let ch = w.apply(Event::ActionEnd {
            unit: "u".into(),
            action: "a1".into(),
            ok: false,
            output: "boom".into(),
            at: 5,
        });
        assert_eq!(ch, vec![Change::ActionFailed { unit: "u".into() }]);
        let u = w.unit("u").unwrap();
        assert!(!u.has_open_action());
        assert!(u.last_failed());
        assert_eq!(u.actions.len(), 1);
        w.apply(Event::ActionEnd {
            unit: "u".into(),
            action: "flash".into(),
            ok: true,
            output: String::new(),
            at: 6,
        });
        assert_eq!(w.unit("u").unwrap().actions.len(), 2);
        assert!(!w.unit("u").unwrap().last_failed());
    }

    #[test]
    fn histories_are_bounded() {
        let mut w = World::new("c");
        for i in 0..(MAX_ACTIONS + 50) {
            w.apply(Event::ActionEnd {
                unit: "u".into(),
                action: format!("a{i}"),
                ok: true,
                output: String::new(),
                at: i as i64,
            });
        }
        assert_eq!(w.unit("u").unwrap().actions.len(), MAX_ACTIONS);
        for i in 0..(MAX_OPEN + 10) {
            w.apply(signal(&format!("s{i}"), "u", Severity::Low));
        }
        assert_eq!(w.open.len(), MAX_OPEN);
        let big = "x".repeat(MAX_TEXT * 2);
        w.apply(Event::Log {
            unit: None,
            text: big,
            level: Severity::Info,
            at: 0,
        });
        assert!(w.log.back().unwrap().text.len() <= MAX_TEXT + 4);
    }

    #[test]
    fn ended_leaves_are_evicted_past_the_bound() {
        let mut w = World::new("c");
        for i in 0..MAX_UNITS {
            w.apply(up(&format!("u{i}"), None));
            if i < 20 {
                w.apply(Event::UnitDown {
                    unit: format!("u{i}"),
                    at: i as i64,
                });
            }
        }
        assert!(w.units.len() <= MAX_UNITS);
        assert!(w.unit("u0").is_none(), "oldest ended unit went first");
        assert!(w.unit(CORE).is_some());
        assert!(w.unit(&format!("u{}", MAX_UNITS - 1)).is_some());
    }

    #[test]
    fn stale_signals_are_reported_once() {
        let mut w = World::new("c");
        w.apply(signal("s", "u", Severity::Medium));
        assert!(w.mark_stale(150, 100).is_empty());
        assert_eq!(w.mark_stale(300, 100).len(), 1);
        assert!(w.mark_stale(400, 100).is_empty());
    }

    #[test]
    fn json_schema_round_trip_and_defaults() {
        let e: Event = serde_json::from_str(r#"{"type":"signal","id":"x","unit":"h","title":"boom"}"#).unwrap();
        match e {
            Event::Signal { severity, at, .. } => {
                assert_eq!(severity, Severity::Info);
                assert!(at > 0);
            }
            _ => panic!(),
        }
        let e: Event = serde_json::from_str(r#"{"type":"action_end","unit":"h","action":"a","at":3}"#).unwrap();
        assert!(matches!(e, Event::ActionEnd { ok: true, at: 3, .. }));
        let s = serde_json::to_string(&Event::UnitDown {
            unit: "h".into(),
            at: 1,
        })
        .unwrap();
        assert_eq!(s, r#"{"type":"unit_down","unit":"h","at":1}"#);
    }
}
