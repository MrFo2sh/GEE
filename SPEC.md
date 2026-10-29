# GAE

**One-liner:** Gamify Any Environment. A Rust + Bevy engine kit and agent playbook that turns any work domain (blue team, red team, programming, ops) into a live, GPU-rendered constellation like Agent Globe.
**Blueprint:** factory/blueprints/desktop-app.md (and it defines the new `gamified-env` blueprint)
**Date:** 2026-10-29

## Problem
Agent Globe made watching coding agents playable, but its engine is welded to Claude transcripts.
Every new "gamify my X" idea would rebuild the camera, scene, queue, cards and sounds from scratch,
repeating the same Bevy and egui mistakes. Agents need one kit plus one playbook.

## MVP (ships today)
1. **Vertical slice:** an environment pushes events (HTTP `POST /events` or a `Source` polled on a
   worker thread), units appear on orbit rings, a signal lands in the Needs-You queue with a badge
   over its unit, the user resolves it and earns XP.
2. `gae-core` (no Bevy): event schema, `World` reducer with bounded histories, orbital layout,
   game rules (XP, levels, streaks, achievements), ingest parser, incremental file tail, bridge.
3. `gae-engine` (Bevy 0.20 + bevy_egui 0.43): HDR orbit camera, scene (core, units by kind and
   status, rings, halos, beams with tracers, dust, stars), HUD with XP bar, sidebar (queue + unit
   tree), unit panel (timeline, artifacts, metrics), dockable cards, sounds, persisted settings.
4. `envs/_template` and `tools/new-env.ps1`: a new environment is a copy plus a `Spec` and a `Source`.
5. `envs/soc-desk`: a blue-team SOC desk over Suricata `eve.json`, with `--demo` replay.
6. `AGENTS.md` playbook and the factory blueprint `gamified-env`.

## Not in MVP
See `BACKLOG.md`.

## Interfaces
- `gae_engine::run(spec, sources, EngineConfig)`; `trait Source { poll() }`; `Spec` (names, kinds,
  palette, XP rules, achievements, sound pack).
- HTTP on 127.0.0.1: `POST /events` (object, array or NDJSON), `GET /health`, `GET /state`.
- Keys: R reset view, F cinematic, Esc back, Ctrl +/-/0 zoom.

## Data
- Events: `unit_up`, `unit_down`, `action_start`, `action_end`, `signal`, `resolve`, `artifact`,
  `metric`, `log`. Units keyed by id with parent, kind, label, meta.
- `%APPDATA%/gae/<env-slug>/settings.json` and `game.json` (XP, level, streaks, badges, stats).

## Risks
- Engine compile time: shared workspace target dir, dev profile opt-level 3 for deps.
- Bevy API drift: pinned to the exact versions agent-globe already builds with.

## Done when
- `node bin/factory.mjs check gae` is green.
- `cargo run -p soc-desk -- --demo` shows hosts on rings, alerts in the queue; resolving one adds XP.
- A copied template compiles unchanged after `tools/new-env.ps1 my-env`.
