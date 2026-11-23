"""S-parameter simulation of the layout, set up by lightsim's tool-neutral
layout layer: the GDS file, the layer stack (stack.toml) and the port file,
with no design tool needed. gamma runs this script to write the simulation
spec; edit the settings below and gamma rebuilds it.

Requires the lightsim Python package (`lightsim.layout`).
See https://developer.su3labs.com/lightsim/docs/guide/layout-tools/
"""

import json
import os
from pathlib import Path

import lightsim.layout as ll

ROOT = Path(os.environ.get("GAMMA_PROJECT") or Path(__file__).resolve().parents[1])

GDS = "{gds}"
CELL = "{cell}"
STACK = "stack.toml"
PORTS = "{ports}"
SOURCE = "{source}"  # the port (or port@mode) the mode source launches into
SPEC = "{spec}"

SETTINGS = dict(
    is_3d=True,
    steps_per_wavelength=20.0,  # graded mesh: steps per wavelength in each material
    margin=1.0,  # um between the layout and the absorber in x and y
    z_margin=0.8,  # um above and below the stack
)

# gamma builds a coarse preview variant with these set; mode profiles belong
# to the grid they were solved on, so the preview is set up afresh.
mesh_scale = float(os.environ.get("GAMMA_PREVIEW_MESH_SCALE", "1"))
run_time_scale = float(os.environ.get("GAMMA_PREVIEW_RUN_TIME_SCALE", "1"))
if mesh_scale != 1:
    SETTINGS["steps_per_wavelength"] = SETTINGS["steps_per_wavelength"] / mesh_scale

sims = ll.simulation(ROOT / GDS, ROOT / STACK, ROOT / PORTS, cell=CELL, sources=[SOURCE], **SETTINGS)
if not sims:
    raise SystemExit(f"no port {SOURCE!r} in {PORTS}")
spec = json.loads(next(iter(sims.values())).to_json())
if run_time_scale != 1 and isinstance(spec.get("run_time"), (int, float)):
    spec["run_time"] *= run_time_scale

out = ROOT / os.environ.get("GAMMA_SPEC_OUT", SPEC)
out.parent.mkdir(parents=True, exist_ok=True)
tmp = out.with_name(f".{out.name}.tmp-{os.getpid()}")
tmp.write_text(json.dumps(spec))
os.replace(tmp, out)
print(f"wrote {out.relative_to(ROOT)} (source {SOURCE})")
