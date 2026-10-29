//! `--demo`: a looping timeline written as eve.json records and run through the real mapper,
//! so the demo exercises the same code a live sensor does. Addresses are RFC 1918 inside and
//! RFC 5737 documentation ranges outside: no real network is named.

use crate::eve::map_line;
use gae_engine::core::model::Event;
use gae_engine::core::source::Replay;
use serde_json::json;
use std::time::Duration;

const HOSTS: [&str; 14] = [
    "10.0.1.1",
    "10.0.1.10",
    "10.0.1.11",
    "10.0.1.20",
    "10.0.1.21",
    "10.0.2.1",
    "10.0.2.5",
    "10.0.2.6",
    "10.0.2.7",
    "10.0.2.8",
    "10.0.3.1",
    "10.0.3.30",
    "10.0.3.31",
    "10.0.3.40",
];

fn ev(v: serde_json::Value) -> Vec<Event> {
    map_line(&v.to_string())
}

fn dns(client: &str, resolver: &str, name: &str, flow: u64) -> Vec<Event> {
    ev(
        json!({"event_type":"dns","flow_id":flow,"src_ip":client,"dest_ip":resolver,"dest_port":53,"proto":"UDP","dns":{"type":"query","rrname":name}}),
    )
}

fn http(host: &str, outside: &str, name: &str, url: &str, flow: u64) -> Vec<Event> {
    ev(
        json!({"event_type":"http","flow_id":flow,"src_ip":host,"dest_ip":outside,"dest_port":443,"proto":"TCP","http":{"hostname":name,"url":url,"http_method":"GET","status":200}}),
    )
}

fn alert(src: &str, dst: &str, dport: u16, sid: u64, sig: &str, category: &str, severity: u8) -> Vec<Event> {
    ev(
        json!({"event_type":"alert","flow_id":sid,"src_ip":src,"src_port":40000 + (sid % 20000),"dest_ip":dst,"dest_port":dport,"proto":"TCP",
        "alert":{"action":"allowed","signature_id":sid,"signature":sig,"category":category,"severity":severity}}),
    )
}

pub fn steps() -> Vec<(Duration, Vec<Event>)> {
    let ms = Duration::from_millis;
    let mut steps: Vec<(Duration, Vec<Event>)> = Vec::new();
    // The network wakes up: every host resolves something.
    for (i, h) in HOSTS.iter().enumerate() {
        let resolver = format!("{}.1", &h[..h.rfind('.').unwrap_or(0)]);
        steps.push((ms(250), dns(h, &resolver, "time.example.com", 100 + i as u64)));
    }
    // Background noise: browsing, updates.
    for (i, h) in HOSTS.iter().enumerate().filter(|(_, h)| !h.ends_with(".1")) {
        steps.push((
            ms(300),
            http(h, "198.51.100.20", "updates.example.com", "/feed.json", 300 + i as u64),
        ));
    }
    // The story, one beat every few seconds.
    let beats: Vec<Vec<Event>> = vec![
        alert(
            "203.0.113.9",
            "10.0.1.20",
            3306,
            2010935,
            "ET SCAN Suspicious inbound to mySQL port 3306",
            "Attempted Information Leak",
            2,
        ),
        alert(
            "203.0.113.9",
            "10.0.1.21",
            3306,
            2010935,
            "ET SCAN Suspicious inbound to mySQL port 3306",
            "Attempted Information Leak",
            2,
        ),
        alert(
            "10.0.2.6",
            "198.51.100.77",
            80,
            2013028,
            "ET POLICY curl User-Agent Outbound",
            "Attempted Information Leak",
            3,
        ),
        alert(
            "203.0.113.44",
            "10.0.1.10",
            22,
            2001219,
            "ET SCAN Potential SSH Scan",
            "Attempted Information Leak",
            2,
        ),
        alert(
            "203.0.113.44",
            "10.0.1.10",
            22,
            2001219,
            "ET SCAN Potential SSH Scan",
            "Attempted Information Leak",
            2,
        ),
        alert(
            "203.0.113.44",
            "10.0.1.10",
            22,
            2006546,
            "ET SCAN LibSSH Based Frequent SSH Connections Likely BruteForce Attack",
            "Attempted Administrator Privilege Gain",
            1,
        ),
        dns("10.0.3.31", "10.0.3.1", "cdn-sync.example.net", 900),
        alert(
            "10.0.3.31",
            "192.0.2.66",
            443,
            2027758,
            "ET MALWARE Observed Malicious SSL Cert (C2)",
            "A Network Trojan was detected",
            1,
        ),
        alert(
            "10.0.2.7",
            "198.51.100.5",
            80,
            2100498,
            "GPL ATTACK_RESPONSE id check returned root",
            "Potentially Bad Traffic",
            2,
        ),
        alert(
            "10.0.2.8",
            "10.0.2.1",
            445,
            2025649,
            "ET POLICY SMB2 NT Create AndX Request For an Executable File",
            "Potential Corporate Privacy Violation",
            3,
        ),
        ev(
            json!({"event_type":"anomaly","src_ip":"10.0.3.40","dest_ip":"10.0.3.1","anomaly":{"type":"decode","event":"ipv4.trunc_pkt"}}),
        ),
        alert(
            "203.0.113.9",
            "10.0.1.11",
            80,
            2019401,
            "ET WEB_SERVER Possible SQL Injection Attempt UNION SELECT",
            "Web Application Attack",
            1,
        ),
    ];
    for b in beats {
        steps.push((Duration::from_secs(4), b));
    }
    // Quiet time to triage before the loop starts again.
    steps.push((Duration::from_secs(60), Vec::new()));
    steps
}

pub fn replay() -> Replay {
    Replay::new("demo", steps(), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gae_engine::core::World;
    use gae_engine::core::model::Severity;

    #[test]
    fn the_story_covers_every_severity_and_three_subnets() {
        let mut w = World::new("SOC");
        for (_, evs) in steps() {
            for e in evs {
                w.apply(e);
            }
        }
        assert!(w.unit("10.0.1.0/24").is_some());
        assert!(w.unit("10.0.2.0/24").is_some());
        assert!(w.unit("10.0.3.0/24").is_some());
        let sev: Vec<Severity> = w.open.iter().map(|s| s.severity).collect();
        for want in [Severity::Critical, Severity::High, Severity::Medium, Severity::Low] {
            assert!(sev.contains(&want), "missing {want:?}");
        }
        let scan = w.open.iter().find(|s| s.id == "sid:2001219:10.0.1.10").unwrap();
        assert_eq!(scan.count, 2, "repeats aggregate");
        assert!(w.units.len() > 14);
    }
}
