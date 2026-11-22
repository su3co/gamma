// Results: S-parameters, spectra, field maps, time series and the decay.

import { store } from "../lib/store";
import { b64f32, enc } from "../lib/api";
import { MapView, Plot, canvasFromImageData } from "../lib/plot";
import { paintMap } from "../lib/colormap";
import { el, fmt, fmtTime } from "../lib/util";

export class ResultsView {
  root = el("div", { class: "page" });
  sim = "";
  run = "";
  overview: any = null;
  private body = el("div");
  private runsBox = el("div");

  constructor() {
    this.root.append(el("h1", {}, "Results"), el("p", { class: "lede" }, "Every run keeps its spec, provenance and lightsim results under results/<simulation>/<run>; pick a run to see its spectra, S-parameters and fields."), this.runsBox, this.body);
    store.on("runs", () => this.renderRuns());
    store.on("show-run", (r) => { this.sim = r.sim; this.run = r.run; this.load(); });
    store.on("theme", () => this.load());
  }

  renderRuns() {
    const done = store.runs.filter((r) => r.has_results);
    if (!this.run && done.length) { this.sim = done[0].sim; this.run = done[0].id; this.load(); }
    const rows = store.runs.slice(0, 12).map((r) => el("tr", { "data-click": r.has_results ? "1" : undefined, class: r.id === this.run ? "sel" : "", onclick: () => { if (!r.has_results) return; this.sim = r.sim; this.run = r.id; this.load(); this.renderRuns(); } },
      el("td", {}, r.sim, r.run.preview ? el("span", { class: "muted" }, " preview") : null),
      el("td", { class: "mono small" }, r.id),
      el("td", {}, r.run.host ? `${r.run.host} ${r.run.device}` : r.run.device || "–"),
      el("td", {}, el("span", { class: `pill ${r.run.status}` }, r.run.status || "?")),
      el("td", { class: "num" }, r.run.stats ? fmtTime(r.run.stats.seconds) : "–"),
      el("td", { class: "num" }, r.run.stats?.mcells_per_s ? `${fmt(r.run.stats.mcells_per_s / 1000, 2)} Gc/s` : "–")));
    this.runsBox.replaceChildren(rows.length ? el("table", { class: "data" }, el("tr", {}, el("th", {}, "Simulation"), el("th", {}, "Run"), el("th", {}, "Where"), el("th", {}, "Status"), el("th", {}, "Wall time"), el("th", {}, "Rate")), ...rows)
      : el("p", { class: "serif muted" }, "No runs yet."));
  }

  async load() {
    if (!this.run) return;
    const base = store.p(`/runs/${enc(this.sim)}/${enc(this.run)}`);
    try { this.overview = await store.hub.get(`${base}/overview`); } catch (e: any) { this.body.replaceChildren(el("p", { class: "serif" }, e.message)); return; }
    const o = this.overview;
    const figs: HTMLElement[] = [];
    const st = o.stats;
    this.body.replaceChildren(
      el("h2", {}, `${this.sim} · ${this.run}`),
      el("div", { class: "stats" },
        stat("Backend", st.backend || "–"), stat("Cells", `${fmt(st.cells / 1e6, 2)} M`), stat("Steps", `${st.steps}${st.stopped_early ? " (shut off)" : ""}`),
        stat("Wall time", fmtTime(st.seconds)), stat("Throughput", `${fmt(st.mcells_per_s / 1000, 2)} Gcells/s`)),
      el("div", { class: "grid2", style: { marginTop: "16px" } }, ...figs));
    const grid = this.body.querySelector(".grid2")!;
    const addPlot = (title: string, spec: any) => {
      const c = el("canvas"); const f = el("div", { class: "figure" }, el("div", { class: "cap" }, title), c);
      grid.append(f); new Plot(c).set(spec);
    };
    if (o.sparams && o.sparams.entries.length) {
      const sp = o.sparams;
      const x = sp.wavelengths.map((w: number) => w * 1e9);
      addPlot("S-parameters (dB)", { x, series: sp.entries.map((e: any) => ({ name: e.name, y: e.db })), xlabel: "wavelength (nm)", ylabel: "" });
      const mid = Math.floor(x.length / 2);
      grid.append(el("div", { class: "figure", style: { height: "auto", padding: "30px 14px 12px" } }, el("div", { class: "cap" }, `At ${fmt(x[mid], 1)} nm`),
        el("table", { class: "data" }, el("tr", {}, el("th", {}, "Entry"), el("th", {}, "|S|²"), el("th", {}, "dB"), el("th", {}, "phase (rad)")),
          ...sp.entries.map((e: any) => el("tr", {}, el("td", { class: "mono" }, e.name), el("td", { class: "num" }, fmt(e.abs2[mid], 4)), el("td", { class: "num" }, fmt(e.db[mid], 2)), el("td", { class: "num" }, fmt(e.phase[mid], 3))))),
        el("p", { class: "muted small serif" }, sp.note)));
    }
    for (const m of o.monitors) {
      if (m.views.includes("field_map")) await this.fieldFigure(grid, base, m.name);
      else if (m.views.includes("spectrum") && !(o.sparams && m.type === "mode")) {
        const s = await store.hub.get(`${base}/spectrum?monitor=${enc(m.name)}`).catch(() => null);
        if (s) addPlot(`${m.name} (${m.type.replace("_", " ")})`, { x: s.wavelengths.map((w: number) => w * 1e9), series: s.series.map((q: any) => ({ name: q.name, y: q.values })), xlabel: "wavelength (nm)", ylabel: "" });
      } else if (m.views.includes("time_series")) {
        const t = await store.hub.get(`${base}/time?monitor=${enc(m.name)}`).catch(() => null);
        if (t) addPlot(`${m.name} (time)`, { x: t.times.map((v: number) => v * 1e15), series: t.series.map((q: any) => ({ name: q.name, y: q.values })), xlabel: "time (fs)", ylabel: "" });
      }
    }
    const d = await store.hub.get(`${base}/decay`).catch(() => null);
    if (d && d.times.length) addPlot("Field energy / peak", { x: d.times.map((v: number) => v * 1e15), series: [{ name: "energy / peak", y: d.series[0].values }], xlabel: "time (fs)", ylabel: "", logY: true });
  }

  async fieldFigure(grid: Element, base: string, monitor: string) {
    const c = el("canvas", { "aria-label": `Field map ${monitor}` });
    const q = el("select", { "aria-label": "Quantity" }, ...["|E|2", "Ex", "Ey", "Ez", "abs(Ey)", "phase(Ey)"].map((v) => el("option", { value: v }, v))) as HTMLSelectElement;
    const info = el("span", { class: "mono small muted" });
    const f = el("div", { class: "figure tall", style: { gridColumn: "1 / -1" } }, el("div", { class: "cap", style: { display: "flex", gap: "12px", alignItems: "center" } }, `${monitor}`, q, info), c);
    grid.append(f);
    const mv = new MapView(c);
    const load = async () => {
      try {
        const fm = await store.hub.get(`${base}/field?monitor=${enc(monitor)}&q=${enc(q.value)}`);
        const data = b64f32(fm.data);
        const [nu, nv] = fm.dims;
        const kind = q.value.startsWith("|") || q.value.startsWith("abs") ? "intensity" : "diverging";
        mv.image = canvasFromImageData(paintMap(data, nu, nv, kind as any, store.theme === "dark"));
        mv.extent = [fm.u[0] * 1e6, fm.v[0] * 1e6, fm.u[nu - 1] * 1e6, fm.v[nv - 1] * 1e6];
        info.textContent = `${fmt(fm.wavelength * 1e9, 1)} nm · ${["y-z", "x-z", "x-y"][fm.axis]} plane · max ${fmt(fm.range[1], 3)}`;
        mv.fit(30);
      } catch (e: any) { info.textContent = e.message; }
    };
    q.onchange = load;
    await load();
  }
}

function stat(l: string, v: string) { return el("div", { class: "s" }, el("div", { class: "l" }, l), el("div", { class: "v" }, v)); }
