# Changelog

All notable changes to gamma. Versions follow semantic versioning; each
section is copied into the update feed and the release notes.

## [Unreleased]

- `docs/reference.md`: every `gamma` command and MCP tool with its
  parameters, generated from the program (a test keeps it current).
- The MCP server lists `export_sparams`, `circuit_view`, `circuit_run` and
  `circuit_results` in `tools/list` (they were answered but not listed, so
  agents did not see them); a test checks that every answered tool is listed.
- lightsim pinned at 849fd1a (the release with the tool-neutral layout layer,
  stabilised absorbing layers and the passive `Absorber` boundary).
- `gamma init --gds` adds an S-parameter simulation, `sims/sparams.py`, set
  up by lightsim's tool-neutral layout layer (`lightsim.layout.simulation`)
  from the GDS, `stack.toml` and the port file; ports read from gdsfactory
  metadata are written as `layout/ports.json`.
- Touchstone and INTERCONNECT files are laid out as lightsim's writers lay
  them out (`! port k = name@mode` comments, GHz, single-quoted block headers,
  unwrapped phase), so lightsim's readers read them.
- Touchstone files are written in exp(+jωt), the complex conjugate of
  lightsim's exp(-iωt) data, matching lightsim's own Touchstone writer; the
  header states it. INTERCONNECT text is unchanged.
- Projects from a plain GDS file of any design tool with a stack and a
  ports file (CSV or JSON): `gamma init --gds`.
- S-parameter export to Touchstone (`.sNp`) and Lumerical INTERCONNECT text.
- A solver per simulation (`solver`, default `fdtd`), ready for lightsim's
  coming solvers.
- Remote hosts whose job queue renumbers GPUs (`renumber_gpus`, `cuda:any`).
- Quantum circuits: lightsim-quantum documents are read, run through the
  lightsim-quantum program with progress, and their results plotted (HOM
  dips, distributions, transfer matrices, click statistics).

## [0.1.1] - test feed only

- Test release used to exercise the update flow (download, signature check,
  install, restart, rollback). Not published.

## [0.1.0] - unreleased

- First release candidate: project workspace, layout, 3-D, set-up, run,
  results, circuit and history views; hub with event log, file watching, git
  checkpoints, local and SSH jobs and agent sessions; MCP server and CLI;
  lightsim CPU and CUDA runs with live field slices; macOS and Linux packages
  with signed automatic updates.
