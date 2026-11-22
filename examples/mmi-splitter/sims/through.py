"""3-D FDTD of the MMI: fundamental TE mode launched into o1, mode monitors at
every port. gamma reads the S-parameters from the mode monitors (o2@0,o1@0 is
the transmission to o2).

The simulation is built with lightsim's gdsfactory import (the core layer on
220 nm of silicon in oxide), its mode solver for the port profiles, and
written as a lightsim JSON spec for gamma to run anywhere.
"""

import sys
from pathlib import Path

import numpy as np

import lightsim as ls
from lightsim.sparams import build_simulation
from lightsim.spec import C0, GaussianPulse, ModeMonitor, ModeSource, um

from gamma_pic import mesh_scale, run_time_scale, write_sim

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "layout"))
import mmi as layout  # noqa: E402

WAVELENGTHS = np.linspace(1.50e-6, 1.60e-6, 21)
STEPS_PER_WAVELENGTH = 12  # 20 for publication-grade numbers
PORT_OFFSET = 0.5 * um  # monitors and source inside the extended waveguides
HALF_WIDTH = 1.0 * um  # lateral half-size of the port planes
Z0, Z1 = -0.6 * um, 0.82 * um


def plane(port, offset):
    """Plane normal to the port's waveguide, `offset` inside the extended
    waveguide (outside the device), with the direction pointing into the device."""
    x, y, ang = port["center"][0], port["center"][1], port["orientation"] % 360  # metres
    if ang in (0, 180):
        s = 1 if ang == 0 else -1
        pos = x + s * offset
        return 0, pos, ("-" if s > 0 else "+"), [pos, y - HALF_WIDTH, Z0], [pos, y + HALF_WIDTH, Z1]
    s = 1 if ang == 90 else -1
    pos = y + s * offset
    return 1, pos, ("-" if s > 0 else "+"), [x - HALF_WIDTH, pos, Z0], [x + HALF_WIDTH, pos, Z1]


def main():
    c = layout.mmi()
    sim, ports, _ = build_simulation(c, WAVELENGTHS, steps_per_wavelength=STEPS_PER_WAVELENGTH / mesh_scale(), zmin=Z0 - 0.2 * um, zmax=Z1 + 0.2 * um)
    sim.name = "mmi_through"
    sim.run_time *= run_time_scale()
    freqs = C0 / WAVELENGTHS
    f0 = float(np.mean(freqs))
    profiles = {}
    for p in ports:
        axis, pos, d, mn, mx = plane(p, PORT_OFFSET)
        m = ls.solve_modes(sim, axis, pos, mn, mx, f0, num_modes=1)[0]
        profiles[p["name"]] = (axis, pos, d, mn, mx, m.profile)
    axis, pos, d, mn, mx, prof = profiles["o1"]
    sim.add_source(ModeSource(axis, pos, d, mn, mx, prof, GaussianPulse(f0, float(np.ptp(freqs))), name="src_o1"))
    for name, (axis, pos, d, mn, mx, prof) in profiles.items():
        if name == "o1":
            # just downstream of the source: incident and reflected modes
            step = 2 * float(np.diff(sim.grid()[axis]).max())
            pos = pos + (step if d == "+" else -step)
            mn, mx = list(mn), list(mx)
            mn[axis] = mx[axis] = pos
        sim.add_monitor(ModeMonitor(f"m_{name}", axis, pos, mn, mx, list(freqs), [prof]))
    # the field in the core plane at the centre wavelength, for the field view
    lo, hi = sim.min, sim.max
    sim.add_monitor(ls.DFTMonitor("core", [lo[0], lo[1], 0.11 * um], [hi[0], hi[1], 0.11 * um], [f0], components=("Ex", "Ey", "Ez")))
    write_sim(sim, "through")


if __name__ == "__main__":
    main()
