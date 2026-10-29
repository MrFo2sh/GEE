# GAE — playbook for any coding agent

GAE (Gamify Any Environment) turns a work domain into a live, GPU-rendered constellation with
a Needs-You queue and a game loop. You are here because a user described an environment
("gamify my SOC", "my homelab", "my bug bounty program", "my CI fleet"). Build it as a new
environment on this kit. Do not fork the engine; do not start from scratch.

Reference implementation: `products/agent-globe` (same stack; read its AGENTS.md "Rules that
cost us time" before touching rendering or egui). Factory products: `products/<slug>` in
`D:\software-factory`; this folder is `products/gae`, its own git repo.

## The stack is fixed

Rust 2024 + Bevy 0.20 (wgpu) + bevy_egui 0.43 (egui 0.36) + bevy_panorbit_camera 0.36, pinned in
the workspace `Cargo.toml`. Environments must hold 60 fps with thousands of live objects, so no
web views, no Electron, no Python UI. A version bump of bevy or bevy_egui is its own change,
done for the whole workspace at once.

## Map

| Path | Owns |
|---|---|
| `crates/gae-core/src/model.rs` | `Event` schema (serde, `type` tag), `World` reducer, bounded histories, `Change`s, signals queue (`queue()`, `close()`, `mark_stale()`). |
| `crates/gae-core/src/layout.rs` | Orbital layout; rings grow with their population. |
| `crates/gae-core/src/game.rs` | `Game` (XP, level, streak, stats), `XpRules`, `Achievement`, `Reward`. Pure. |
| `crates/gae-core/src/spec.rs` | `Spec`: name, slug, nouns, kinds (shape, color, size), palette, XP rules, achievements, sounds, port. `validate()`. `Cue` list. |
| `crates/gae-core/src/source.rs`, `tail.rs` | `Source` trait (own thread each), `JsonlSource` (tail + mapper), `Replay` (demo timelines), `FileTail`. |
| `crates/gae-core/src/bridge.rs`, `ingest.rs` | Bounded channel to the app; HTTP on 127.0.0.1: `POST /events`, `GET /health`, `GET /state`. Refuses requests with an `Origin` header (browsers). |
| `crates/gae-engine/src/lib.rs` | `run(spec, sources, EngineConfig)`, window, F11, Ctrl +/-/0, `GAE_SHOT`. |
| `crates/gae-engine/src/store.rs` | `Store`: world, game, selection, toasts, cues; `drain_bridge` (64 messages a frame), `close_signal`, `/state` JSON, `game.json`. |
| `crates/gae-engine/src/scene.rs` | Units (shape by kind, color by status), rings, halos, beams with tracers, dust, stars, badge projection, picking. |
| `crates/gae-engine/src/camera.rs` | HDR orbit camera, viewport between panels, fly-to, idle orbit, R / F / Esc. |
| `crates/gae-engine/src/ui.rs` | HUD with XP bar, Needs-You queue, unit tree, unit panel, badges, toasts, level flash, settings and trophies cards. |
| `crates/gae-engine/src/cards.rs` | Dockable floating cards (ported from agent-globe). |
| `crates/gae-engine/src/sounds.rs`, `settings.rs`, `theme.rs` | Radio cues on a chosen device, persisted settings, palette to Bevy/egui colors. |
| `envs/_template` | What `tools/new-env.ps1` copies. |
| `envs/soc-desk` | The worked example: Suricata eve.json, demo timeline, tests. Read it before writing yours. |

Data flow: source threads / HTTP → bounded channel → `drain_bridge` → `World::apply` →
`Change`s → cues + `Game::score` → toasts. Scene and UI read the store every frame.

## Build an environment from a prompt

1. **Map the domain** (answer in `src/domain.rs`'s header comment, five lines):
   unit and its grouping, action, signal (what needs the human), resolve vs dismiss, feed format.
   If the user's idea has no "needs the human" moment, find one (failed job, finding to verify,
   SLA about to break); without signals there is no game.
2. **Scaffold**: `powershell -NoProfile -File tools/new-env.ps1 -Slug <slug> -Title "<Title>"`.
   It copies the template, picks a free port, adds the workspace member. `cargo test -p <slug>`
   passes before you change anything.
3. **Spec** (`domain.rs`): nouns in the user's words, 2–5 kinds with distinct shapes, XP rules
   (`stale_ms` = how long a signal may wait in this domain), 1–3 domain achievements on top of
   `default_achievements()`. `Spec::validate()` must return nothing (a test checks it).
4. **Source** (`source.rs`, or a module per format like soc-desk's `eve.rs`): a pure
   `map_line(&str) -> Vec<Event>` for the real feed, with tests built from real sample lines.
   Stable signal ids (`<rule>:<unit>`) so repeats aggregate instead of flooding the queue.
   Live polling of an API: implement `Source` yourself; it runs on its own thread, may block.
5. **Demo** (`--demo`): a `Replay` that shows every kind and every severity within a minute,
   then leaves quiet time to triage. Prefer writing the demo in the feed's own format and
   passing it through `map_line` (soc-desk does), so the demo tests the mapper.
6. **Verify**: `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`, then run
   it and look:
   ```
   $env:GAE_SHOT="shot.png"; $env:GAE_SHOT_AT="30"; cargo run -p <slug> -- --demo
   curl.exe -s http://127.0.0.1:<port>/state
   ```
   `/state` says what the user sees (units, open queue, xp, level, badges). The screenshot is the
   window as rendered. Never drive the user's mouse or keyboard to test; ask them to click.
7. **Document** the environment in `README.md` (a section: what it shows, how to feed it).

If the environment is a factory product of its own (`products/<slug>`), use the factory's
`gamified-env` blueprint: it depends on these crates by path.

## Adding to the engine

Only when two environments need the same thing. Keep `gae-core` free of Bevy. New `Event`
variants are additive with `#[serde(default)]` fields: old feeds must keep parsing. Every list
stays bounded and every UI list paginates (`theme::PAGE`). New settings get a default and
`#[serde(default)]` already covers old files.

## Rules that cost time before (from agent-globe)

- Bevy system tuples hold 20 systems at most: chain groups.
- egui 0.36: `Panel::top/left/...` inside a root `Ui`; a font family installed this frame
  cannot be used this frame (`style_once` returns true and the frame is skipped).
- Fonts lack ◆ ● ◉ and many symbols: paint shapes (`kind_icon`, `dot`) instead of glyphs.
- Two widgets with one id print "Double use of widget ID" in red over the scene: give every
  interactive thing its own `Id` (see the badge click sensor).
- Emissive strengths are nits: 40–220 glows, 1–5 is invisible.
- `world_to_viewport` returns window logical coordinates; divide by `ui_scale`, add no insets.
- A drag needs real pointer motion before it counts (cards.rs), or slow clicks become drags.
- The Bash tool mangles backslashes in heredocs: write patch scripts with the Write tool and run
  `python -I <file>`. Run `cargo fmt` before anchoring a patch on code.
- Settings are saved by the running app on change: edit `settings.json` only while it is closed.
- Defensive only. A red-team environment organizes an authorized engagement's notes, scope and
  tool output; it never runs attacks, stores credentials it captured, or evades detection.

## Loop for changes here

1. Change one thing; `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`.
2. Bump `version` in the workspace `Cargo.toml` and `factory.json`; add to `CHANGELOG.md` under `## Unreleased`.
3. From `D:\software-factory`: `node bin/factory.mjs check gae`, then `node bin/factory.mjs ship gae`
   (gate it: `grep -q 'version = "X"' Cargo.toml && ...`). Commit the factory ledger. Pushing is the owner's job.
