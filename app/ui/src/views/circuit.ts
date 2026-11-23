// Quantum circuits with lightsim-quantum: the components, connections and
// tasks of each circuit document, a run through the lightsim-quantum program
// with progress, and plots, bar charts and tables of the results.

import { store, css } from "../lib/store";
import { enc } from "../lib/api";
import { Plot } from "../lib/plot";
import { el, fitCanvas, fmt, toast } from "../lib/util";

export class CircuitView {
  root = el("div", { class: "page" });
  private progress = new Map<string, HTMLElement>();
  constructor() {
    store.on("event", (e) => {
      if (e.kind === "circuit.progress") {
        const p = this.progress.get(e.data.circuit);
        if (p) p.textContent = `${(100 * e.data.progress.fraction).toFixed(0)}% · ${e.data.progress.message}`;
      }
    });
  }
  async load() {
    const c = await store.hub.get(store.p("/circuits")).catch(() => null);
    const items: HTMLElement[] = [];
    for (const k of c?.circuits ?? []) {
      const v = await store.hub.get(store.p(`/circuits/${enc(k.name)}`)).catch((e) => ({ error: e.message }));
      const box = el("div");
      items.push(el("h2", {}, v.name || k.name), el("p", { class: "muted mono small" }, `${k.file} · ${v.kind ?? ""}${v.sweep ? " · sweep of " + v.sweep : ""}`), box);
      if (v.error) { box.append(el("p", { class: "serif" }, v.error)); continue; }
      if (!v.recognised) { box.append(el("p", { class: "serif" }, `gamma cannot read this circuit's components (top-level keys: ${v.keys.join(", ")}).`)); continue; }
      const prog = el("span", { class: "muted small mono" });
      this.progress.set(k.name, prog);
      const results = el("div");
      const run = el("button", { class: "link accent", disabled: !c.engine_available, title: c.engine_available ? "" : "lightsim-quantum is not installed" }, "Run");
      run.addEventListener("click", async () => {
        prog.textContent = "starting…";
        try { const r = await store.hub.post(store.p(`/circuits/${enc(k.name)}/run`)); prog.textContent = `done (${r.run})`; this.renderResults(results, r.summary); }
        catch (e: any) { prog.textContent = ""; toast(e.message, "err", 9000); }
      });
      box.append(
        el("div", { class: "grid2" },
          el("div", {}, el("table", { class: "data" }, el("tr", {}, el("th", {}, "Component"), el("th", {}, "Model"), el("th", {}, "Parameters")),
            ...v.components.map((n: any) => el("tr", { "data-click": "1", onclick: () => store.select({ reference: `circuit:${k.name}/${n.id}`, description: `component ${n.id} (${n.kind}) of circuit ${k.name}`, data: n }) },
              el("td", { class: "mono" }, n.id), el("td", {}, n.kind), el("td", { class: "mono small muted" }, Object.entries(n.params || {}).map(([a, b]) => `${a}=${typeof b === "object" ? JSON.stringify(b) : b}`).join(" ")))))),
          el("div", {},
            v.connections.length ? el("p", { class: "mono small" }, v.connections.map((x: string[]) => `${x[0]} → ${x[1]}`).join("   ")) : "",
            v.inputs.length ? el("p", { class: "small" }, `inputs ${v.inputs.join(", ")} · outputs ${v.outputs.join(", ")}`) : "",
            el("table", { class: "data" }, el("tr", {}, el("th", {}, "Task"), el("th", {}, "Type")), ...v.tasks.map((t: any) => el("tr", {}, el("td", { class: "mono" }, t.id), el("td", {}, t.type ?? t.kind)))))),
        el("div", { class: "actions" }, run, prog),
        results);
      const last = await store.hub.get(store.p(`/circuits/${enc(k.name)}/results`)).catch(() => null);
      if (last?.summary) { prog.textContent = `latest run ${last.run}`; this.renderResults(results, last.summary); }
    }
    this.root.replaceChildren(
      el("h1", {}, "Circuit"),
      el("p", { class: "lede" }, "Circuits of quantum light simulated with lightsim-quantum: sources of single photons or squeezed light, linear optics, detectors. Components can take their S-parameters from lightsim runs (Touchstone files)."),
      el("p", { class: "serif" }, c?.engine_available ? `lightsim-quantum ${c.engine_version ?? ""} is installed.` : "lightsim-quantum is not installed on this machine, so circuits can be read but not run. Put the lightsim-quantum program on PATH or next to gamma."),
      ...(items.length ? items : [el("p", { class: "serif muted" }, "This project has no circuits. Add one under [[circuit]] in gamma.toml (a lightsim-quantum document).")]));
  }
  renderResults(box: HTMLElement, s: any) {
    const grid = el("div", { class: "grid2", style: { marginTop: "12px" } });
    for (const p of s.plots ?? []) {
      const c = el("canvas");
      grid.append(el("div", { class: "figure" }, el("div", { class: "cap" }, p.title), c));
      new Plot(c).set({ x: p.x.map((v: number) => (p.xlabel === "delay" ? v * 1e12 : v)), series: p.series, xlabel: p.xlabel === "delay" ? "delay (ps)" : p.xlabel, ylabel: "" });
    }
    for (const b of s.bars ?? []) {
      const c = el("canvas");
      grid.append(el("div", { class: "figure" }, el("div", { class: "cap" }, b.title), c));
      requestAnimationFrame(() => bars(c, b.labels, b.values));
      new ResizeObserver(() => bars(c, b.labels, b.values)).observe(c);
    }
    for (const t of s.tables ?? []) {
      grid.append(el("div", { class: "figure", style: { height: "auto", padding: "30px 14px 12px" } }, el("div", { class: "cap" }, t.title),
        el("table", { class: "data" }, ...t.rows.map((r: string[]) => el("tr", {}, ...r.map((x) => el("td", { class: "num" }, x)))))));
    }
    box.replaceChildren(grid);
  }
}

function bars(c: HTMLCanvasElement, labels: string[], values: number[]) {
  const [w, h] = fitCanvas(c);
  const g = c.getContext("2d")!;
  const dpr = window.devicePixelRatio || 1;
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  const L = 50, R = w - 14, T = 34, B = h - 46;
  const mx = Math.max(...values, 1e-12);
  const bw = (R - L) / Math.max(values.length, 1);
  g.font = "11px 'IBM Plex Mono', monospace";
  g.strokeStyle = css("--rule"); g.fillStyle = css("--ink-3"); g.textAlign = "right";
  for (const t of [0, 0.5, 1]) { const y = B - t * (B - T); g.beginPath(); g.moveTo(L, y); g.lineTo(R, y); g.stroke(); g.fillText(fmt(t * mx, 3), L - 6, y + 4); }
  g.fillStyle = css("--accent");
  values.forEach((v, i) => g.fillRect(L + i * bw + bw * 0.15, B - (v / mx) * (B - T), bw * 0.7, (v / mx) * (B - T)));
  g.fillStyle = css("--ink-2"); g.textAlign = "center";
  labels.forEach((l, i) => { if (bw > 14 || i % Math.ceil(14 / bw) === 0) g.fillText(l, L + (i + 0.5) * bw, B + 15); });
  g.fillText("output pattern (photons per mode)", (L + R) / 2, h - 10);
}
