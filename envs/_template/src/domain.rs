//! The domain: names, kinds, colors, XP rules, achievements. GAE: edit this file first.
//!
//! Mapping worksheet (answer these, then fill the spec):
//! - What is a unit? (a host, a repo, a target, a service) and what groups them? (subnet, org, scope)
//! - What is an action? (a request, a build, a scan, a test run)
//! - What is a signal, i.e. what needs the human? (an alert, a red build, a finding to verify)
//! - What does resolving one mean, and what is noise (dismiss)?

use gae_engine::core::Spec;
use gae_engine::core::game::{Achievement, default_achievements};
use gae_engine::core::spec::{KindStyle, Nouns, Shape};

pub fn spec() -> Spec {
    let mut s = Spec::new("Template Desk", "gae-env-template");
    s.tagline = "replace me";
    s.port = 4790;
    s.nouns = Nouns {
        core: "base",
        unit: "unit",
        units: "units",
        signal: "signal",
        signals: "signals",
        action: "action",
        actions: "actions",
        resolve: "resolve",
        dismiss: "dismiss",
    };
    s.kinds = vec![
        KindStyle::new("group", "groups", [0.35, 0.55, 1.0], Shape::Cylinder),
        KindStyle::new("worker", "workers", [0.45, 1.0, 0.75], Shape::Capsule),
    ];
    s.achievements = default_achievements();
    s.achievements.push(Achievement {
        id: "template-ten",
        title: "Ten Down",
        detail: "Resolve ten signals.",
        test: |g| g.resolved >= 10,
    });
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn spec_is_valid() {
        assert_eq!(super::spec().validate(), Vec::<String>::new());
    }
}
