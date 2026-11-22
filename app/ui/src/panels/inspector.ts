// The selected object: what it is, its parameters, and actions.

import { store } from "../lib/store";
import { el, fmt } from "../lib/util";

export class Inspector {
  root = el("div", { class: "inspector", "aria-live": "polite" });
  constructor() { store.on("selection", () => this.render()); this.render(); }
  render() {
    const s = store.selection;
    if (!s) {
      this.root.replaceChildren(el("h2", {}, store.info?.name ?? "gamma"), el("p", {}, store.info?.description || "Select a component, port, layer or monitor in any view to see it here and to ask the agent about it."));
      return;
    }
    const kv: [string, string][] = [];
    const d = s.data;
    if (d && typeof d === "object") {
      if (d.kind === "source" || d.kind === "monitor" || d.kind === "structure") {
        kv.push(["type", d.type]);
        kv.push(["min (µm)", d.min.map((x: number) => fmt(x * 1e6, 3)).join(", ")]);
        kv.push(["max (µm)", d.max.map((x: number) => fmt(x * 1e6, 3)).join(", ")]);
        if (d.medium) kv.push(["medium", d.medium]);
        for (const [k, v] of Object.entries(d.params || {})) if (!["type", "name", "min", "max", "position", "axis"].includes(k) && typeof v !== "object") kv.push([k, String(v)]);
        if (d.params?.freqs) kv.push(["frequencies", Array.isArray(d.params.freqs) ? String(d.params.freqs.length) : String(d.params.freqs.count)]);
      } else if ("orientation" in d) {
        kv.push(["x, y (µm)", `${fmt(d.x, 4)}, ${fmt(d.y, 4)}`], ["width (µm)", fmt(d.width, 4)], ["facing", `${fmt(d.orientation, 0)}°`]);
        if (d.layer) kv.push(["layer", `${d.layer[0]}/${d.layer[1]}`]);
      } else if ("polygons" in d && "layer" in d) {
        kv.push(["polygons", String(d.polygons)]);
        if (d.material) kv.push(["material", d.material]);
        if (d.zmin !== null && d.zmin !== undefined) kv.push(["z (µm)", `${fmt(d.zmin)} – ${fmt(d.zmax)}`]);
      } else if ("status" in d) {
        kv.push(["status", d.status], ["device", `${d.host ? d.host + " " : ""}${d.device}`]);
        if (d.stats) kv.push(["wall time (s)", fmt(d.stats.seconds, 1)], ["Gcells/s", fmt(d.stats.mcells_per_s / 1000, 2)]);
        if (d.commit) kv.push(["commit", String(d.commit).slice(0, 10)]);
      }
    }
    if (s.bbox && !kv.length) kv.push(["size (µm)", `${fmt(s.bbox.max[0] - s.bbox.min[0], 3)} × ${fmt(s.bbox.max[1] - s.bbox.min[1], 3)}`]);
    const [kind, ...rest] = s.reference.split(":");
    const id = rest.join(":");
    const names: Record<string, string> = { poly: "Polygon", port: "Port", layer: "Layer", cell: "Cell", sim: "Simulation", monitor: "Monitor", source: "Source", run: "Run", point: "Point", structure: "Structure", file: "File" };
    const title = kind === "poly" ? `Polygon ${id.split("#")[1]} on ${id.split("#")[0]}` : `${names[kind] ?? kind} ${kind === "monitor" || kind === "source" || kind === "run" ? id.split("/").slice(1).join("/") : id}`;
    this.root.replaceChildren(
      el("h2", {}, title),
      el("div", { class: "ref" }, s.reference),
      el("p", {}, s.description),
      kv.length ? el("dl", { class: "kv" }, ...kv.flatMap(([k, v]) => [el("dt", {}, k), el("dd", { title: v }, v)])) : "",
      el("div", { class: "actions" },
        el("button", { class: "link accent", onclick: () => store.emit("ask", s) }, "Ask the agent about this"),
        s.bbox ? el("button", { class: "link", onclick: () => { store.setTab("layout"); store.emit("zoom-to", s.bbox); } }, "Show in layout") : null,
        el("button", { class: "link", onclick: () => navigator.clipboard?.writeText(s.reference) }, "Copy reference"),
        el("button", { class: "link", onclick: () => store.select(null) }, "Clear")));
  }
}
