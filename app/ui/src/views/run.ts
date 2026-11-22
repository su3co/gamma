// Running jobs: progress, energy decay and the live field slice.

import { store } from "../lib/store";
import { b64f32 } from "../lib/api";
import { MapView, Plot, canvasFromImageData } from "../lib/plot";
import { paintMap } from "../lib/colormap";
import { el, fmt, fmtBytes, fmtTime, toast } from "../lib/util";

export class RunView {
  root = el("div", { class: "page" });
  job: string | null = null;
  decay = new Map<string, { t: number[]; d: number[] }>();
  slices = new Map<string, any>();
  private map!: MapView;
  private plot!: Plot;
  private head = el("div");
  private list = el("div");
  private logBox = el("pre", { class: "mono small muted", style: { whiteSpace: "pre-wrap", maxHeight: "160px", overflow: "auto", display: "none" } });

  constructor() {
    const mapCanvas = el("canvas", { "aria-label": "Live field slice" });
    const plotCanvas = el("canvas", { "aria-label": "Energy decay" });
    this.map = new MapView(mapCanvas);
    this.map.units = "cells";
    this.plot = new Plot(plotCanvas);
    this.root.append(
      el("h1", {}, "Run"),
      el("p", { class: "lede" }, "Simulations queue per device; each host runs one job at a time. Progress, the energy decay that ends a run, and a field plane arrive about once a second."),
      this.head,
      el("div", { class: "grid2", style: { marginTop: "14px" } },
        el("div", { class: "figure", style: { gridColumn: "1 / -1", height: "320px" } }, el("div", { class: "cap" }, "Live field"), mapCanvas),
        el("div", { class: "figure", style: { gridColumn: "1 / -1", height: "240px" } }, el("div", { class: "cap" }, "Field energy / peak"), plotCanvas)),
      el("h2", {}, "Jobs"), this.list,
    );
    store.on("jobs", () => this.render());
    store.on("job-started", (id) => { this.job = id; this.render(); });
    store.on("theme", () => this.paintSlice());
    store.on("event", (e) => {
      if (e.kind === "sim.progress") {
        const p = e.data.progress;
        const d = this.decay.get(e.data.job) || { t: [], d: [] };
        if (p.decay !== null && p.decay !== undefined) { d.t.push(p.step); d.d.push(p.decay); }
        this.decay.set(e.data.job, d);
        const j = store.jobs.find((x) => x.id === e.data.job);
        if (j) { j.progress = p; j.status = "running"; }
        if (!this.job) this.job = e.data.job;
        if (e.data.job === this.job) this.renderHead();
      } else if (e.kind === "sim.slice") {
        this.slices.set(e.data.job, e.data.slice);
        if (!this.job) this.job = e.data.job;
        if (e.data.job === this.job) this.paintSlice();
      } else if (e.kind === "sim.plan") {
        const j = store.jobs.find((x) => x.id === e.data.job);
        if (j) j.plan = e.data.plan;
        if (e.data.job === this.job) this.renderHead();
      } else if (e.kind === "sim.log" && e.data.job === this.job) {
        this.logBox.textContent += e.data.line + "\n";
      } else if (e.kind === "sim.done") {
        if (e.data.status === "failed") toast(`${e.data.sim} failed: ${e.data.error}`, "err", 10000);
        else if (e.data.status === "done") toast(`${e.data.sim} finished on ${e.data.host || "this machine"} (${e.data.device})`, "ok", 5000);
      }
    });
  }

  async render() {
    if (!this.job && store.jobs.length) this.job = store.jobs[store.jobs.length - 1].id;
    const rows = [...store.jobs].reverse().map((j) => el("tr", { "data-click": "1", class: j.id === this.job ? "sel" : "", onclick: () => { this.job = j.id; this.render(); } },
      el("td", {}, j.sim, j.preview ? el("span", { class: "muted" }, " preview") : null),
      el("td", {}, j.host ? `${j.host} ${j.device}` : `${j.device}`),
      el("td", {}, el("span", { class: `pill ${j.status}` }, j.status)),
      el("td", { class: "num" }, j.progress ? `${j.progress.step}/${j.progress.steps}` : "–"),
      el("td", { class: "num muted" }, (j.created || "").replace("T", " ").slice(0, 19))));
    this.list.replaceChildren(rows.length ? el("table", { class: "data" }, el("tr", {}, el("th", {}, "Simulation"), el("th", {}, "Where"), el("th", {}, "Status"), el("th", {}, "Steps"), el("th", {}, "Queued (UTC)")), ...rows)
      : el("p", { class: "serif muted" }, "No jobs yet. Start one from the Set-up view or ask the agent."));
    this.renderHead();
    if (this.job && !this.slices.has(this.job)) {
      const s = await store.hub.get(`/api/jobs/${this.job}/slice`).catch(() => null);
      if (s) { this.slices.set(this.job, s); this.paintSlice(); }
    }
  }

  renderHead() {
    const j = store.jobs.find((x) => x.id === this.job);
    if (!j) { this.head.replaceChildren(); return; }
    const p = j.progress, plan = j.plan;
    const finished = j.status === "done";
    const frac = finished ? 1 : p ? p.step / Math.max(p.steps, 1) : 0;
    const eta = !finished && p && p.mcells_per_s && plan ? (plan.cells * (p.steps - p.step)) / (p.mcells_per_s * 1e6) : null;
    const early = finished && p && p.step < p.steps;
    const showLog = el("button", { class: "link small", onclick: () => { this.logBox.style.display = this.logBox.style.display === "none" ? "block" : "none"; } }, "Log");
    this.head.replaceChildren(
      el("div", { style: { display: "flex", alignItems: "baseline", gap: "18px", flexWrap: "wrap" } },
        el("span", { class: "big" }, finished ? "Done" : `${(frac * 100).toFixed(0)}%`),
        el("span", { style: { font: "700 17px var(--sans)" } }, `${j.sim}${j.preview ? " · preview" : ""}`),
        el("span", { class: "muted" }, j.host ? `on ${j.host} · ${j.device}` : `on this machine · ${j.device}`),
        el("span", { class: `pill ${j.status}` }, j.status),
        ["queued", "staging", "running"].includes(j.status) ? el("button", { class: "link accent", onclick: () => store.hub.post(`/api/jobs/${j.id}/cancel`).then(() => store.refreshJobs()) }, "Cancel") : null,
        j.status === "done" ? el("button", { class: "link", onclick: () => { store.emit("show-run", { sim: j.sim, run: j.run_id }); store.setTab("results"); } }, "Results") : null,
        showLog),
      el("div", { class: "bar", style: { margin: "10px 0" } }, el("div", { style: { width: `${frac * 100}%` } })),
      el("div", { class: "stats" },
        stat("Step", p ? `${p.step} / ${p.steps}${early ? " (shut off)" : ""}` : "–"),
        stat("Decay", p?.decay ? p.decay.toExponential(1) : "–"),
        stat("Throughput", p?.mcells_per_s ? `${fmt(p.mcells_per_s / 1000, 2)} Gcells/s` : "–"),
        stat("Elapsed", p ? fmtTime(p.elapsed) : "–"),
        finished ? "" : stat("Remaining", eta !== null ? `≤ ${fmtTime(eta)}` : "–"),
        stat("Cells", plan ? `${fmt(plan.cells / 1e6, 2)} M` : "–"),
        stat("Memory", plan ? fmtBytes(plan.memory_bytes) : "–")),
      j.error ? el("p", { class: "serif", style: { color: "var(--err)", whiteSpace: "pre-wrap" } }, j.error) : "",
      this.logBox);
    if (j.decay?.length && !(this.decay.get(j.id)?.t.length)) this.decay.set(j.id, { t: j.decay.map((x: number[]) => x[0]), d: j.decay.map((x: number[]) => x[1]) });
    const d = this.decay.get(j.id);
    this.plot.set(d && d.t.length ? { x: d.t, series: [{ name: "energy / peak", y: d.d }], xlabel: "step", ylabel: "", logY: true } : null);
  }

  paintSlice() {
    const s = this.slices.get(this.job || "");
    if (!s) { this.map.image = null; this.map.draw(); return; }
    const data = b64f32(s.data);
    const first = !this.map.image;
    this.map.image = canvasFromImageData(paintMap(data, s.dims[0], s.dims[1], "diverging", store.theme === "dark"));
    this.map.extent = [0, 0, s.dims[0], s.dims[1]];
    this.map.overlays = [];
    if (first) this.map.fit(30); else this.map.draw();
  }
}

function stat(l: string, v: string) { return el("div", { class: "s" }, el("div", { class: "l" }, l), el("div", { class: "v" }, v)); }
