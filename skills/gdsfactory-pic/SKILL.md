---
name: gdsfactory-pic
description: Design photonic layouts with gdsfactory in a gamma project — components, cross-sections, routing, YAML netlists, ports — and write them where gamma's viewers read them. Use when creating or changing a layout script or netlist.
---

# Layouts with gdsfactory in gamma

- Activate a PDK first: `gf.gpdk.PDK.activate()` (generic PDK: waveguide core
  on layer 1/0, 220 nm silicon) unless the project names another.
- End a layout script with `gamma_pic.write_layout(component)`: it writes the
  GDS and a ports file to the paths in `gamma.toml` (`[layout] gds`, `ports`)
  atomically, so the viewers never read a half-written file.
- Keep parameters as named constants at the top of the script; the person
  edits them, and the hub rebuilds on save.
- For assemblies, prefer a YAML netlist (`layout/top.pic.yml`) built with
  `gamma_pic.build_netlist("layout/top.pic.yml")`: instances, placements,
  routes (`routes: {r1: {links: {mmi,o2: s1,o1}}}`) and top-level ports. The
  MCP tools `place_component`, `route`, `expose_port` and `remove_component`
  edit it for you.
- Ports: name them o1, o2… counter-clockwise from the west (gdsfactory
  convention); simulations find their planes from them.
- Every patterned layer needs an entry in `stack.toml` (GDS layer, z range,
  material, design rules) for the 3-D view, the design-rule check and
  simulations.
- After a change: `build`, `view_render` (layout), `check`.
