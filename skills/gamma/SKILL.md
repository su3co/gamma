---
name: gamma
description: Work in a gamma photonic-circuit project through gamma's MCP tools — inspect layout and stack, build, set up and run lightsim simulations locally or on GPU servers, read results, check, render views, checkpoint. Use whenever the working folder has a gamma.toml.
---

# Working in a gamma project

A gamma project is a folder with `gamma.toml`. The person watches the layout,
3-D, set-up, run and results views update while you work, so act through the
project's files and gamma's tools, and look at what you made.

## The loop

1. `project_info` first: layout summary, simulations, recent runs, and what
   the person has selected (`selection_get` for details).
2. Edit sources, not outputs: layout scripts (`layout/*.py`), YAML netlists
   (`layout/*.pic.yml`, or `place_component` / `route`), `stack.toml`,
   simulation scripts (`sims/*.py`). Generated files live in `build/` and are
   rebuilt by `build` (the hub also rebuilds when sources change).
3. Look: `view_render` with `layout`, `3d`, `setup` (sim, axis, coord) after
   every geometric change. Measure gaps with `layout_measure`.
4. `check` before running: design rules, stale files, mesh resolution,
   sources and monitors outside the region, spec against layout.
5. `sim_setup` gives cells, steps, memory and time per device. Prefer
   `preview: true` or the CPU for small checks; state the estimate before a
   long run; put long runs on a GPU (`device: cuda:0`) or a remote host
   (`remotes_list`, then `sim_run` with `host`).
6. `results`: `overview`, `sparams`, `spectrum`, `field` (then `view_render`
   `field` to see the map), `decay`.
7. `checkpoint_create` with a message after a meaningful step (the end of
   each turn is checkpointed automatically). `checkpoint_diff` compares
   layouts and results.

## References

Objects are named the same way everywhere: `port:o1`, `layer:1/0`,
`poly:1/0#3`, `cell:mmi`, `sim:through`, `monitor:through/m_o2`,
`source:through/src_o1`, `point:1.2,-0.4` (µm). Use them in your replies; the
person can click them.

## Units

Layout and stack in µm; lightsim specs in SI (m, s, Hz). S-parameter names
follow gdsfactory: `o2@0,o1@0` is the transmission from o1 to o2.
