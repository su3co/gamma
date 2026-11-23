# gamma

**An open-source workspace for designing, simulating and testing photonic
integrated circuits, built for AI agents.** gamma covers the working loop of
the Lumerical interface — layout and object tree, layer stack and materials,
mesh, sources and monitors, run, results — on top of
[lightsim](https://github.com/su3co/lightsim), SU3 Labs' GPU FDTD solver,
with lightsim-quantum for circuits of quantum light to follow. The agent of
your choice (Claude Code, Codex, or any MCP client) works in the same project
with the same tools you use, and every view follows its edits as it makes
them.

![Results of a 1×2 MMI splitter: S-parameters and the field in the core plane](docs/img/results-dark.png)

- **Projects are folders of text under git**: a GDS file from any design
  tool, or gdsfactory Python or YAML, for layout; `stack.toml` for the layer
  stack; lightsim JSON for simulations. Nothing is locked in a binary project
  file, and S-parameters export to Touchstone and Lumerical INTERCONNECT text.
- **Viewers**: layout (GDS layers, cells, ports, measuring), 3-D (extruded
  stack), set-up (the permittivity lightsim's own rasteriser produces on its
  own grid, mesh lines, sources, monitors, absorbing layers), run (progress,
  energy decay, live field slice), results (S-parameters, spectra, field maps,
  time series), history (checkpoints, layout and result diffs, review
  comments), and quantum circuits with lightsim-quantum (HOM dips, output
  distributions, transfer matrices). WebGL2 rendering keeps large layouts
  interactive.
- **Simulation anywhere**: the CPU engine locally, NVIDIA GPUs on Linux (one
  or several), and GPU servers over SSH — design on a laptop, simulate on an
  RTX PRO 6000 or a GB10, with progress and field slices streamed back.
  Device, memory and time estimates before every run.
- **Built for agents**: an MCP server with 32 tools (inspect layout and
  stack, place and route gdsfactory components, set up and run simulations,
  read results, design-rule and consistency checks, render any view as an
  image), a CLI with the same verbs, skills for gdsfactory and lightsim, a
  design history with a checkpoint at the end of every agent turn, and
  click-to-reference: select a port or monitor and ask about it.
- **One hub per machine** owns the event log, file watching, git, jobs and
  agent processes; the desktop window, browser tabs, the CLI and the MCP
  server are its clients.
- **Desktop app for macOS and Linux** (Windows to follow) with signed
  automatic updates, stable and beta channels, and rollback.

| | |
|---|---|
| ![Layout](docs/img/layout-dark.png) | ![3-D](docs/img/3d-light.png) |
| ![Set-up](docs/img/setup-light.png) | ![Run](docs/img/run-dark.png) |

## Status

Early: version 0.1.0, not yet released as binaries. It builds and is tested
on macOS (Apple silicon) and Linux (x86_64 and aarch64); Windows packaging is
configured but untested. For now, build gamma from source.

## Build from source

Requirements: [Rust](https://rustup.rs) (the pinned toolchain installs itself),
Node.js 20 or newer, git, and Python 3.10+ for project scripts. On Linux also
the WebKitGTK 4.1 and GTK 3 development packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev   # Debian, Ubuntu
sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel      # Fedora
```

Then:

```bash
git clone https://github.com/su3co/gamma && cd gamma
scripts/run-from-source.sh          # builds the interface, the gamma program and the app, then starts it
```

The `gamma` program on its own (hub, CLI, MCP server, simulation worker) needs
only Rust:

```bash
cargo build --release -p gamma-cli          # target/release/gamma (CPU)
scripts/build-cuda.sh                       # with NVIDIA GPU support (Linux, CUDA 13)
cargo test --release --workspace            # unit and end-to-end tests
```

Installers for your platform: `scripts/package.sh` (macOS `.dmg`; Linux
AppImage, `.deb`, `.rpm`) or `packaging/linux/build.sh` (Linux packages in a
Docker container). Signed releases with automatic updates are planned:
[docs/install.md](docs/install.md), [docs/release.md](docs/release.md).

## Quick start

```bash
git clone https://github.com/su3co/gamma && cd gamma        # if you have not already
python3 -m venv .venv && .venv/bin/pip install gdsfactory "lightsim @ git+https://github.com/su3co/lightsim" ./python
cargo build --release -p gamma-cli
cd examples/mmi-splitter
sed -i.bak 's|interpreter = "python3"|interpreter = "../../.venv/bin/python"|' gamma.toml
../../target/release/gamma build                 # GDS, ports and the lightsim spec from the scripts
../../target/release/gamma setup through --preview
../../target/release/gamma run through --preview --device cpu
../../target/release/gamma results through --what sparams
```

On an Apple M1 Pro the preview (1.7 M cells) takes about 50 s and gives
|S₂₁|² = |S₃₁|² = −3.13 dB at 1550 nm. Open the same folder in the desktop
app (or `gamma open` for a browser address) to see it. The full walk-through,
including the lightsim examples: [docs/quickstart.md](docs/quickstart.md).

## Start from any GDS file

```bash
gamma init my-chip --gds chip.gds --stack stack.toml --ports ports.csv
```

copies the layout into a new project; ports come from the CSV
(`name,x,y,width,orientation`) or from gdsfactory metadata in the file. See
[docs/quickstart.md](docs/quickstart.md#start-from-a-gds-file).

## Agents

```bash
gamma agent setup claude      # writes .mcp.json in the project; then run `claude` there
gamma agent setup codex       # prints the Codex configuration
```

Or use the conversation panel in the app, which runs the agent for you and
checkpoints every turn. Tools and skills: [docs/agents.md](docs/agents.md).

## GPU servers

```bash
gamma remote add gpu user@gpu-server            # your SSH keys, no passwords
gamma remote install gpu --binary gamma-linux-x86_64-cuda
gamma run through --host gpu --device cuda:0
```

A host can name a job-queue command that runs in front of the worker:
[docs/remote.md](docs/remote.md).

## Documentation

- [Architecture](docs/architecture.md): components, processes, data formats, why Tauri
- [Install and update](docs/install.md) · [Quick start](docs/quickstart.md) · [Agents](docs/agents.md) · [Remote GPU hosts](docs/remote.md)
- [Developing gamma](docs/development.md) · [Releasing](docs/release.md) · [Test runs](docs/test-runs/)

## Limitations

Quantum circuits run through the lightsim-quantum program when it is
installed (that library is not public yet); live field slices during
multi-GPU runs wait for lightsim's run hook. See [CHANGELOG.md](CHANGELOG.md).

## License

MIT; see [LICENSE](LICENSE). Bundled fonts (Archivo, Literata, IBM Plex Mono)
are under the SIL Open Font License (`assets/fonts/OFL-*.txt`).
