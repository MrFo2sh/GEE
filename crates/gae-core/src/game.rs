//! The game layer: XP, levels, streaks and achievements, as pure rules over `Change`s.
//!
//! The loop every environment shares: things need you (signals), you handle them, you earn XP.
//! Handling fast pays double; letting one go stale breaks the streak. A domain tunes the numbers
//! with `XpRules` and adds its own achievements; the engine shows the rewards and plays the cues.

use crate::model::{Change, How, Severity};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct XpRules {
    /// XP for resolving a signal, by severity (info, low, medium, high, critical).
    pub resolve: [u64; 5],
    /// XP for dismissing one as noise. Triage is work too, but less than a fix.
    pub dismiss: u64,
    /// Handled within this many millis of arriving: XP times `fast_multiplier`.
    pub fast_ms: i64,
    pub fast_multiplier: u64,
    /// Open longer than this: stale, and the streak breaks.
    pub stale_ms: i64,
    /// XP when the source itself clears a signal (the fix landed on its own), per severity.
    pub source_resolve: [u64; 5],
    /// XP for every action that ends well, and for every new unit.
    pub action_ok: u64,
    pub unit_added: u64,
}

impl Default for XpRules {
    fn default() -> Self {
        XpRules {
            resolve: [2, 5, 10, 25, 50],
            dismiss: 2,
            fast_ms: 60_000,
            fast_multiplier: 2,
            stale_ms: 15 * 60_000,
            source_resolve: [0, 1, 2, 5, 10],
            action_ok: 0,
            unit_added: 0,
        }
    }
}

/// Everything the game remembers. Persisted by the engine as `game.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Game {
    pub xp: u64,
    pub streak: u32,
    pub best_streak: u32,
    pub resolved: u64,
    pub dismissed: u64,
    pub fast: u64,
    pub stale: u64,
    /// Resolved, by severity.
    pub by_severity: [u64; 5],
    pub actions_ok: u64,
    pub units_seen: u64,
    /// The queue went from five or more open to zero.
    pub inbox_zero: u64,
    pub peak_open: u64,
    /// Earned achievement ids with the time they were earned.
    pub badges: Vec<(String, i64)>,
}

/// What the player sees and hears.
#[derive(Clone, Debug, PartialEq)]
pub enum Reward {
    Xp { amount: u64, reason: String },
    LevelUp { level: u32 },
    Badge { id: String, title: String },
    StreakBroken { was: u32 },
}

/// A goal with a test over the game state. Domains add their own next to `default_achievements`.
#[derive(Clone)]
pub struct Achievement {
    pub id: &'static str,
    pub title: &'static str,
    pub detail: &'static str,
    pub test: fn(&Game) -> bool,
}

impl std::fmt::Debug for Achievement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Achievement").field("id", &self.id).finish()
    }
}

pub fn default_achievements() -> Vec<Achievement> {
    vec![
        Achievement {
            id: "first-blood",
            title: "First Blood",
            detail: "Resolve your first signal.",
            test: |g| g.resolved >= 1,
        },
        Achievement {
            id: "quick-draw",
            title: "Quick Draw",
            detail: "Resolve 10 signals inside the fast window.",
            test: |g| g.fast >= 10,
        },
        Achievement {
            id: "streak-5",
            title: "On a Roll",
            detail: "A streak of 5.",
            test: |g| g.best_streak >= 5,
        },
        Achievement {
            id: "streak-25",
            title: "Untouchable",
            detail: "A streak of 25.",
            test: |g| g.best_streak >= 25,
        },
        Achievement {
            id: "critical-hit",
            title: "Critical Hit",
            detail: "Resolve a critical signal.",
            test: |g| g.by_severity[Severity::Critical.index()] >= 1,
        },
        Achievement {
            id: "inbox-zero",
            title: "Inbox Zero",
            detail: "Clear a queue of five or more.",
            test: |g| g.inbox_zero >= 1,
        },
        Achievement {
            id: "centurion",
            title: "Centurion",
            detail: "Resolve 100 signals.",
            test: |g| g.resolved >= 100,
        },
        Achievement {
            id: "noise-filter",
            title: "Noise Filter",
            detail: "Dismiss 25 false positives.",
            test: |g| g.dismissed >= 25,
        },
    ]
}

/// XP needed to reach `level` (level 1 needs 0, 2 needs 100, 3 needs 300, 4 needs 600...).
pub fn threshold(level: u32) -> u64 {
    let l = level.max(1) as u64;
    50 * l * (l - 1)
}

impl Game {
    pub fn level(&self) -> u32 {
        let mut l = 1;
        while threshold(l + 1) <= self.xp {
            l += 1;
        }
        l
    }

    /// (XP into this level, XP this level spans).
    pub fn progress(&self) -> (u64, u64) {
        let l = self.level();
        let lo = threshold(l);
        (self.xp - lo, threshold(l + 1) - lo)
    }

    pub fn has(&self, id: &str) -> bool {
        self.badges.iter().any(|(b, _)| b == id)
    }

    fn gain(&mut self, amount: u64, reason: String, out: &mut Vec<Reward>) {
        if amount == 0 {
            return;
        }
        let before = self.level();
        self.xp += amount;
        out.push(Reward::Xp { amount, reason });
        let after = self.level();
        for level in before + 1..=after {
            out.push(Reward::LevelUp { level });
        }
    }

    /// Score one change. `open_now` is the queue length after the change.
    pub fn score(
        &mut self,
        change: &Change,
        open_now: usize,
        rules: &XpRules,
        achievements: &[Achievement],
        now: i64,
    ) -> Vec<Reward> {
        let mut out = Vec::new();
        let open_before = self.peak_open;
        self.peak_open = self.peak_open.max(open_now as u64);
        match change {
            Change::UnitAdded { .. } => {
                self.units_seen += 1;
                self.gain(rules.unit_added, "new unit".into(), &mut out);
            }
            Change::ActionOk { .. } => {
                self.actions_ok += 1;
                self.gain(rules.action_ok, "action done".into(), &mut out);
            }
            Change::SignalClosed {
                severity, how, age_ms, ..
            } => {
                let fast = *age_ms <= rules.fast_ms;
                match how {
                    How::Resolved | How::Dismissed => {
                        let base = if *how == How::Resolved {
                            self.resolved += 1;
                            self.by_severity[severity.index()] += 1;
                            rules.resolve[severity.index()]
                        } else {
                            self.dismissed += 1;
                            rules.dismiss
                        };
                        let amount = if fast {
                            self.fast += 1;
                            base * rules.fast_multiplier
                        } else {
                            base
                        };
                        self.streak += 1;
                        self.best_streak = self.best_streak.max(self.streak);
                        let verb = if *how == How::Resolved { "resolved" } else { "dismissed" };
                        let reason = if fast {
                            format!("{} {verb}, fast", severity.label())
                        } else {
                            format!("{} {verb}", severity.label())
                        };
                        self.gain(amount, reason, &mut out);
                    }
                    How::Source => {
                        self.gain(
                            rules.source_resolve[severity.index()],
                            format!("{} cleared at the source", severity.label()),
                            &mut out,
                        );
                    }
                    How::Overflow => {}
                }
                if open_now == 0 && open_before >= 5 {
                    self.inbox_zero += 1;
                }
                if open_now == 0 {
                    self.peak_open = 0;
                }
            }
            Change::SignalStale { .. } => {
                self.stale += 1;
                if self.streak > 0 {
                    out.push(Reward::StreakBroken { was: self.streak });
                }
                self.streak = 0;
            }
            _ => {}
        }
        for a in achievements {
            if !self.has(a.id) && (a.test)(self) {
                self.badges.push((a.id.to_string(), now));
                out.push(Reward::Badge {
                    id: a.id.to_string(),
                    title: a.title.to_string(),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(severity: Severity, how: How, age_ms: i64) -> Change {
        Change::SignalClosed {
            id: "s".into(),
            severity,
            how,
            age_ms,
        }
    }

    #[test]
    fn levels_follow_thresholds() {
        let mut g = Game::default();
        assert_eq!(g.level(), 1);
        g.xp = 99;
        assert_eq!(g.level(), 1);
        g.xp = 100;
        assert_eq!(g.level(), 2);
        assert_eq!(g.progress(), (0, 200));
        g.xp = 650;
        assert_eq!(g.level(), 4);
    }

    #[test]
    fn fast_resolve_pays_double_and_builds_streak() {
        let rules = XpRules::default();
        let mut g = Game::default();
        let r = g.score(&closed(Severity::High, How::Resolved, 1_000), 0, &rules, &[], 0);
        assert_eq!(
            r[0],
            Reward::Xp {
                amount: 50,
                reason: "high resolved, fast".into()
            }
        );
        g.score(&closed(Severity::High, How::Resolved, 600_000), 0, &rules, &[], 0);
        assert_eq!(g.xp, 75);
        assert_eq!(g.streak, 2);
        assert_eq!(g.fast, 1);
    }

    #[test]
    fn stale_breaks_the_streak_and_level_ups_are_announced() {
        let rules = XpRules::default();
        let mut g = Game::default();
        for _ in 0..3 {
            g.score(&closed(Severity::Critical, How::Resolved, 0), 0, &rules, &[], 0);
        }
        assert!(g.level() >= 2);
        let r = g.score(
            &Change::SignalStale {
                id: "x".into(),
                severity: Severity::Low,
            },
            1,
            &rules,
            &[],
            0,
        );
        assert_eq!(r, vec![Reward::StreakBroken { was: 3 }]);
        assert_eq!(g.best_streak, 3);
        let mut g = Game {
            xp: 95,
            ..Game::default()
        };
        let r = g.score(&closed(Severity::Medium, How::Resolved, 0), 0, &rules, &[], 0);
        assert!(r.contains(&Reward::LevelUp { level: 2 }));
    }

    #[test]
    fn achievements_fire_once_including_inbox_zero() {
        let rules = XpRules::default();
        let all = default_achievements();
        let mut g = Game {
            peak_open: 6,
            ..Game::default()
        };
        let r = g.score(&closed(Severity::Critical, How::Resolved, 0), 0, &rules, &all, 42);
        let ids: Vec<_> = r
            .iter()
            .filter_map(|x| match x {
                Reward::Badge { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert!(ids.contains(&"first-blood"));
        assert!(ids.contains(&"critical-hit"));
        assert!(ids.contains(&"inbox-zero"));
        let again = g.score(&closed(Severity::Critical, How::Resolved, 0), 0, &rules, &all, 43);
        assert!(!again.iter().any(|x| matches!(x, Reward::Badge { .. })));
    }

    #[test]
    fn overflow_and_source_closures_do_not_touch_the_streak() {
        let rules = XpRules::default();
        let mut g = Game::default();
        g.score(&closed(Severity::Low, How::Overflow, 0), 3, &rules, &[], 0);
        g.score(&closed(Severity::High, How::Source, 0), 2, &rules, &[], 0);
        assert_eq!(g.streak, 0);
        assert_eq!(g.resolved, 0);
        assert_eq!(g.xp, 5);
    }
}
