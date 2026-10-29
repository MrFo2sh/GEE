# Security review — GAE 0.1.0

1. Inputs: CLI flags (clap), files named on the command line (read as lines, parsed by serde/pure mappers; bad lines counted, never fatal), HTTP `POST /events` (UTF-8, ≤ 4 MB, ≤ 10,000 events, serde-typed schema), no env beyond `GAE_SHOT*` dev aids.
2. Files: only paths the user passes (`--eve`, `--file`); read-only; app writes only `<config dir>/gae/<slug>/` (settings, game) and the `GAE_SHOT` path the user sets.
3. Shell: no subprocesses.
4. Network: no outbound calls. Inbound HTTP binds 127.0.0.1 only, no auth by design (local tools and hooks post to it, like agent-globe); requests with an `Origin` header are refused (403) so browser pages cannot write; unknown routes 404.
5. Secrets: none read, none stored. Fixtures use RFC 1918 / RFC 5737 addresses and example.com names.
6. Dependencies: Cargo.lock committed; `cargo audit` runs in check (exceptions, if any, listed below).
7. Permissions: user-level process, no elevation; the Suricata log is read with the user's own rights.
8. Data: in memory, bounded (4,000 units, 400 actions per unit, 2,000 open signals, 500 closed). On disk: settings and game stats only (no event content). Delete `%APPDATA%\gae\<slug>` to reset.
9. DoS: body and event caps, bounded channel (HTTP answers 503 when full, sources block), 64 messages applied per frame, text fields clipped to 16 KB, file tail reads ≤ 4 MB per poll.
10. Defensive scope: visualizes and triages events; it sends nothing, scans nothing, executes nothing. Artifact URLs open in the browser only on a user click (http/https only). The playbook forbids attack tooling in environments.

Audit exceptions: see the check log; none expected.
