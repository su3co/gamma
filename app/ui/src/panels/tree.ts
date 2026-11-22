// The project tree: cells, layers (visibility), ports, simulations with their
// sources and monitors, runs, circuits.

import { store } from "../lib/store";
import { el, fmt } from "../lib/util";

export class Tree {
  root = el("nav", { class: "tree", "aria-label": "Project" });
  private setup: any = null;
  constructor() {
    for (const t of ["layout", "sims", "runs", "selection", "hidden", "info"]) store.on(t, () => this.render());
    store.on("sims", () => this.loadSetup());
    store.on("sim-selected", () => this.loadSetup());
  }
  async loadSetup() {
    this.setup = store.sim ? await store.hub.get(store.p(`/sims/${encodeURIComponent(store.sim)}/setup`)).catch(() => null) : null;
    this.render();
  }
  row(label: string | Node, ref: string, opts: { swatch?: string; meta?: string; indent?: boolean; desc?: string; data?: any; bbox?: any; extra?: Node; off?: boolean; onclick?: () => void } = {}) {
    const sel = store.selection?.reference === ref;
    const r = el("div", { class: `row${opts.indent ? " indent" : ""}${opts.off ? " off" : ""}`, role: "treeitem", tabindex: "0", "aria-selected": sel ? "true" : "false", title: ref },
      opts.swatch ? el("span", { class: "swatch", style: { background: opts.swatch } }) : null, label, opts.extra ?? null, opts.meta ? el("span", { class: "meta" }, opts.meta) : null);
    const act = () => { if (opts.onclick) opts.onclick(); else store.select({ reference: ref, description: opts.desc ?? ref, data: opts.data, bbox: opts.bbox }); };
    r.addEventListener("click", act);
    r.addEventListener("keydown", (e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); act(); } });
    return r;
  }
  render() {
    const kids: Node[] = [];
    const lay = store.layout;
    kids.push(el("h3", {}, "Layout"));
    if (!lay) kids.push(el("div", { class: "empty" }, "not built"));
    else {
      kids.push(this.row(el("span", {}, lay.top.length > 26 ? lay.top.slice(0, 24) + "…" : lay.top), `cell:${lay.top}`, { meta: `${lay.polygons}`, desc: `top cell ${lay.top} with ${lay.polygons} polygons`, bbox: lay.bbox }));
      for (const c of lay.cells.filter((c: any) => c.name !== lay.top).slice(0, 30)) kids.push(this.row(c.name.length > 26 ? c.name.slice(0, 24) + "…" : c.name, `cell:${c.name}`, { indent: true, meta: `×${c.instances}`, desc: `cell ${c.name}, placed ${c.instances} times` }));
      kids.push(el("h3", {}, "Layers"));
      for (const l of lay.layers) {
        const key = `${l.layer[0]}/${l.layer[1]}`;
        const off = store.hidden.has(key);
        const eye = el("button", { class: "eye", title: off ? "Show" : "Hide", "aria-label": `${off ? "Show" : "Hide"} layer ${key}`, onclick: (e: Event) => { e.stopPropagation(); off ? store.hidden.delete(key) : store.hidden.add(key); store.emit("hidden"); } }, off ? "○" : "●");
        kids.push(this.row(`${l.name ?? "layer"} ${key}`, l.reference, { swatch: l.color, meta: `${l.polygons}`, off, extra: eye,
          desc: `layer ${l.name ?? key} (${key})${l.material ? `, ${l.material}` : ""}${l.zmin !== null ? `, z ${fmt(l.zmin)}–${fmt(l.zmax)} µm` : ""}, ${l.polygons} polygons`, data: l, bbox: l.bbox }));
      }
      kids.push(el("h3", {}, "Ports"));
      if (!lay.ports.length) kids.push(el("div", { class: "empty" }, "no ports file"));
      for (const p of lay.ports) kids.push(this.row(p.name, `port:${p.name}`, { meta: `${fmt(p.width, 2)} µm`, data: p, desc: `port ${p.name} at (${fmt(p.x, 3)}, ${fmt(p.y, 3)}) µm, width ${fmt(p.width, 3)} µm, facing ${fmt(p.orientation, 0)}°`, bbox: { min: [p.x - p.width / 2, p.y - p.width / 2], max: [p.x + p.width / 2, p.y + p.width / 2] } }));
    }
    kids.push(el("h3", {}, "Simulations"));
    if (!store.sims.length) kids.push(el("div", { class: "empty" }, "none"));
    for (const s of store.sims) {
      kids.push(this.row(s.name, `sim:${s.name}`, { meta: s.built ? "" : "not built", desc: `simulation ${s.name}: ${s.description || s.spec}`, onclick: () => { store.sim = s.name; store.emit("sim-selected"); store.select({ reference: `sim:${s.name}`, description: `simulation ${s.name}: ${s.description || s.spec}` }); } }));
      if (s.name === store.sim && this.setup) {
        for (const o of this.setup.objects.filter((o: any) => o.kind !== "structure")) {
          kids.push(this.row(el("span", { style: { color: o.kind === "source" ? "var(--source)" : "var(--monitor)" } }, o.name), o.reference, { indent: true, meta: o.type.replace("_", " "), data: o, desc: `${o.kind} ${o.name} (${o.type}) of simulation ${s.name}` }));
        }
      }
    }
    kids.push(el("h3", {}, "Runs"));
    const runs = store.runs.slice(0, 8);
    if (!runs.length) kids.push(el("div", { class: "empty" }, "none yet"));
    for (const r of runs) kids.push(this.row(`${r.sim}`, `run:${r.sim}/${r.id}`, { meta: `${r.run.host ?? ""} ${r.run.status ?? ""}`.trim(), data: r.run, desc: `run ${r.id} of ${r.sim} on ${r.run.host ?? "this machine"} ${r.run.device} (${r.run.status})`,
      onclick: () => { store.select({ reference: `run:${r.sim}/${r.id}`, description: `run ${r.id} of ${r.sim}`, data: r.run }); if (r.has_results) { store.emit("show-run", { sim: r.sim, run: r.id }); store.setTab("results"); } } }));
    const circuits = store.info?.manifest?.circuit ?? [];
    if (circuits.length) {
      kids.push(el("h3", {}, "Circuits"));
      for (const c of circuits) kids.push(this.row(c.name, `file:${c.file}`, { meta: "quantum", onclick: () => store.setTab("circuit") }));
    }
    this.root.replaceChildren(...kids);
  }
}
