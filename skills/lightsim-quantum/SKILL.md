---
name: lightsim-quantum
description: Circuits of quantum light (single photons, squeezed light, linear optics, detectors) with lightsim-quantum in a gamma project. Use when the project lists [[circuit]] entries or the person asks about HOM interference, boson sampling, heralding or photon statistics.
---

# lightsim-quantum in gamma

A circuit is a lightsim-quantum document (JSON, `"format":
"lightsim-quantum/1"`) named under `[[circuit]]` in `gamma.toml`. It holds:

- `circuit`: `kind: netlist` with `instances` (name → `{"model": ..., params}`),
  `connections` (`[["a.out0", "b.in"]]`), `ports`, `inputs`, `outputs`; or
  `kind: modes` with `elements` placed on numbered modes; or `kind: matrix`;
- `wavelength(s)` or `frequency(ies)`;
- `tasks`: `transfer`, `distribution`, `fock`, `hom`, `spectral`,
  `gaussian`, `gate`, `ring_source`;
- optionally `sweep`: `{"pointer": "/json/pointer", "values": [...]}`.

Components can be built-in models (beam_splitter, phase_shifter, waveguide,
delay, loss, mzi, crossing, rings, transfer, s_matrix, table) or a `file`
model reading S-parameters, so a lightsim FDTD result exported with
`export_sparams` (Touchstone) can become a component.

Tools: `circuit_list`, `circuit_view`, `circuit_run` (needs the
lightsim-quantum program installed), `circuit_results`. The Circuit view
shows the same plots: HOM dips against the swept delay, output-pattern
distributions, |T|² of transfer matrices, click statistics.
