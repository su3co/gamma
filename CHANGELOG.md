# Changelog

All notable changes to gamma. Versions follow semantic versioning; each
section is copied into the update feed and the release notes.

## [Unreleased]

- Projects from a plain GDS file of any design tool with a stack and a
  ports file (CSV or JSON): `gamma init --gds`.
- S-parameter export to Touchstone (`.sNp`) and Lumerical INTERCONNECT text.
- A solver per simulation (`solver`, default `fdtd`), ready for lightsim's
  coming solvers.
- Remote hosts whose job queue renumbers GPUs (`renumber_gpus`, `cuda:any`).

## [0.1.1] - test feed only

- Test release used to exercise the update flow (download, signature check,
  install, restart, rollback). Not published.

## [0.1.0] - unreleased

- First release candidate: project workspace, layout, 3-D, set-up, run,
  results, circuit and history views; hub with event log, file watching, git
  checkpoints, local and SSH jobs and agent sessions; MCP server and CLI;
  lightsim CPU and CUDA runs with live field slices; macOS and Linux packages
  with signed automatic updates.
