# Command and tool reference

The `gamma` program's commands and the tools of its MCP server, generated from their definitions in `crates/gamma-cli` (do not edit by hand: run `GAMMA_UPDATE_REFERENCE=1 cargo test -p gamma-cli reference`). How to connect an agent: [agents.md](agents.md).

## Command line

Every command that works on a project takes these options:

| Option | Meaning |
|---|---|
| `-C, --project <PROJECT>` | Project folder (default: the current folder or a parent with gamma.toml) |
| `--json` | Print JSON instead of text |

### `gamma hub`

Run the hub (one per machine and user)

```text
gamma hub [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `--port <PORT>` | Port on 127.0.0.1 (0: any free port) (default: `0`) |
| `--ui <UI>` | Folder of the built interface to serve to browsers |
| `--idle-exit-min <IDLE_EXIT_MIN>` | Exit after this many idle minutes (0: never) (default: `0`) |
| `--print-url` | Print the browser address and keep running even if a hub is already up |

### `gamma mcp`

MCP server for agents on standard input/output

```text
gamma mcp
```

### `gamma worker`

Run one simulation and stream progress as JSON lines (used by the hub, locally or on a remote host)

```text
gamma worker <COMMAND>
```

#### `gamma worker run`

Run a lightsim spec

```text
gamma worker run <SPEC> [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<SPEC>` | lightsim JSON spec |
| `--device <DEVICE>` | cpu, cuda:0 or cuda:0,1 (default: `cpu`) |
| `--out <OUT>` | Results file (default: results.json next to the spec) |
| `--slice <SLICE>` | auto, none, or COMPONENT,AXIS,COORDINATE (default: `auto`) |
| `--every <EVERY>` | Seconds between progress messages (default: `1`) |

#### `gamma worker info`

Version and devices of this machine

```text
gamma worker info
```

### `gamma init`

Create a project in a folder (empty, or around a GDS file from any design tool)

```text
gamma init [<DIR>] [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<DIR>` | Project folder (default: the current folder) |
| `--name <NAME>` | Project name (default: the folder or GDS file name) |
| `--gds <GDS>` | Start from this GDS file (copied into layout/) |
| `--stack <STACK>` | Layer stack (stack.toml); default: generic 220 nm SOI |
| `--ports <PORTS>` | Ports (JSON or CSV: name,x,y,width,orientation,layer,datatype); default: from the GDS metadata if any |

### `gamma export`

Export a run's S-parameters (Touchstone .sNp or Lumerical INTERCONNECT .dat)

```text
gamma export <SIM> [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<SIM>` | Simulation name |
| `--run <RUN>` | Run id (default: the latest finished run) |
| `--format <FORMAT>` | touchstone or lumerical (default: `touchstone`) |
| `-o, --out <OUT>` | Also copy the file here |

### `gamma open`

Open a project in the hub and print its id and address

```text
gamma open [<DIR>]
```

| Argument | Meaning |
|---|---|
| `<DIR>` | Project folder (default: the current folder) |

### `gamma info`

Project overview

```text
gamma info
```

### `gamma build`

Regenerate files from their sources (all, layout, sim:<name>)

```text
gamma build [<TARGET>]
```

| Argument | Meaning |
|---|---|
| `<TARGET>` | all, layout or sim:<name> (default: `all`) |

### `gamma setup`

What a simulation describes, the grid and run-time estimates

```text
gamma setup <SIM> [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<SIM>` | Simulation name |
| `--preview` | The coarse preview variant |

### `gamma run`

Run a simulation

```text
gamma run <SIM> [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<SIM>` | Simulation name |
| `--device <DEVICE>` | cpu, cuda:0, cuda:0,1 |
| `--host <HOST>` | Remote host name |
| `--preview` | Run the coarse preview variant |
| `--detach` | Return at once instead of following the job |

### `gamma jobs`

List jobs, or follow/cancel one

```text
gamma jobs [<ID>] [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<ID>` | Job id: follow it (default: list all jobs) |
| `--cancel` | Cancel the job |

### `gamma results`

Results of a run (latest by default)

```text
gamma results <SIM> [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<SIM>` | Simulation name |
| `--run <RUN>` | Run id (default: the latest finished run) |
| `--what <WHAT>` | overview, sparams, spectrum, field, time, decay or farfield (default: `overview`) |
| `--monitor <MONITOR>` | Monitor (for spectrum, field, time and farfield) |

### `gamma check`

Design-rule and consistency checks

```text
gamma check
```

### `gamma view`

Render a viewer to PNG: gamma view layout -o out.png [key=value ...]

```text
gamma view <VIEW> [<PARAMS>] [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<VIEW>` | layout, 3d, setup, field, live, spectrum, sparams, decay or time |
| `-o, --out <OUT>` | PNG file to write |
| `<PARAMS>` | View parameters as key=value (sim, run, monitor, quantity, f, axis, coord, preview, job, highlight, window, theme, width, height) |

### `gamma checkpoint`

Create a checkpoint, or list them

```text
gamma checkpoint [<MESSAGE>] [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<MESSAGE>` | Checkpoint message |
| `--list` | List the checkpoints instead |

### `gamma diff`

Compare two checkpoints (or one with the current state)

```text
gamma diff <A> [<B>]
```

| Argument | Meaning |
|---|---|
| `<A>` | Checkpoint number |
| `<B>` | Second checkpoint (default: the current state) |

### `gamma remote`

Remote simulation hosts

```text
gamma remote <COMMAND>
```

#### `gamma remote add`

Add (or replace) a host

```text
gamma remote add <NAME> <SSH> [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<NAME>` | Name to refer to the host by |
| `<SSH>` | user@host or an SSH config alias |
| `--launcher <LAUNCHER>` | Command prefix for a job queue; with {host} it runs locally and reaches the host itself |
| `--program <PROGRAM>` | Path of gamma on the host |
| `--renumber-gpus` | The launcher exposes only the GPUs it grants (they are cuda:0.. inside the job) |

#### `gamma remote list`

List the hosts

```text
gamma remote list
```

#### `gamma remote remove`

Remove a host

```text
gamma remote remove <NAME>
```

| Argument | Meaning |
|---|---|
| `<NAME>` | Host name |

#### `gamma remote test`

Check the connection, the installed gamma and its GPUs

```text
gamma remote test <NAME>
```

| Argument | Meaning |
|---|---|
| `<NAME>` | Host name |

#### `gamma remote install`

Copy a gamma build for the host's platform to the host

```text
gamma remote install <NAME> [OPTIONS]
```

| Argument | Meaning |
|---|---|
| `<NAME>` | Host name |
| `--binary <BINARY>` | gamma program built for the host (e.g. gamma-linux-x86_64-cuda) |

### `gamma agent`

Connect an agent (Claude Code, Codex) to this project

```text
gamma agent <COMMAND>
```

#### `gamma agent setup`

Write the MCP configuration an agent CLI reads in this project (.mcp.json for Claude Code; prints the Codex command)

```text
gamma agent setup [<PROVIDER>]
```

| Argument | Meaning |
|---|---|
| `<PROVIDER>` | claude or codex (default: `claude`) |

#### `gamma agent send`

Send a message to the in-app agent session

```text
gamma agent send <TEXT>
```

| Argument | Meaning |
|---|---|
| `<TEXT>` | The message |

### `gamma stop`

Stop the hub

```text
gamma stop
```

## MCP tools

`gamma mcp` serves 31 tools to an agent over standard input and output (JSON-RPC, MCP). Each forwards to the hub, so the person sees the agent's actions as it makes them. Lengths are in µm unless a description says otherwise.

### `project_info`

Overview of the open gamma project: manifest, layout summary (cells, layers, ports, size), simulations, latest runs, circuits, checkpoints and what the person has selected.

No parameters.

### `layout_summary`

Cells, layers (with stack names, materials, z range, polygon counts), ports (position, width, orientation) and bounding box of the layout. Units: um.

No parameters.

### `layout_query`

Polygons of one layer inside a box (um): references, owning cell, bounding boxes, vertex counts, areas.

| Parameter | Type | Meaning |
|---|---|---|
| `layer` | string, required | GDS layer/datatype, e.g. 1/0 |
| `limit` | number | max polygons (default 200) |
| `x0` | number | um |
| `x1` | number | um |
| `y0` | number | um |
| `y1` | number | um |

### `layout_pick`

What is at a point of the layout (um): a port, else the top-most polygon, with a description.

| Parameter | Type | Meaning |
|---|---|---|
| `tol` | number | tolerance um (default 0.05) |
| `x` | number, required | um |
| `y` | number, required | um |

### `layout_measure`

Edge-to-edge distance between two polygons given as references (poly:1/0#3), e.g. a coupling gap.

| Parameter | Type | Meaning |
|---|---|---|
| `a` | string, required | poly:L/D#i |
| `b` | string, required | poly:L/D#i |

### `stack_get`

The layer stack: layers (GDS layer, z range, material, colour, design rules) and materials (index, lightsim model).

No parameters.

### `stack_set`

Replace the layer stack (same structure as stack_get returns). Prefer editing stack.toml for small changes.

| Parameter | Type | Meaning |
|---|---|---|
| `stack` | object, required | the whole stack as stack_get returns it: layers (name, gds, zmin, thickness, material, ...) and materials |

### `place_component`

Place (or replace) a gdsfactory component instance in a YAML netlist (layout/*.pic.yml), then rebuild. Example: component mmi1x2, settings {length_mmi: 5.5}.

| Parameter | Type | Meaning |
|---|---|---|
| `component` | string, required | gdsfactory cell name |
| `mirror` | boolean | mirror |
| `name` | string, required | instance name |
| `netlist` | string | netlist file, default layout/top.pic.yml |
| `rotation` | number | degrees |
| `settings` | object | parameters of the cell, e.g. {"length": 10} |
| `x` | number | um |
| `y` | number | um |

### `route`

Route between two instance ports in a YAML netlist with a gdsfactory router (routes section), then rebuild. Ports as instance,port (e.g. mmi,o2).

| Parameter | Type | Meaning |
|---|---|---|
| `bundle` | string | route bundle name (default r_<from>) |
| `cross_section` | string | e.g. strip |
| `from` | string, required | instance,port |
| `netlist` | string | netlist file, default layout/top.pic.yml |
| `to` | string, required | instance,port |

### `remove_component`

Remove an instance (and its connections) from a YAML netlist, then rebuild.

| Parameter | Type | Meaning |
|---|---|---|
| `name` | string, required | instance name |
| `netlist` | string | netlist file |

### `expose_port`

Make an instance port a port of the top cell in a YAML netlist.

| Parameter | Type | Meaning |
|---|---|---|
| `instance_port` | string, required | instance,port |
| `netlist` | string | netlist file |
| `port` | string, required | top-level port name |

### `build`

Regenerate files from their sources: the GDS and ports from the layout generator, and simulation specs from their generators. target: all, layout or sim:<name>. Returns the scripts' output.

| Parameter | Type | Meaning |
|---|---|---|
| `target` | string | all \| layout \| sim:<name> |

### `sim_list`

Simulations of the project (name, spec file, generator, built or not).

No parameters.

### `sim_setup`

What a simulation describes as lightsim builds it: grid (cells, steps, time step, memory), media, structures, sources, monitors, boundaries, wavelength range, and run-time estimates per device. preview: the coarse variant.

| Parameter | Type | Meaning |
|---|---|---|
| `preview` | boolean | coarse preview variant |
| `sim` | string, required | simulation name |

### `sim_run`

Start a simulation on a device: cpu, cuda:0, cuda:0,1 (this machine) or on a remote host (host + device). Returns the job; wait_s waits up to that many seconds for it to finish.

| Parameter | Type | Meaning |
|---|---|---|
| `device` | string | cpu \| cuda:0 \| cuda:0,1 |
| `host` | string | remote host name |
| `preview` | boolean | run the coarse preview |
| `sim` | string, required | simulation name |
| `wait_s` | number | seconds to wait (0 = return at once) |

### `job_status`

Status, plan, progress (step, energy decay, throughput) and log of a job; without job: all jobs.

| Parameter | Type | Meaning |
|---|---|---|
| `job` | string | job id |

### `job_cancel`

Cancel a queued or running job (a running job keeps the results so far).

| Parameter | Type | Meaning |
|---|---|---|
| `job` | string, required | job id |

### `results`

Results of a run (latest finished run by default): overview (stats, monitors, S-parameters, metrics), or a monitor's spectrum, time series, energy decay or field-map statistics.

| Parameter | Type | Meaning |
|---|---|---|
| `f` | number | frequency index |
| `monitor` | string | monitor name |
| `quantity` | string | for field: \|E\|2, Ey, abs(Ey), phase(Ey) |
| `run` | string | run id |
| `sim` | string, required | simulation |
| `what` | string | overview \| spectrum \| sparams \| time \| decay \| field |

### `check`

Design-rule check (min width and spacing per layer from the stack) and consistency checks (stale or missing generated files, unmapped layers, mesh resolution, absorbing layers, sources and monitors outside the region, spec vs layout).

No parameters.

### `view_render`

Render a viewer to an image to look at it: layout (highlight=refs separated by ;, window=x0,y0,x1,y1), 3d, setup (sim, axis, coord, preview), field (sim, monitor, quantity, f, run), live (job), spectrum, sparams, decay, time.

| Parameter | Type | Meaning |
|---|---|---|
| `axis` | number | 0, 1, 2 |
| `coord` | number | m |
| `f` | number | frequency index |
| `height` | number | px |
| `highlight` | string | references to highlight, separated by ; |
| `job` | string | job id (view live) |
| `monitor` | string | monitor name |
| `preview` | boolean | the coarse preview variant |
| `quantity` | string | field quantity, e.g. \|E\|2, Ey (default \|E\|2) |
| `run` | string | run id (default: the latest) |
| `sim` | string | simulation name |
| `theme` | string | dark \| light |
| `view` | string, required | layout \| 3d \| setup \| field \| live \| spectrum \| sparams \| decay \| time |
| `width` | number | px |
| `window` | string | x0,y0,x1,y1 in um |

### `selection_get`

The object the person selected in a viewer (reference and description), if any.

No parameters.

### `checkpoint_create`

Commit the project and tag it as a checkpoint with a message (gamma also checkpoints at the end of every agent turn).

| Parameter | Type | Meaning |
|---|---|---|
| `message` | string | what changed |

### `checkpoint_list`

Checkpoints (number, commit, time, actor, message) and review comments.

No parameters.

### `checkpoint_diff`

Compare checkpoint a with b (or the current state): files changed, layout diff (polygons added/removed per layer, ports moved) and result diff (metrics of the latest runs).

| Parameter | Type | Meaning |
|---|---|---|
| `a` | number, required | checkpoint |
| `b` | number | checkpoint (default: current state) |

### `comment_add`

Add a review comment, optionally anchored to an object reference and a checkpoint.

| Parameter | Type | Meaning |
|---|---|---|
| `checkpoint` | number | checkpoint number it refers to (default: the current state) |
| `object` | string | e.g. port:o1 |
| `text` | string, required | the comment |

### `remotes_list`

Remote simulation hosts known to gamma.

No parameters.

### `export_sparams`

Write a run's S-parameters to a file in the run's folder, in a format other tools read: Touchstone (.sNp, exp(+jwt), as lightsim writes it) or Lumerical INTERCONNECT text (.dat, exp(-iwt)). Returns the path.

| Parameter | Type | Meaning |
|---|---|---|
| `format` | string | file format (default touchstone) (`touchstone`, `lumerical`) |
| `run` | string | run id (default: the latest finished run) |
| `sim` | string, required | simulation name |

### `circuit_list`

Quantum circuits of the project (lightsim-quantum) and whether the engine is installed.

No parameters.

### `circuit_view`

One quantum circuit: its components, connections, external ports, inputs and outputs, frequencies and tasks.

| Parameter | Type | Meaning |
|---|---|---|
| `name` | string, required | circuit name |

### `circuit_run`

Run a quantum circuit with the lightsim-quantum engine (when installed) and wait for it; returns the run id and a summary of the results (plots, bar charts, tables).

| Parameter | Type | Meaning |
|---|---|---|
| `name` | string, required | circuit name |

### `circuit_results`

Results of a quantum circuit's latest run, summarised as plots, bar charts and tables (HOM dips, output distributions, |T|^2, click statistics).

| Parameter | Type | Meaning |
|---|---|---|
| `name` | string, required | circuit name |
