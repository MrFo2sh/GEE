//! Suricata eve.json (one JSON object per line) to GAE events.
//!
//! Internal addresses (RFC 1918, unique-local IPv6) become hosts under their /24 (or /64)
//! subnet; the outside side of a flow is recorded as an artifact on the internal host.
//! `alert` and `anomaly` records are signals; `flow`, `dns`, `http`, `tls`, `ssh`, `smb`,
//! `fileinfo` are actions. Anything else is skipped.

use gae_engine::core::model::{Event, Severity};
use gae_engine::core::source::JsonlSource;
use serde_json::Value;
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::PathBuf;

/// Categories that are always critical, whatever the rule's own severity says.
const CRITICAL_CATEGORIES: [&str; 4] = [
    "A Network Trojan was detected",
    "Malware Command and Control Activity Detected",
    "Successful Administrator Privilege Gain",
    "Successful User Privilege Gain",
];

pub fn source(path: PathBuf, from_start: bool) -> JsonlSource<fn(&str) -> Vec<Event>> {
    JsonlSource::new(path, from_start, map_line as fn(&str) -> Vec<Event>)
}

pub fn internal(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_unique_local() || v6.is_loopback() || v6.is_unicast_link_local(),
    }
}

/// "10.0.1.0/24" for 10.0.1.7; "fd00:1:2:3::/64" for an IPv6 host.
pub fn subnet_of(ip: &IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
    }
}

fn is_gateway(ip: &IpAddr) -> bool {
    matches!(ip, IpAddr::V4(v4) if v4.octets()[3] == 1)
}

/// Suricata severity 1 is the worst. Some categories are critical whatever the number says.
pub fn severity(level: i64, category: &str) -> Severity {
    if CRITICAL_CATEGORIES.iter().any(|c| category.eq_ignore_ascii_case(c)) {
        return Severity::Critical;
    }
    match level {
        1 => Severity::High,
        2 => Severity::Medium,
        3 => Severity::Low,
        _ => Severity::Info,
    }
}

fn s<'a>(v: &'a Value, path: &[&str]) -> &'a str {
    let mut cur = v;
    for p in path {
        cur = &cur[*p];
    }
    cur.as_str().unwrap_or("")
}

fn host_events(ip: &IpAddr, at: i64) -> Vec<Event> {
    let subnet = subnet_of(ip);
    let kind = if is_gateway(ip) { "gateway" } else { "host" };
    let mut meta = BTreeMap::new();
    meta.insert("subnet".to_string(), subnet.clone());
    vec![
        Event::UnitUp {
            unit: subnet.clone(),
            parent: None,
            kind: "subnet".into(),
            label: subnet.clone(),
            meta: BTreeMap::new(),
            at,
        },
        Event::UnitUp {
            unit: ip.to_string(),
            parent: Some(subnet),
            kind: kind.into(),
            label: String::new(),
            meta,
            at,
        },
    ]
}

pub fn map_line(line: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let event_type = s(&v, &["event_type"]);
    let src: Option<IpAddr> = s(&v, &["src_ip"]).parse().ok();
    let dst: Option<IpAddr> = s(&v, &["dest_ip"]).parse().ok();
    // The host we care about: the internal end; for internal-to-internal traffic, the target.
    let (host, other) = match (src, dst) {
        (Some(a), Some(b)) if internal(&b) => (b, Some(a)),
        (Some(a), b) if internal(&a) => (a, b),
        (None, Some(b)) if internal(&b) => (b, None),
        _ => return Vec::new(),
    };
    // Arrival time, not the record's timestamp: ages in the queue mean "since I saw it",
    // and parsing every zone format Suricata can write is not worth a dependency.
    let at = gae_engine::core::now_millis();
    let unit = host.to_string();
    let mut out = host_events(&host, at);
    if let Some(o) = other
        && internal(&o)
        && o != host
    {
        out.extend(host_events(&o, at));
    }
    let peer = other.map(|o| o.to_string()).unwrap_or_default();
    let flow = v["flow_id"].as_u64().unwrap_or(0);
    let proto = s(&v, &["proto"]).to_ascii_lowercase();
    let dport = v["dest_port"].as_u64().unwrap_or(0);

    match event_type {
        "alert" => {
            let sig = s(&v, &["alert", "signature"]);
            let sid = v["alert"]["signature_id"].as_u64().unwrap_or(0);
            let category = s(&v, &["alert", "category"]);
            let level = v["alert"]["severity"].as_i64().unwrap_or(3);
            let action = s(&v, &["alert", "action"]);
            let title = if sig.is_empty() {
                "alert".to_string()
            } else {
                sig.to_string()
            };
            let detail = format!(
                "{} {}:{} → {}:{}  {}  sid {sid}{}",
                proto,
                s(&v, &["src_ip"]),
                v["src_port"].as_u64().unwrap_or(0),
                s(&v, &["dest_ip"]),
                dport,
                category,
                if action.is_empty() {
                    String::new()
                } else {
                    format!("  ({action})")
                }
            );
            out.push(Event::Signal {
                id: format!("sid:{sid}:{unit}"),
                unit: unit.clone(),
                severity: severity(level, category),
                title,
                detail,
                kind: category.to_string(),
                at,
            });
            if !peer.is_empty() && other.is_some_and(|o| !internal(&o)) {
                out.push(Event::Artifact {
                    unit,
                    kind: "ip".into(),
                    target: peer,
                    label: String::new(),
                    at,
                });
            }
        }
        "anomaly" => {
            let what = s(&v, &["anomaly", "event"]);
            out.push(Event::Signal {
                id: format!("anomaly:{what}:{unit}"),
                unit,
                severity: Severity::Low,
                title: format!("anomaly: {what}"),
                detail: s(&v, &["anomaly", "type"]).to_string(),
                kind: "anomaly".into(),
                at,
            });
        }
        "flow" | "dns" | "http" | "tls" | "ssh" | "smb" | "fileinfo" => {
            let (summary, artifact) = match event_type {
                "dns" => {
                    // eve v1 puts the name at dns.rrname, v2+ under dns.queries[0].
                    let name = s(&v, &["dns", "rrname"]);
                    let name = if name.is_empty() {
                        v["dns"]["queries"][0]["rrname"].as_str().unwrap_or("")
                    } else {
                        name
                    };
                    (format!("dns {name}"), Some(("domain", name.to_string())))
                }
                "http" => {
                    let hostname = s(&v, &["http", "hostname"]);
                    (
                        format!(
                            "http {} {}{} {}",
                            s(&v, &["http", "http_method"]),
                            hostname,
                            s(&v, &["http", "url"]),
                            v["http"]["status"].as_u64().map(|c| c.to_string()).unwrap_or_default()
                        ),
                        Some(("domain", hostname.to_string())),
                    )
                }
                "tls" => {
                    let sni = s(&v, &["tls", "sni"]);
                    (
                        format!("tls {sni} {}", s(&v, &["tls", "version"])),
                        Some(("domain", sni.to_string())),
                    )
                }
                "fileinfo" => {
                    let name = s(&v, &["fileinfo", "filename"]);
                    (format!("file {name}"), Some(("file", name.to_string())))
                }
                "flow" => (
                    format!("flow {proto} {peer}:{dport} {}", s(&v, &["flow", "state"])),
                    None,
                ),
                other => (format!("{other} {peer}:{dport}"), None),
            };
            out.push(Event::ActionEnd {
                unit: unit.clone(),
                action: format!("{event_type}:{flow}"),
                ok: true,
                output: summary.trim().to_string(),
                at,
            });
            if let Some((kind, target)) = artifact
                && !target.is_empty()
            {
                out.push(Event::Artifact {
                    unit,
                    kind: kind.into(),
                    target,
                    label: String::new(),
                    at,
                });
            }
        }
        _ => return Vec::new(),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gae_engine::core::World;

    const ALERT: &str = r#"{"timestamp":"2026-10-29T15:04:05.123456+0000","flow_id":1234,"event_type":"alert","src_ip":"203.0.113.9","src_port":51515,"dest_ip":"10.0.1.20","dest_port":80,"proto":"TCP","alert":{"action":"allowed","signature_id":2010935,"signature":"ET SCAN Suspicious inbound to mySQL port 3306","category":"Potentially Bad Traffic","severity":2}}"#;

    #[test]
    fn alert_becomes_a_signal_on_the_internal_host() {
        let evs = map_line(ALERT);
        let mut w = World::new("SOC");
        for e in evs {
            w.apply(e);
        }
        let host = w.unit("10.0.1.20").unwrap();
        assert_eq!(host.parent.as_deref(), Some("10.0.1.0/24"));
        assert_eq!(host.kind, "host");
        assert_eq!(w.open.len(), 1);
        let s = &w.open[0];
        assert_eq!(s.severity, Severity::Medium);
        assert_eq!(s.id, "sid:2010935:10.0.1.20");
        assert!(s.detail.contains("203.0.113.9:51515"));
        assert_eq!(host.artifacts[0].target, "203.0.113.9");
        // The same alert again is a repeat, not a second queue item.
        for e in map_line(ALERT) {
            w.apply(e);
        }
        assert_eq!(w.open.len(), 1);
        assert_eq!(w.open[0].count, 2);
    }

    #[test]
    fn severity_mapping_and_critical_categories() {
        assert_eq!(severity(1, "Misc"), Severity::High);
        assert_eq!(severity(3, "Misc"), Severity::Low);
        assert_eq!(severity(3, "A Network Trojan was detected"), Severity::Critical);
        assert_eq!(severity(9, ""), Severity::Info);
    }

    #[test]
    fn protocol_records_are_actions_with_artifacts() {
        let dns = r#"{"event_type":"dns","flow_id":7,"src_ip":"10.0.2.5","dest_ip":"10.0.2.1","dest_port":53,"proto":"UDP","dns":{"type":"query","rrname":"updates.example.com"}}"#;
        let mut w = World::new("SOC");
        for e in map_line(dns) {
            w.apply(e);
        }
        // Internal to internal: the target (the resolver, a gateway) is the unit.
        let gw = w.unit("10.0.2.1").unwrap();
        assert_eq!(gw.kind, "gateway");
        assert!(gw.actions[0].output.contains("updates.example.com"));
        assert_eq!(gw.artifacts[0].kind, "domain");
        assert!(w.unit("10.0.2.5").is_some(), "the client appears too");
    }

    #[test]
    fn external_only_and_garbage_lines_are_skipped() {
        let ext = r#"{"event_type":"flow","src_ip":"8.8.8.8","dest_ip":"1.1.1.1"}"#;
        assert!(map_line(ext).is_empty());
        assert!(map_line("not json").is_empty());
        assert!(map_line(r#"{"event_type":"stats","src_ip":"10.0.0.2"}"#).is_empty());
    }

    #[test]
    fn subnets_for_v4_and_v6() {
        assert_eq!(subnet_of(&"192.168.7.42".parse().unwrap()), "192.168.7.0/24");
        assert_eq!(subnet_of(&"fd00:1:2:3::9".parse().unwrap()), "fd00:1:2:3::/64");
        assert!(internal(&"fd00::1".parse().unwrap()));
        assert!(!internal(&"2001:db8::1".parse().unwrap()));
    }
}
