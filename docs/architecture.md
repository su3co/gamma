# gamma architecture

gamma is a desktop workspace for designing, simulating and testing photonic
integrated circuits (PICs). It covers the working loop of the Lumerical
interface (layout and object tree, materials and layer stack, mesh, sources
and monitors, run, results, sweeps, scripting) on top of two open solvers,
**lightsim** (GPU FDTD) and **lightsim-quantum** (quantum light in circuits),
and it is built so that an AI agent can do everything a person can, in the
same project, while the person watches and steers.

This page records the components, the processes, the data formats, the
choice of GUI technology and what the first release leaves out.

## 1. Principles

1. **A project is a folder of text files under git.** gdsfactory Python and
   YAML for layout, TOML for the layer stack and the project, lightsim JSON
   for simulations, lightsim-quantum JSON for circuits. The GDS and results
   are generated files. No binary project format, nothing a text editor or an
   agent cannot change. gamma never holds state that is not on disk.
2. **One hub per machine owns everything that changes.** Event log, file
   watching, git, simulation jobs and agent processes live in one process
   (`gamma hub`). Windows, browser tabs, the MCP server and the CLI are
   clients. Two clients never disagree, because there is one writer.
3. **Agents and people use the same verbs.** Every action in the interface is
   a hub call; the MCP tools and the CLI commands are thin wrappers around
   the same calls. If a person can do it by clicking, an agent can do it by
   calling a tool, and the other way round.
4. **The solver computes what the viewers show.** The set-up view shows the
   permittivity rasterised by lightsim's own rasteriser on lightsim's own
   grid, not a re-implementation. Estimates come from the same grid builder
   and time step as the run.
5. **Rust for everything that computes.** GDS parsing, geometry,
   triangulation, rasterisation, the simulation runner, result reduction,
   rendering for agents, the hub, the MCP server and the updater logic are
   Rust. TypeScript is used only for the interface layer inside the window.

## 2. Components

```
crates/
  gamma-core     project model (gamma.toml, stack.toml), GDS reader and writer-free
                 geometry, triangulation and extrusion, layout queries and diff,
                 design-rule and consistency checks, atomic file writes
  gamma-sim      lightsim integration: spec loading, set-up slices from lightsim's
                 rasteriser, estimates, a runner with live progress and field
                 slices, result reduction (spectra, S-parameters, field maps),
                 devices (CPU, CUDA), the remote-worker stream protocol
  gamma-quantum  integration point for lightsim-quantum (circuit format, runner
                 trait); a stub until that library has a stable API
  gamma-render   headless rendering to PNG (layout, 3-D, set-up, fields, plots)
                 for agents, thumbnails and the CLI; no window needed
  gamma-hub      the hub: HTTP + WebSocket API, event log, file watcher, git,
                 jobs (local and SSH), checkpoints and comments, agent sessions
  gamma-cli      the `gamma` program: `hub`, `mcp`, `worker`, and project
                 commands (`open`, `build`, `setup`, `run`, `results`, `check`,
                 `view`, `checkpoint`, `remote`, `agent`)
app/
  src-tauri      the desktop shell (Tauri 2): window, menus, hub start-up,
                 updater with channels and rollback
  ui             the interface (TypeScript, WebGL2): viewers, panels, the agent
                 conversation; served by the hub to browsers as well
python/gamma_pic a small Python helper used by project scripts: write a layout
                 (GDS + ports JSON) and a simulation spec where gamma expects them
skills/          skills shipped to agents (gamma, gdsfactory, lightsim,
                 lightsim-quantum)
packaging/       update feeds, Linux package repositories, release scripts
```

## 3. Processes

```
 person                         agent (Claude Code, Codex, ...)
   |                                 |  stdio MCP
 gamma window (Tauri) ----+     gamma mcp ----+
 browser tab -------------+                   |
 gamma CLI ---------------+--- HTTP/WS ---> gamma hub  (one per machine and user)
                                               |  owns: projects, event logs, watchers,
                                               |        git, jobs, agent processes
                                               +--> local runner threads (CPU, CUDA)
                                               +--> ssh HOST [launcher] gamma worker ...
                                                       (remote GPU host, NDJSON stream)
```

- **gamma hub** listens on `127.0.0.1` on a free port and writes its port,
  process id, version and a random access token to
  `<runtime dir>/gamma/hub.json` (mode 0600). A lock file makes it the only
  hub of that user. Every request carries the token (header or, for the
  WebSocket, a query parameter). The hub never listens on other interfaces;
  a remote person reaches it through an SSH tunnel.
- **The window** starts the hub if `hub.json` names no live hub (the `gamma`
  program is bundled next to the app), then loads the interface. Closing the
  window leaves the hub running while jobs or agents are active, and stops it
  after an idle period otherwise.
- **gamma mcp** is started by the agent's CLI. It finds or starts the hub and
  forwards tool calls to it, tagging events with the agent's session, so the
  viewers show who changed what.
- **gamma worker** runs on a simulation host. It reads a lightsim spec, runs
  it on the requested device and writes newline-delimited JSON to stdout:
  `hello` (version, devices), `plan` (cells, steps, memory), `progress` (step,
  decay, rate), `slice` (a field plane, f32, base64), `done` (statistics) or
  `error`. The hub reads the stream over SSH and fetches the results file.
  A host entry may name a *launcher*, a command prefix such as a site's job
  queue, that the hub puts in front of the worker command; gamma ships no
  site-specific launcher.

## 4. Data formats

### Project folder

```
gamma.toml              project manifest (below)
stack.toml              layer stack and materials
layout/                 gdsfactory sources (*.py, *.pic.yml)
sims/                   simulation generators (*.py) or hand-written specs (*.json)
circuits/               lightsim-quantum circuits (*.json)
build/                  generated: GDS, ports, specs (git-ignored)
results/<sim>/<run>/    results.json (lightsim format), run.json (provenance)
.gamma/comments.jsonl   review comments (committed)
.gamma/local/           event log and caches (git-ignored)
```

`gamma.toml`:

```toml
[project]
name = "Racetrack ring sensor"

[python]
interpreter = "python3"          # or a project venv

[layout]
gds = "build/ring.gds"           # what the viewers read
generator = "layout/ring.py"     # optional: regenerates the GDS and ports
ports = "build/ring.ports.json"  # optional: ports written by the generator
stack = "stack.toml"

[[simulation]]
name = "ringdown"
spec = "build/sims/ringdown.json"   # a lightsim simulation file
generator = "sims/ringdown.py"      # optional: writes the spec
[simulation.preview]                # optional: a coarse variant for quick runs
mesh_scale = 2.0
run_time_scale = 0.5

[[circuit]]
name = "hom"
file = "circuits/hom.json"          # lightsim-quantum, when available

[[remote]]                          # usually in the user's config instead
name = "gpu-server"
ssh = "user@host"
launcher = ""                       # optional command prefix (a job queue)
```

`stack.toml` lists layers (GDS layer and datatype, z range, material, colour,
minimum width and spacing for the design-rule check) and materials (a
constant index or a lightsim library name).

A simulation with a `[simulation.preview]` table also has a coarse variant.
Its script builds it when the hub runs it with `GAMMA_PREVIEW_MESH_SCALE`,
`GAMMA_PREVIEW_RUN_TIME_SCALE` and `GAMMA_SPEC_OUT` set (`gamma_pic.mesh_scale()`
and friends), because mode profiles belong to the grid they were solved on;
specs without mode profiles are coarsened by the hub itself.

Ports are read from the ports JSON when the project names one, else from the
port metadata gdsfactory writes into the GDS file.

### Simulation specs and results

The lightsim JSON format, unchanged (`docs/reference/json-spec.md` in
lightsim). gamma adds nothing to these files. Run provenance goes next to the
results in `run.json`: spec hash, git commit of the project, device, host,
lightsim version, wall time, measured throughput.

### Event log

`.gamma/local/events.jsonl`: one JSON object per line,
`{"seq", "time", "actor", "kind", "data"}`, appended and fsynced by the hub
only. Kinds: `file.changed`, `layout.built`, `sim.queued`, `sim.progress`
(not logged, streamed), `sim.done`, `selection`, `checkpoint`, `comment`,
`agent.message`, `agent.tool`, `error`. On start-up a torn last line is
dropped. Clients subscribe over a WebSocket and receive every event after the
sequence number they name, so a client that reconnects misses nothing.

### Design history

A checkpoint is a git commit of the project folder made by the hub, tagged
`gamma/checkpoint/<n>`, with a message naming the actor (person or agent
session) and what changed. The hub checkpoints at the end of each agent turn
and when a person asks. Two checkpoints compare as a **layout diff**
(polygons added and removed per layer, computed from the two GDS files) and a
**result diff** (the metrics of the runs attached to each). Comments are
anchored to a checkpoint and optionally to an object (cell, port, monitor,
point) and live in `.gamma/comments.jsonl`.

### Object references

Every selectable object has a stable text reference used by the interface,
the event log, the MCP tools and the conversation: `cell:ring`,
`layer:1/0`, `port:o1`, `poly:1/0#12`, `sim:ringdown`,
`monitor:ringdown/through`, `source:ringdown/dipole`, `point:1.25,-3.0`. A
click in any viewer publishes a `selection` event; every viewer highlights
the referenced object; the conversation inserts the reference with a short
description, and the agent reads it with the `selection_get` tool.

## 5. Simulation

- **Local CPU** through `lightsim-core` linked into the hub, single or double
  precision as lightsim is built.
- **Local NVIDIA GPUs** (Linux) through `lightsim-cuda`, one or several GPUs
  per run (`cuda:0`, `cuda:0,1`), when gamma is built with the `cuda`
  feature.
- **Remote hosts over SSH.** `gamma remote add` records the host;
  `gamma remote install` copies the matching `gamma` program (Linux x86_64 or
  aarch64, CUDA build) to `~/.local/share/gamma/<version>/` on the host. Jobs
  are queued per host in the hub; progress and field slices stream back; the
  results file is copied into `results/<sim>/<run>/`.

Before a run the hub builds lightsim's grid for the spec and reports cells,
steps, memory and an estimated time per available device from a throughput
table that each finished run updates. During a run the runner reports step,
energy decay relative to its peak (what lightsim's shut-off compares) and
throughput, and sends a field slice of the selected component and plane about
once a second. The runner drives lightsim's backend step by step through the
public `Backend` trait (one slab); multi-GPU runs use lightsim's own
multi-slab driver without live slices until lightsim's run hook is released.

## 6. Interface

Panels: the project tree (files, cells, layers, ports, simulations with
their sources and monitors, runs, circuits, checkpoints), the viewers in tabs
(Layout, 3-D, Set-up, Run, Results, Circuit), an inspector for the selected
object (parameters, editable where the source is a text value), and the
conversation with the agent. Every action has a keyboard path; the command
palette (Cmd/Ctrl-K) lists them all.

Rendering: layouts are triangulated in Rust and drawn with WebGL2 (one draw
call per layer, instanced port glyphs), so layouts with millions of vertices
pan and zoom at frame rate; field maps are float textures with the colour map
applied in a shader; the 3-D view is an extruded mesh built in Rust. The same
scenes are drawn without a window by `gamma-render` for agents and the CLI.

Typography and colour follow the lightsim site: Archivo for names and
headings, Literata for sentences, IBM Plex Mono for numbers and code; dark
and light themes; no logo mark, plain text navigation.

## 7. GUI technology: Tauri 2 with a WebGL2 interface

Considered:

| | Packaging and updates | Rendering | Typography and layout | Effort |
|---|---|---|---|---|
| **Tauri 2** (Rust shell, system web view) | signed updater with feeds for macOS, AppImage and Windows; dmg, AppImage, deb, rpm, msi/nsis bundlers | WebGL2 (GPU) in every supported web view | full CSS, web fonts | low; the interface is also servable to browsers |
| GPUI (as Backplane) | none built in; custom guard and installers | native GPU | good, custom widgets | high: widgets, text input, accessibility by hand |
| egui / eframe + wgpu | none built in (cargo-packager possible) | native GPU | immediate-mode look, limited text layout | medium |
| Qt / Slint | own installers; licence questions (Qt) | GPU | good | medium-high, C++ or DSL |

Tauri wins on the two requirements that are expensive to build and easy to
get wrong, platform-standard packaging and signed automatic updates, and the
web layer gives the typographic control the design calls for. The cost is a
TypeScript interface layer; everything that computes stays in Rust, and the
same interface is served by the hub to a browser, which gives a web client
for free (as Backplane has). WebGL2 is available in WKWebView (macOS),
WebKitGTK (Linux) and WebView2 (Windows).

## 8. Packaging, updates and release

- **macOS (arm64):** `gamma.app` in a `.dmg`, signed with a Developer ID and
  notarised (hardened runtime; the bundled `gamma` program is signed too).
  Updates through the Tauri updater: an ed25519-signed `latest.json` per
  channel (stable, beta) pointing at a signed `.app.tar.gz`.
- **Linux (x86_64, aarch64):** an AppImage that updates itself through the
  same updater, and `.deb` / `.rpm` packages served from an apt and a dnf
  repository (signed metadata) so the system updater keeps them current. An
  installed package never replaces itself; it tells the person an update is
  available.
- **Windows (later):** NSIS installer and the same updater; the code keeps
  paths, processes and the hub's runtime directory portable.
- **Channels:** stable and beta feeds; the channel is a setting.
- **Signature checks:** the updater refuses a package whose ed25519 signature
  does not verify with the public key compiled into the app.
- **Rollback:** before installing, the app keeps the current bundle
  (`.app` or AppImage) in its data folder. A newly installed version starts
  on probation; if it fails to reach a healthy state (hub reachable,
  interface loaded) on two consecutive launches, the next launch restores the
  kept bundle. A person can also go back from Settings.
- **Release notes** are `CHANGELOG.md` sections, copied into the feed and the
  release.
- **CI:** GitHub Actions runs format, lint, unit and integration tests and a
  headless UI smoke test on every push; tags `v*` build, sign and publish.
  GPU tests (CUDA runner, remote worker) run on the owner's machines; until
  self-hosted runners are approved they are run by hand and recorded in
  `docs/test-runs/`.

## 9. Security and privacy

No telemetry. The hub binds to loopback only and requires a per-start
token. Update checks are the only network traffic gamma starts by itself and
can be turned off. Agent processes run with the project folder as working
directory and the person's own agent credentials; gamma does not see or
store them. SSH uses the person's own SSH configuration and keys.

## 10. Left out of the first release

- Windows builds (code and packaging kept portable; no machine to test on).
- lightsim-quantum integration beyond the format and the integration point:
  the library has no stable API yet.
- Interactive drawing of arbitrary polygons in the layout view: layout is
  edited as gdsfactory Python or YAML (by the person or the agent), and the
  interface edits parameters, placements and connections in those files.
- Lumerical's INTERCONNECT-style circuit simulator, CHARGE/HEAT solvers and
  the eigenmode expansion solver.
- Multi-user collaboration on one hub, and phone clients.
- Live field slices for multi-GPU and multi-node runs (single-device runs
  have them).
