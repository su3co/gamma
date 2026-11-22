---
name: lightsim-quantum
description: Circuits of quantum light (squeezed and single photons, linear optics, detectors) with lightsim-quantum in a gamma project. Use when the project lists [[circuit]] entries.
---

# lightsim-quantum in gamma

lightsim-quantum is being written; its interface is not final. Today gamma
lists the circuits a project names (`circuit_list`) and reads their
components and connections. When the library is installed, gamma runs a
circuit with its command-line program. Until then, describe circuits in a
JSON file with `name`, `components` (`[{id, type}]`) and `connections`
(`[["a.o1", "b.i1"]]`), and take the linear-optics part (beam-splitter
ratios, phases, losses) from lightsim S-parameters of the components.
