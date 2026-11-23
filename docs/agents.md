# Agents in gamma

gamma is built so that an agent can do everything a person can, in the same
project, while the person watches. Three ways to connect one:

1. **The conversation panel** in the app runs Claude Code or Codex for you in
   the project folder with gamma's MCP server attached. Its messages and tool
   calls appear in the panel, its edits in the views, and the end of each
   turn is a checkpoint.
2. **Your own terminal session**: `gamma agent setup claude` writes the
   project's `.mcp.json` and installs gamma's skills into `.claude/skills/`;
   start `claude` in the project. `gamma agent setup codex` prints the Codex
   configuration and writes `AGENTS.md`.
3. **Any MCP client**: run `gamma mcp --project <folder>` as a stdio MCP
   server.

All three talk to the same hub, so the views follow whichever is working.

## Tools

| Tool | What it does |
|---|---|
| `project_info` | Manifest, layout summary, simulations, recent runs, the person's selection |
| `layout_summary`, `layout_query`, `layout_pick`, `layout_measure` | Cells, layers, ports, polygons in a box, what is at a point, gaps between polygons |
| `stack_get`, `stack_set` | The layer stack and materials |
| `place_component`, `route`, `expose_port`, `remove_component` | Place and route gdsfactory components in a YAML netlist, then rebuild |
| `build` | Regenerate GDS, ports and simulation specs from their scripts |
| `sim_list`, `sim_setup` | Simulations; grid, media, sources, monitors and estimates per device |
| `sim_run`, `job_status`, `job_cancel` | Run on the CPU, local GPUs or a remote host; follow and cancel |
| `results` | Overview, S-parameters, spectra, time series, decay, field statistics |
| `export_sparams` | Touchstone or Lumerical-text S-parameters next to a run's results |
| `check` | Design rules and consistency checks |
| `view_render` | Any view as an image: layout, 3d, setup, field, live, spectrum, sparams, decay, time |
| `selection_get` | What the person selected |
| `checkpoint_create`, `checkpoint_list`, `checkpoint_diff` | Design history, layout and result diffs |
| `comment_add` | Review comments anchored to objects and checkpoints |
| `remotes_list` | Remote hosts |
| `circuit_list`, `circuit_view`, `circuit_run`, `circuit_results` | Quantum circuits with lightsim-quantum |

## Skills

`skills/` holds the skills gamma installs for agents: `gamma` (the working
loop and the tools), `gdsfactory-pic` (layouts), `lightsim-fdtd`
(simulations), `lightsim-quantum` (circuits, provisional).

## Referring to objects

Click a port, polygon, monitor or run in any view and press *Ask the agent
about this*: the reference (`port:o2`, `monitor:through/m_o2`, …) and a
description go into your next message. References in the agent's replies are
clickable.

A recorded session: [test-runs/2026-11-22-agent-mcp.md](test-runs/2026-11-22-agent-mcp.md).
