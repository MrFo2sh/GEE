//! SOC Desk: Suricata's eve.json as a constellation. Subnets orbit the SOC, hosts orbit their
//! subnet, alerts land in the Needs-You queue, triage earns XP.

mod demo;
mod domain;
mod eve;

use clap::Parser;
use gae_engine::core::Source;
use gae_engine::{EngineConfig, run};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "SOC Desk: Suricata eve.json, gamified")]
struct Args {
    /// Play a scripted attack-and-noise timeline (no sensor needed).
    #[arg(long)]
    demo: bool,
    /// Suricata eve.json to follow (default when not in demo: /var/log/suricata/eve.json).
    #[arg(long)]
    eve: Option<PathBuf>,
    /// Read eve.json from the start instead of following new lines only.
    #[arg(long)]
    from_start: bool,
    /// Ingest port on 127.0.0.1 (default 4791).
    #[arg(long)]
    port: Option<u16>,
    /// Turn the HTTP ingest off.
    #[arg(long)]
    no_http: bool,
}

fn main() {
    let args = Args::parse();
    let spec = domain::spec();
    let mut sources: Vec<Box<dyn Source>> = Vec::new();
    let eve_path = args
        .eve
        .or_else(|| (!args.demo).then(|| PathBuf::from("/var/log/suricata/eve.json")));
    if let Some(path) = eve_path {
        sources.push(Box::new(eve::source(path, args.from_start)));
    }
    if args.demo {
        sources.push(Box::new(demo::replay()));
    }
    let port = if args.no_http {
        None
    } else {
        Some(args.port.unwrap_or(spec.port))
    };
    run(spec, sources, EngineConfig::default().port(port));
}
