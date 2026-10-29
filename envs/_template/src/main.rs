//! A GAE environment: a `Spec` (domain.rs), sources (source.rs), and this entry point.
//! Usually nothing here needs changing: add flags for your sources and pass them on.

mod domain;
mod source;

use clap::Parser;
use gae_engine::core::Source;
use gae_engine::{EngineConfig, run};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "GAE environment")]
struct Args {
    /// Play the scripted demo instead of (or next to) the real feed.
    #[arg(long)]
    demo: bool,
    /// JSON-lines file to follow (one event per line, see the README for the schema).
    #[arg(long)]
    file: Option<PathBuf>,
    /// Read the file from the start instead of following new lines only.
    #[arg(long)]
    from_start: bool,
    /// Ingest port on 127.0.0.1 (default: the spec's).
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
    if let Some(path) = args.file {
        sources.push(Box::new(source::file(path, args.from_start)));
    }
    if args.demo {
        sources.push(Box::new(source::demo()));
    }
    let port = if args.no_http {
        None
    } else {
        Some(args.port.unwrap_or(spec.port))
    };
    run(spec, sources, EngineConfig::default().port(port));
}
