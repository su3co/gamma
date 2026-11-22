# Quick start

This walk-through opens the MMI example, looks at it, runs it on the CPU and
reads the S-parameters. It takes about five minutes on a laptop.

## 1. Python for the project's scripts

gamma projects describe layouts with gdsfactory and simulations with
lightsim's Python package; gamma runs those scripts to produce the files its
viewers read.

```bash
python3 -m venv ~/.venvs/gamma
~/.venvs/gamma/bin/pip install gdsfactory "lightsim @ git+https://github.com/su3co/lightsim" ./python   # from the gamma checkout
```

In `examples/mmi-splitter/gamma.toml`, point `[python] interpreter` at it
(`~/.venvs/gamma/bin/python`, or a path relative to the project).

## 2. Open the project

Start gamma and open `examples/mmi-splitter`, or from a terminal:

```bash
cd examples/mmi-splitter
gamma build          # runs layout/mmi.py and sims/through.py (full and preview variants)
gamma open           # prints the browser address of the same views
```

- **Layout**: the splitter, its three ports, a scale bar. Scroll to zoom,
  drag to pan, `M` to measure, click a polygon or port to select it.
- **3-D**: the 220 nm silicon core extruded from the stack.
- **Set-up**: the permittivity lightsim will use, on its own grid, with the
  mode source `src_o1`, the mode monitors `m_o1`–`m_o3`, the absorbing
  region and, when zoomed in, the mesh lines. The side panel gives cells,
  steps, memory and a time estimate for every device.

Edit `layout/mmi.py` (say `LENGTH_MMI = 6.0`) and save: gamma rebuilds and
every view updates.

## 3. Run

In Set-up, tick *preview mesh* and press *Run* next to `cpu`, or:

```bash
gamma run through --preview --device cpu
```

The Run view shows progress, the energy decay (the run stops when it falls
below 1e-5 of its peak) and the field in the core plane as it evolves.

## 4. Results

The Results view (or `gamma results through --what sparams`) shows
|S|² in dB over 1.50–1.60 µm, the values at 1550 nm, and |E|² in the core
plane. On an Apple M1 Pro the preview (1.7 M cells) took 49 s and gave
|S₂₁|² = |S₃₁|² = −3.13 dB, reflection −29.7 dB. The full mesh (4.6 M cells)
is a GPU job: 17 s of solver time on one GB10 ([remote.md](remote.md)).

## 5. Ask the agent

Type in the Agent panel, for example: *"Make the MMI 0.5 µm longer, rerun the
preview and compare the splitting with the previous checkpoint."* The agent
edits `layout/mmi.py`, the views follow, it runs the simulation, and the end
of its turn is a checkpoint you can compare in History.

## The lightsim examples

The worked examples published with lightsim (Mach–Zehnder modulator, ring
sensor, grating coupler, quantum circuits…) are gdsfactory designs with
lightsim simulation scripts. To open one in gamma, add a `gamma.toml` to a
copy of its folder: `[layout]` pointing at the example's GDS and a
`[[simulation]]` whose generator writes the spec with
`gamma_pic.write_sim(build(...), "name")` from the example's `simulate.py`
builder.
