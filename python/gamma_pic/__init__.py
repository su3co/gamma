"""Helpers for gamma project scripts.

A gamma project keeps its layout and simulations as scripts; gamma runs them
to regenerate the files its viewers read. These helpers put the output where
``gamma.toml`` says, so a script can stay short::

    import gdsfactory as gf
    from gamma_pic import write_layout

    c = gf.components.mmi1x2()
    write_layout(c)                      # build/layout.gds + build/layout.ports.json

    from gamma_pic import write_sim
    write_sim(sim, "through")            # build/sims/through.json (a lightsim Simulation)

The project folder is taken from ``GAMMA_PROJECT`` (set by the hub when it
runs a script) or found by walking up from the working directory.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

__all__ = ["mesh_scale", "is_preview", "project_root", "manifest", "write_layout", "write_ports", "write_sim", "build_netlist", "ports_of"]


def mesh_scale() -> float:
    """Factor by which a simulation script should coarsen its mesh: 1 for the
    full simulation, larger when gamma builds the coarse preview variant
    (``[simulation.preview] mesh_scale``). Scripts that solve port modes must
    use it, because mode profiles belong to the grid they were solved on."""
    return float(os.environ.get("GAMMA_PREVIEW_MESH_SCALE", "1"))


def run_time_scale() -> float:
    return float(os.environ.get("GAMMA_PREVIEW_RUN_TIME_SCALE", "1"))


def is_preview() -> bool:
    return "GAMMA_PREVIEW_MESH_SCALE" in os.environ


def project_root() -> Path:
    env = os.environ.get("GAMMA_PROJECT")
    if env:
        return Path(env)
    d = Path.cwd().resolve()
    for p in [d, *d.parents]:
        if (p / "gamma.toml").is_file():
            return p
    raise RuntimeError("not inside a gamma project (no gamma.toml here or above)")


def manifest() -> dict:
    try:
        import tomllib
    except ModuleNotFoundError:  # Python < 3.11
        import tomli as tomllib  # type: ignore
    return tomllib.loads((project_root() / "gamma.toml").read_text())


def _atomic_write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    with open(tmp, "wb") as f:
        f.write(data)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, path)


def ports_of(component) -> list[dict]:
    """Ports of a gdsfactory component as gamma reads them (um, degrees)."""
    out = []
    for p in component.ports:
        layer = p.layer
        try:
            import gdsfactory as gf

            info = gf.get_layer(layer)
            layer = [int(info.layer), int(info.datatype)] if hasattr(info, "layer") else list(gf.kcl.get_info(info).to_tuple()) if hasattr(gf, "kcl") else None
        except Exception:
            layer = None
        out.append({"name": p.name, "x": float(p.center[0]), "y": float(p.center[1]), "width": float(p.width), "orientation": float(p.orientation) % 360.0, "layer": layer})
    return out


def write_ports(ports: list[dict], path: str | Path | None = None) -> Path:
    m = manifest().get("layout", {})
    path = project_root() / (path or m.get("ports") or "build/layout.ports.json")
    _atomic_write(Path(path), json.dumps(ports, indent=1).encode())
    return Path(path)


def write_layout(component, gds: str | Path | None = None, ports: str | Path | None = None) -> Path:
    """Write the component's GDS and ports to the paths in gamma.toml."""
    m = manifest().get("layout", {})
    root = project_root()
    gds_path = root / (gds or m.get("gds") or "build/layout.gds")
    gds_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = gds_path.with_name(f".{gds_path.stem}.tmp-{os.getpid()}.gds")
    component.write_gds(tmp)
    os.replace(tmp, gds_path)
    write_ports(ports_of(component), ports or m.get("ports") or gds_path.with_suffix(".ports.json").relative_to(root))
    print(f"wrote {gds_path.relative_to(root)} ({len(component.ports)} ports)")
    return gds_path


def write_sim(sim, name: str, path: str | Path | None = None) -> Path:
    """Write a lightsim Simulation (or its JSON text, or a dict) as simulation `name`."""
    root = project_root()
    if path is None and os.environ.get("GAMMA_SPEC_OUT"):
        path = os.environ["GAMMA_SPEC_OUT"]
    if path is None:
        for s in manifest().get("simulation", []):
            if s.get("name") == name:
                path = s.get("spec")
        path = path or f"build/sims/{name}.json"
    text = sim if isinstance(sim, str) else json.dumps(sim) if isinstance(sim, dict) else sim.to_json()
    p = root / path
    _atomic_write(p, text.encode())
    print(f"wrote {p.relative_to(root)}")
    return p


def build_netlist(netlist: str | Path, gds: str | Path | None = None, ports: str | Path | None = None):
    """Build a gdsfactory YAML netlist (`*.pic.yml`, edited by gamma's place and
    route tools) into the project's GDS and ports."""
    import gdsfactory as gf

    gf.gpdk.PDK.activate()
    root = project_root()
    c = gf.read.from_yaml((root / netlist).read_text())
    write_layout(c, gds, ports)
    return c
