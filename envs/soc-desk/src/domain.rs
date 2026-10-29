//! SOC Desk's domain. Worksheet answers:
//! - unit: an internal host, grouped by its /24 subnet; the gateway (.1) is drawn apart.
//! - action: a flow, DNS query, HTTP request or TLS handshake seen for that host.
//! - signal: a Suricata alert (or anomaly), keyed by signature and host so repeats aggregate.
//! - resolve: the alert was triaged and handled; dismiss: a false positive.

use gae_engine::core::Spec;
use gae_engine::core::game::{Achievement, XpRules, default_achievements};
use gae_engine::core::model::Severity;
use gae_engine::core::spec::{KindStyle, Nouns, Shape};

pub fn spec() -> Spec {
    let mut s = Spec::new("SOC Desk", "soc-desk");
    s.tagline = "suricata · triage is the game";
    s.port = 4791;
    s.nouns = Nouns {
        core: "SOC",
        unit: "host",
        units: "hosts",
        signal: "alert",
        signals: "alerts",
        action: "event",
        actions: "events",
        resolve: "handled",
        dismiss: "false positive",
    };
    s.kinds = vec![
        KindStyle::new("subnet", "subnets", [0.30, 0.55, 1.0], Shape::Cylinder).size(1.3),
        KindStyle::new("gateway", "gateways", [0.95, 0.80, 0.35], Shape::Diamond),
        KindStyle::new("host", "hosts", [0.40, 0.95, 0.80], Shape::Cube),
    ];
    s.xp = XpRules {
        stale_ms: 10 * 60_000,
        ..XpRules::default()
    };
    s.achievements = default_achievements();
    s.achievements.push(Achievement {
        id: "threat-hunter",
        title: "Threat Hunter",
        detail: "Handle ten high or critical alerts.",
        test: |g| g.by_severity[Severity::High.index()] + g.by_severity[Severity::Critical.index()] >= 10,
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
