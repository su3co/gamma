# Developing gamma

## Layout of the repository

| Path | What |
|---|---|
| `crates/gamma-core` | Project model, GDS reader, geometry, layout queries and diff, YAML netlists, checks |
| `crates/gamma-sim` | lightsim integration: set-up slices, estimates, live runner, results, worker stream protocol |
| `crates/gamma-quantum` | lightsim-quantum integration point |
| `crates/gamma-render` | Headless PNG rendering (agents, CLI) |
| `crates/gamma-hub` | The hub: API, event log, watcher, git, jobs, remote hosts, agents |
| `crates/gamma-cli` | The `gamma` program: hub, MCP server, worker, project commands |
| `app/src-tauri` | Desktop shell: window, hub start-up, updates and rollback |
| `app/ui` | Interface (TypeScript, WebGL2), served by the shell and by the hub |
| `python/gamma_pic` | Helper for project scripts |
| `skills/` | Skills installed for agents |
| `examples/` | Example projects |
| `tests/` | Interface smoke test and fixtures |
| `packaging/` | Linux build container, repositories, update feeds |
| `scripts/` | Build, package, update and GPU tests |

The design is in [architecture.md](architecture.md).

## Everyday commands

```bash
cargo test --release --workspace                       # unit and end-to-end tests (spawns a hub and workers)
cargo clippy --release --workspace --all-targets -- -D warnings
cargo fmt --all
(cd app/ui && npm ci && npm run dev)                   # interface with hot reload (point it at a hub with ?token=)
cargo build --release -p gamma-cli && target/release/gamma hub --print-url   # hub serving app/ui/dist
node tests/ui/smoke.mjs                                # headless interface test, screenshots in target/ui-smoke
(cd app && ui/node_modules/.bin/tauri dev)             # the desktop shell against the dev server
```

Use separate hubs for development with `GAMMA_RUNTIME_DIR`, `GAMMA_DATA_DIR`
and `GAMMA_CONFIG_DIR` pointing at scratch folders.

## lightsim

gamma depends on lightsim by git revision (`Cargo.toml`, `[workspace.dependencies]`).
To move to a newer lightsim, change the revision there and `LIGHTSIM_REV` in
`crates/gamma-sim/src/lib.rs`, and run the tests; `runner.rs` steps lightsim's
backend itself and its test compares against `lightsim_core::run_cpu`
bit for bit. When lightsim publishes its run hook (live slices in the
multi-slab driver), `runner.rs` can use it for multi-GPU runs too.

## Testing on GPUs

`scripts/gpu-test.sh` on a machine with NVIDIA GPUs (after
`scripts/build-cuda.sh`) compares every GPU with the CPU on the fixture and
measures throughput; `.github/workflows/gpu.yml` runs it on self-hosted
runners once they exist. Results go to `docs/test-runs/`.

## Conventions

- Rust for everything that computes; TypeScript only for the interface.
- Files gamma writes are written atomically (`gamma_core::fsutil`).
- Every interface action is a hub call, so the CLI and MCP get it too.
- Commit messages describe the change; no machine names, user names or
  local paths in code, docs, test records or screenshots.
