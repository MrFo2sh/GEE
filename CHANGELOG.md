# Changelog

## Unreleased

## 0.1.0 - 2026-10-29
- First release. `gae-core`: event schema, bounded `World` reducer with a severity-ordered
  Needs-You queue, orbital layout that grows crowded rings, game rules (XP by severity, fast
  bonus, streaks broken by stale signals, levels, achievements), `JsonlSource`, `Replay`, file
  tail, and a bounded bridge with HTTP ingest on 127.0.0.1 (`/events`, `/health`, `/state`)
  that refuses browser requests.
- `gae-engine`: HDR orbit camera, units shaped by kind and colored by status, halos, beams with
  tracers, HUD with XP bar and streak, Needs-You queue with handle/dismiss, unit tree and panel,
  signal badges, toasts, level-up flash, dockable settings and achievements cards, radio cues
  with device picker, persisted settings and progress, `GAE_SHOT` self-screenshot.
- `envs/_template` and `tools/new-env.ps1`; `envs/soc-desk` over Suricata eve.json with `--demo`.
- `AGENTS.md`: the playbook for building an environment from a one-line idea.
