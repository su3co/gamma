# Remote GPU hosts

Design on a laptop, simulate on a GPU server: gamma sends the simulation
spec over SSH, runs `gamma worker` on the host, streams progress and field
slices back, and copies the results into the project.

## Requirements

- SSH access with keys (or an agent): gamma uses `ssh` in batch mode and
  never asks for passwords.
- The `gamma` program on the host, built with CUDA for the host's
  architecture (the release has `gamma-linux-x86_64-cuda` and
  `gamma-linux-aarch64-cuda`; or `scripts/build-cuda.sh` on the host).

## Adding a host

```bash
gamma remote add gpu user@gpu-server
gamma remote install gpu --binary gamma-linux-x86_64-cuda   # copies it to ~/.local/share/gamma/bin/gamma
gamma remote test gpu                                       # architecture, gamma version, GPUs
```

Or in the app: Settings → Remote simulation hosts. Hosts are stored in your
user configuration (`gamma.toml` of a project can add its own `[[remote]]`
entries too).

| Field | Meaning |
|---|---|
| `name` | What you call the host in gamma |
| `ssh` | `user@host` or an alias from `~/.ssh/config` |
| `program` | Path of gamma on the host (default `~/.local/share/gamma/bin/gamma`) |
| `workdir` | Where specs and results are staged (default `~/.cache/gamma/jobs`) |
| `launcher` | Optional command put in front of the worker: a job queue |

### Job queues

A launcher without `{host}` runs **on the host**, inside the SSH command, for
example a Slurm allocation:

```bash
gamma remote add cluster me@login --launcher "srun --gres=gpu:1 --time=60"
```

A launcher with `{host}` runs **on your computer** and reaches the host
itself, for queue helpers that take a host name; `{gpus}` and `{label}` are
filled in:

```bash
gamma remote add gpu gpu-server --launcher "my-queue run {host} --gpus {gpus} --label {label} --"
```

The worker command is handed to it as `bash -c '<worker command>'`. Exit code
75 from a launcher is reported as "host busy".

## Running

```bash
gamma run through --host gpu --device cuda:0        # or cuda:0,1 for two GPUs
```

In the app, every known host appears in the Set-up view's *Run on* table.
Jobs queue per host (one at a time); the Run view shows the queue position,
progress, decay and the live field. Results land in
`results/<sim>/<run>/` with the host and device recorded in `run.json`.
Cancelling stops the remote worker.

## Measured

| Host GPU | Simulation | Cells | Rate |
|---|---|---|---|
| GB10 (aarch64) | MMI splitter, full mesh | 4.6 M | 2.39 Gcells/s, 17.4 s solver time |

Records: [test-runs/](test-runs/).
