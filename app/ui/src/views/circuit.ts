// Quantum circuits (lightsim-quantum). The library has no stable API yet;
// gamma lists the project's circuits, draws the components and connections
// it can read, and runs them once the engine is installed.

import { store } from "../lib/store";
import { enc } from "../lib/api";
import { el } from "../lib/util";

export class CircuitView {
  root = el("div", { class: "page" });
  async load() {
    const c = await store.hub.get(store.p("/circuits")).catch(() => null);
    const items: HTMLElement[] = [];
    for (const k of c?.circuits ?? []) {
      const v = await store.hub.get(store.p(`/circuits/${enc(k.name)}`)).catch((e) => ({ error: e.message }));
      items.push(el("h2", {}, k.name), el("p", { class: "muted mono small" }, k.file));
      if (v.error) { items.push(el("p", { class: "serif" }, v.error)); continue; }
      if (!v.recognised) { items.push(el("p", { class: "serif" }, `gamma cannot read this circuit's components yet (top-level keys: ${v.keys.join(", ")}).`)); continue; }
      items.push(el("table", { class: "data" }, el("tr", {}, el("th", {}, "Component"), el("th", {}, "Kind")), ...v.components.map((n: any) => el("tr", {}, el("td", { class: "mono" }, n.id), el("td", {}, n.kind)))));
      if (v.connections.length) items.push(el("p", { class: "mono small" }, v.connections.map((x: string[]) => `${x[0]} → ${x[1]}`).join("   ")));
    }
    this.root.replaceChildren(
      el("h1", {}, "Circuit"),
      el("p", { class: "lede" }, "Circuits of quantum light, simulated with lightsim-quantum: sources of squeezed or single photons, linear optics from the S-parameters lightsim computes, detectors."),
      el("p", { class: "serif" }, c?.engine_available ? `Engine: ${c.engine} is installed.` : `lightsim-quantum is not installed on this machine yet, so circuits can be read but not run. gamma will run them through the library as soon as its interface is published.`),
      ...(items.length ? items : [el("p", { class: "serif muted" }, "This project has no circuits. Add one under [[circuit]] in gamma.toml.")]));
  }
}
