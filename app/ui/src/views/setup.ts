// Simulation set-up: the permittivity lightsim rasterises on its own grid,
// mesh lines, sources, monitors and absorbing layers, estimates per device,
// and the run button.

import { store, css } from "../lib/store";
import { b64f32, enc } from "../lib/api";
import { MapView, canvasFromImageData, type MapOverlay } from "../lib/plot";
import { paintMap } from "../lib/colormap";
import { el, fmt, fmtBytes, fmtTime, toast, um } from "../lib/util";

export class SetupView {
  root = el("div", { style: { position: "absolute", inset: "0" } });
  canvas = el("canvas", { class: "fill", "aria-label": "Simulation set-up view", tabindex: "0" });
  map = new MapView(this.canvas);
  summary: any = null;
  axis = 2;
  coord: number | null = null;
  preview = false;
  private version = "";
  private side = el("div", { class: "panel", style: { right: "12px", top: "52px", width: "330px", maxHeight: "calc(100% - 70px)", overflow: "auto" } });
  private tools = el("div", { class: "toolbar" });

  constructor() {
    this.root.append(this.canvas, this.tools, this.side);
    this.map.insetRight = 350;
    this.map.onPick = (x, y, ov) => {
      if (ov?.ref) {
        const o = this.summary.objects.find((q: any) => q.reference === ov.ref);
        store.select({ reference: ov.ref, description: `${o.kind} ${o.name} (${o.type}) of simulation ${store.sim}`, data: o });
      } else store.select({ reference: `point:${fmt(x, 4)},${fmt(y, 4)}`, description: `point in the ${["y-z", "x-z", "x-y"][this.axis]} plane of ${store.sim}` });
    };
    this.map.onHover = (x, y) => store.emit("cursor", `${["y", "x", "x"][this.axis]} ${fmt(x, 3)} µm  ${["z", "z", "y"][this.axis]} ${fmt(y, 3)} µm`);
    store.on("sims", () => this.load());
    store.on("theme", () => this.paint());
    store.on("selection", (s) => { this.map.highlight = s?.reference ?? null; this.map.draw(); });
  }

  async load(force = false) {
    const key = `${store.sim}|${store.simVersion}|${this.preview}`;
    if (!force && key === this.version) return;
    this.version = key;
    this.renderTools();
    if (!store.sim) { this.side.replaceChildren(el("p", { class: "serif muted" }, "This project has no simulations.")); this.map.image = null; this.map.draw(); return; }
    this.side.replaceChildren(el("p", { class: "muted small" }, "Building lightsim's grid…"));
    try {
      this.summary = await store.hub.get(store.p(`/sims/${enc(store.sim)}/setup?preview=${this.preview ? 1 : 0}`));
      this.renderSide();
      await this.loadSlice(true);
    } catch (e: any) {
      this.summary = null;
      this.side.replaceChildren(el("h4", {}, store.sim), el("p", { class: "serif" }, e.message),
        el("button", { class: "link accent", onclick: () => this.build() }, "Build the simulation"));
      this.map.image = null; this.map.overlays = []; this.map.draw();
    }
  }

  async build() {
    toast("Building…", "ok", 2000);
    try { await store.hub.post(store.p("/build"), { target: `sim:${store.sim}` }); } catch (e: any) { toast(e.message, "err", 9000); }
  }

  private slice: any = null;
  async loadSlice(fit = false) {
    const c = this.coord === null ? "" : `&coord=${this.coord}`;
    const s = await store.hub.get(store.p(`/sims/${enc(store.sim)}/slice?axis=${this.axis}${c}&preview=${this.preview ? 1 : 0}`));
    s.eps = b64f32(s.eps);
    this.slice = s;
    if (this.coord === null) this.coord = s.coord;
    this.renderTools();
    this.paint();
    if (fit) this.map.fit();
  }

  paint() {
    const s = this.slice; if (!s) return;
    const [nu, nv] = s.dims;
    this.map.image = canvasFromImageData(paintMap(s.eps, nu, nv, "material", store.theme === "dark"));
    const du = nu > 1 ? s.u[1] - s.u[0] : 0, dv = nv > 1 ? s.v[1] - s.v[0] : 0;
    this.map.extent = [(s.u[0] - du / 2) * 1e6, (s.v[0] - dv / 2) * 1e6, (s.u[nu - 1] + du / 2) * 1e6, (s.v[nv - 1] + dv / 2) * 1e6];
    this.map.lines = { u: s.u_nodes.map((x: number) => x * 1e6), v: s.v_nodes.map((x: number) => x * 1e6) };
    const [a0, a1] = s.axes;
    const ov: MapOverlay[] = [];
    const sm = this.summary;
    if (sm) {
      const r = sm.region;
      ov.push({ min: [r[0][a0] * 1e6, r[0][a1] * 1e6], max: [r[1][a0] * 1e6, r[1][a1] * 1e6], label: "region (absorbing layers outside)", color: css("--ink-3"), dashed: true });
      const tol = 0.75 * sm.grid.max_cell[this.axis]; // objects within a cell of the plane
      for (const o of sm.objects) {
        if (o.kind === "structure") continue;
        if (o.min[this.axis] > s.coord + tol || o.max[this.axis] < s.coord - tol) continue;
        ov.push({ min: [o.min[a0] * 1e6, o.min[a1] * 1e6], max: [o.max[a0] * 1e6, o.max[a1] * 1e6], label: o.name, color: o.kind === "source" ? css("--source") : css("--monitor"), ref: o.reference });
      }
    }
    this.map.overlays = ov;
    this.map.draw();
  }

  renderTools() {
    const sel = el("select", { "aria-label": "Simulation" }, ...store.sims.map((s) => el("option", { value: s.name, selected: s.name === store.sim }, s.name))) as HTMLSelectElement;
    sel.onchange = () => { store.sim = sel.value; this.coord = null; this.load(true); store.emit("sim-selected"); };
    const ax = el("select", { "aria-label": "Plane normal" }, ...["x", "y", "z"].map((a, i) => el("option", { value: String(i), selected: i === this.axis }, `normal ${a}`))) as HTMLSelectElement;
    ax.onchange = () => { this.axis = Number(ax.value); this.coord = null; this.loadSlice(true); };
    const g = this.summary?.grid;
    let slider: HTMLElement | null = null;
    if (g) {
      const lo = this.summary.region[0][this.axis], hi = this.summary.region[1][this.axis];
      const r = el("input", { type: "range", min: String(lo), max: String(hi), step: String((hi - lo) / 400), value: String(this.coord ?? (lo + hi) / 2), "aria-label": "Plane position" }) as HTMLInputElement;
      const lab = el("span", { class: "mono small" }, `${["x", "y", "z"][this.axis]} = ${um(Number(r.value))} µm`);
      r.oninput = () => (lab.textContent = `${["x", "y", "z"][this.axis]} = ${um(Number(r.value))} µm`);
      r.onchange = () => { this.coord = Number(r.value); this.loadSlice(); };
      slider = el("label", {}, r, lab);
    }
    const pv = el("input", { type: "checkbox", checked: this.preview }) as HTMLInputElement;
    pv.onchange = () => { this.preview = pv.checked; this.load(true); };
    this.tools.replaceChildren(el("span", { class: "title" }, "Set-up"), sel, ax, slider ?? "", el("label", { title: "The coarse preview variant of this simulation" }, pv, "preview mesh"),
      el("button", { class: "link", onclick: () => this.map.fit() }, "Fit"),
      el("button", { class: "link", onclick: () => this.build(), title: "Regenerate the spec from its script" }, "Rebuild"));
  }

  renderSide() {
    const s = this.summary, g = s.grid;
    const est = el("table", { class: "data" }, el("tr", {}, el("th", {}, "Device"), el("th", {}, "Time"), el("th", {}, "")));
    const devs: { id: string; name: string; seconds: number | null; fits: boolean | null; host?: string }[] = s.estimates.map((e: any) => ({ id: e.device, name: e.name, seconds: e.seconds, fits: e.fits }));
    for (const r of s.remotes || []) {
      for (const d of ["cuda:0"]) {
        const known = devs.find((x) => x.id === `remote:${r.name}/${d}`);
        if (!known) devs.push({ id: `remote:${r.name}/${d}`, name: `${r.name} · ${d}`, seconds: null, fits: null, host: r.name });
      }
    }
    for (const d of devs) {
      const isRemote = d.id.startsWith("remote:");
      const host = isRemote ? d.id.slice(7).split("/")[0] : undefined;
      const device = isRemote ? d.id.split("/")[1] : d.id;
      const run = el("button", { class: "link accent", onclick: () => this.run(device, host) }, "Run");
      est.append(el("tr", {},
        el("td", { title: d.name }, el("div", {}, isRemote ? `${host} ${device}` : d.id), el("div", { class: "muted small", style: { maxWidth: "150px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } }, isRemote ? "remote host" : d.name.replace("NVIDIA ", ""))),
        el("td", { class: "num" }, d.seconds === null ? "–" : `~${fmtTime(d.seconds)}`, d.fits === false ? el("div", { class: "small", style: { color: "var(--err)" } }, "too large") : null),
        el("td", { style: { textAlign: "right" } }, run)));
    }
    const objs = s.objects.filter((o: any) => o.kind !== "structure");
    const wl = s.wavelengths ? `${fmt(s.wavelengths[0] * 1e9, 1)}–${fmt(s.wavelengths[1] * 1e9, 1)} nm` : "–";
    this.side.replaceChildren(
      el("h4", {}, `${s.name}${this.preview ? " · preview" : ""}`),
      el("p", { class: "muted small", style: { margin: "-4px 0 6px" } }, `${(store.sims.find((x) => x.name === store.sim)?.solver ?? "fdtd").toUpperCase()} · lightsim`),
      el("dl", { class: "kv" },
        el("dt", {}, "Grid"), el("dd", {}, `${g.n[0]} × ${g.n[1]} × ${g.n[2]}`),
        el("dt", {}, "Cells"), el("dd", {}, `${fmt(g.cells / 1e6, 2)} M`),
        el("dt", {}, "Smallest cell"), el("dd", {}, `${fmt(g.min_cell[0] * 1e9, 1)} · ${fmt(g.min_cell[1] * 1e9, 1)} · ${fmt(g.min_cell[2] * 1e9, 1)} nm`),
        el("dt", {}, "Time step"), el("dd", {}, `${fmt(g.dt * 1e15, 4)} fs`),
        el("dt", {}, "Steps"), el("dd", {}, `up to ${g.steps}`),
        el("dt", {}, "Shut-off"), el("dd", {}, s.shutoff ? `energy ≤ ${s.shutoff.toExponential(0)} of peak` : "none"),
        el("dt", {}, "Memory"), el("dd", {}, fmtBytes(g.memory_bytes)),
        el("dt", {}, "Wavelengths"), el("dd", {}, wl),
        el("dt", {}, "Absorbing layers"), el("dd", {}, g.pml_layers.map((p: number[]) => `${p[0]}/${p[1]}`).join(" · "))),
      el("h4", { style: { marginTop: "14px" } }, "Run on"), est,
      el("h4", { style: { marginTop: "14px" } }, "Media"),
      el("dl", { class: "kv" }, ...s.media.flatMap((m: any) => [el("dt", {}, m.name), el("dd", {}, `n ${fmt(m.index, 3)}${m.dispersive ? " · dispersive" : ""}${m.lossy ? " · lossy" : ""}`)])),
      el("h4", { style: { marginTop: "14px" } }, "Sources and monitors"),
      ...objs.map((o: any) => el("div", { class: "small", style: { padding: "2px 0", cursor: "pointer", color: o.kind === "source" ? "var(--source)" : "var(--monitor)" }, onclick: () => store.select({ reference: o.reference, description: `${o.kind} ${o.name} (${o.type})`, data: o }) },
        `${o.name}`, el("span", { class: "muted" }, ` ${o.type.replace("_", " ")}`))),
      el("p", { class: "muted small", style: { marginTop: "12px" } }, `${s.objects.length - objs.length} structures. Estimates use measured throughput where a device has run before.`),
    );
  }

  async run(device: string, host?: string) {
    try {
      const j = await store.hub.post(store.p(`/sims/${enc(store.sim)}/run`), { device, host, preview: this.preview });
      toast(`Queued ${store.sim}${this.preview ? " (preview)" : ""} on ${host ? host + " " : ""}${device}`, "ok", 3000);
      store.emit("job-started", j.id);
      store.setTab("run");
    } catch (e: any) { toast(e.message, "err", 9000); }
  }
}
