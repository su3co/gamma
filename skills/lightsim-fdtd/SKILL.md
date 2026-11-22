---
name: lightsim-fdtd
description: Set up, run and read lightsim FDTD simulations of photonic components in a gamma project — specs from gdsfactory, mode sources and monitors, S-parameters, meshes, previews, GPUs. Use when writing or changing a simulation script or interpreting results.
---

# lightsim simulations in gamma

- A simulation script builds a `lightsim.Simulation` and ends with
  `gamma_pic.write_sim(sim, "<name>")`, which writes the JSON spec named in
  `gamma.toml`. gamma runs the spec anywhere (CPU, local GPU, remote GPU).
- From a gdsfactory component: `lightsim.sparams.build_simulation(component,
  wavelengths, steps_per_wavelength=...)` returns the simulation with the
  stack's structures; add a `ModeSource` named `src_<port>` just outside the
  input port and `ModeMonitor`s named `m_<port>` at every port (the input's
  monitor two cells downstream of the source). gamma then computes
  S-parameters like `lightsim.sparams.write_sparameters`.
- Mode profiles belong to the grid they were solved on. Scale the mesh with
  `gamma_pic.mesh_scale()` and the run time with
  `gamma_pic.run_time_scale()` so `[simulation.preview]` builds a coarse
  variant correctly.
- Resolution: 15–20 cells per wavelength in the densest medium for
  quantitative work, 10–12 for a preview. Keep sources and monitors at least
  half a wavelength from the absorbing layers.
- Set `shutoff` (1e-5 is typical): runs stop when the field energy decays.
  The run view and `results` `decay` show it.
- Throughput: an RTX PRO 6000 does about 18–27 Gcells/s, a GB10 about 2.4,
  a laptop CPU 0.1–0.3. `sim_setup` estimates per device from measured
  rates.
- Results: `results` `sparams` (dB, phase), `spectrum` per monitor, `field`
  for frequency-domain planes (`|E|2`, a component, `abs()`, `phase()`).
