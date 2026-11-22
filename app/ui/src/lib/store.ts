// Application state shared by the panels and views, with change listeners.

import type { Hub, HubEvent } from "./api";

export type Tab = "layout" | "3d" | "setup" | "run" | "results" | "circuit" | "history";

export interface Selection { reference: string; description: string; bbox?: { min: number[]; max: number[] }; data?: any }

class Store {
  hub!: Hub;
  project = "";
  info: any = null;
  layout: any = null;
  stack: any = null;
  sims: any[] = [];
  runs: any[] = [];
  jobs: any[] = [];
  hidden = new Set<string>();
  tab: Tab = "layout";
  sim = "";
  selection: Selection | null = null;
  connected = false;
  theme: "dark" | "light" = (localStorage.getItem("gamma-theme") as any) || (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
  layoutVersion = 0;
  simVersion = 0;
  status = "";
  private listeners = new Map<string, Set<(d?: any) => void>>();

  on(topic: string, f: (d?: any) => void) {
    if (!this.listeners.has(topic)) this.listeners.set(topic, new Set());
    this.listeners.get(topic)!.add(f);
    return () => this.listeners.get(topic)!.delete(f);
  }
  emit(topic: string, d?: any) {
    this.listeners.get(topic)?.forEach((f) => { try { f(d); } catch (e) { console.error(e); } });
  }
  p(rest = "") { return `/api/p/${this.project}${rest}`; }

  async refreshInfo() { this.info = await this.hub.get(this.p()); this.emit("info"); }
  async refreshLayout() {
    try {
      const v = await this.hub.get(this.p("/layout"));
      this.layout = v.layout; this.stack = v.stack;
    } catch (e: any) { this.layout = null; this.status = e.message; }
    this.layoutVersion++;
    this.emit("layout");
  }
  async refreshSims() {
    this.sims = await this.hub.get(this.p("/sims"));
    if (!this.sim || !this.sims.find((s) => s.name === this.sim)) this.sim = this.sims[0]?.name || "";
    this.simVersion++;
    this.emit("sims");
  }
  async refreshRuns() { this.runs = await this.hub.get(this.p("/runs")); this.emit("runs"); }
  async refreshJobs() { this.jobs = (await this.hub.get("/api/jobs")).filter((j: any) => j.project === this.project); this.emit("jobs"); }

  setTab(t: Tab) { this.tab = t; this.emit("tab", t); }
  select(s: Selection | null, publish = true) {
    this.selection = s;
    this.emit("selection", s);
    if (publish) this.hub.post(this.p("/selection"), s ?? null).catch(() => {});
  }
  setTheme(t: "dark" | "light") {
    this.theme = t;
    document.documentElement.dataset.theme = t;
    localStorage.setItem("gamma-theme", t);
    this.emit("theme");
  }
  handle(e: HubEvent) {
    this.emit("event", e);
    switch (e.kind) {
      case "layout.changed": case "stack.changed": this.refreshLayout(); break;
      case "sim.changed": case "project.changed": this.refreshSims(); this.refreshInfo(); break;
      case "sim.queued": case "sim.done": this.refreshJobs(); this.refreshRuns(); break;
      case "results.changed": this.refreshRuns(); break;
      case "selection": if (e.actor !== "person") { this.selection = e.data; this.emit("selection", e.data); } break;
    }
  }
}

export const store = new Store();

export function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

export function rgb(hex: string): [number, number, number] {
  const h = hex.replace("#", "");
  return [parseInt(h.slice(0, 2), 16) / 255, parseInt(h.slice(2, 4), 16) / 255, parseInt(h.slice(4, 6), 16) / 255];
}
