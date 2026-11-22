// Design history: checkpoints (git commits made by the hub), comparison of
// two checkpoints (files, layout, results) and review comments.

import { store } from "../lib/store";
import { el, fmt, toast } from "../lib/util";

export class HistoryView {
  root = el("div", { class: "page" });
  a: number | null = null;
  b: number | null = null;
  private diffBox = el("div");

  constructor() {
    store.on("event", (e) => { if ((e.kind === "checkpoint" || e.kind === "comment") && store.tab === "history") this.load(); });
  }

  async load() {
    const v = await store.hub.get(store.p("/checkpoints")).catch(() => ({ checkpoints: [], comments: [] }));
    const cks = [...v.checkpoints].reverse();
    const msg = el("input", { type: "text", placeholder: "What changed", style: { width: "320px", background: "var(--ground-2)", border: "1px solid var(--rule)", padding: "5px 8px", borderRadius: "3px" } }) as HTMLInputElement;
    const create = async () => {
      const r = await store.hub.post(store.p("/checkpoints"), { message: msg.value || "Checkpoint" }).catch((e) => { toast(e.message, "err"); return undefined; });
      if (r === null) toast("Nothing changed since the last checkpoint", "warn", 3000);
      this.load();
    };
    const comment = el("input", { type: "text", placeholder: store.selection ? `Comment on ${store.selection.reference}` : "Comment", style: { width: "380px", background: "var(--ground-2)", border: "1px solid var(--rule)", padding: "5px 8px", borderRadius: "3px" } }) as HTMLInputElement;
    const addComment = async () => {
      if (!comment.value.trim()) return;
      await store.hub.post(store.p("/comments"), { text: comment.value, object: store.selection?.reference, checkpoint: cks[0]?.n }).catch((e) => toast(e.message, "err"));
      this.load();
    };
    const rows = cks.map((c: any) => el("tr", {},
      el("td", { class: "num" }, String(c.n)),
      el("td", {}, c.message),
      el("td", { class: "muted" }, c.actor.startsWith("agent:") ? "agent" : c.actor),
      el("td", { class: "num muted" }, (c.time || "").replace("T", " ").slice(0, 16)),
      el("td", {}, el("label", { class: "small" }, el("input", { type: "radio", name: "ck-a", checked: this.a === c.n, onchange: () => { this.a = c.n; this.diff(); } }), " A"), " ",
        el("label", { class: "small" }, el("input", { type: "radio", name: "ck-b", checked: this.b === c.n, onchange: () => { this.b = c.n; this.diff(); } }), " B"))));
    this.root.replaceChildren(
      el("h1", {}, "History"),
      el("p", { class: "lede" }, "A checkpoint is a git commit of the project made by gamma, at the end of every agent turn or when you ask. Pick A and B to compare layouts and results."),
      el("div", { style: { display: "flex", gap: "14px", alignItems: "center", flexWrap: "wrap" } }, msg, el("button", { class: "link accent", onclick: create }, "Create checkpoint")),
      el("h2", {}, "Checkpoints"),
      rows.length ? el("table", { class: "data" }, el("tr", {}, el("th", {}, "#"), el("th", {}, "Message"), el("th", {}, "By"), el("th", {}, "When (UTC)"), el("th", {}, "Compare")), ...rows) : el("p", { class: "serif muted" }, "No checkpoints yet."),
      this.diffBox,
      el("h2", {}, "Review comments"),
      el("div", { style: { display: "flex", gap: "14px", alignItems: "center", flexWrap: "wrap", marginBottom: "10px" } }, comment, el("button", { class: "link accent", onclick: addComment }, "Add comment")),
      ...([...v.comments].reverse().map((c: any) => el("div", { style: { margin: "8px 0" } },
        el("div", { class: "small muted mono" }, `${(c.time || "").replace("T", " ").slice(0, 16)} · ${c.actor.startsWith("agent:") ? "agent" : c.actor}${c.object ? " · " + c.object : ""}${c.checkpoint ? " · checkpoint " + c.checkpoint : ""}`),
        el("div", { class: "serif" }, c.text)))));
    if (this.a !== null) this.diff();
  }

  async diff() {
    if (this.a === null) return;
    const q = this.b !== null ? `a=${this.a}&b=${this.b}` : `a=${this.a}`;
    const d = await store.hub.get(store.p(`/checkpoints/diff?${q}`)).catch((e) => ({ error: e.message }));
    if (d.error) { this.diffBox.replaceChildren(el("p", { class: "serif" }, d.error)); return; }
    const lay = d.layout;
    this.diffBox.replaceChildren(
      el("h2", {}, `Checkpoint ${this.a} → ${this.b ?? "now"}`),
      el("p", { class: "mono small" }, d.files.length ? d.files.join("   ") : "no files changed"),
      lay ? el("table", { class: "data" }, el("tr", {}, el("th", {}, "Layer"), el("th", {}, "Added"), el("th", {}, "Removed"), el("th", {}, "Area added µm²"), el("th", {}, "Area removed µm²")),
        ...lay.layers.map((l: any) => el("tr", {}, el("td", { class: "mono" }, `${l.layer[0]}/${l.layer[1]}`), el("td", { class: "num" }, String(l.added.length)), el("td", { class: "num" }, String(l.removed.length)), el("td", { class: "num" }, fmt(l.added_area, 3)), el("td", { class: "num" }, fmt(l.removed_area, 3)))))
        : el("p", { class: "serif muted" }, d.layout_note || "No layout to compare."),
      lay && (lay.ports_moved.length || lay.ports_added.length || lay.ports_removed.length) ? el("p", { class: "small" }, `ports moved: ${lay.ports_moved.join(", ") || "–"}; added: ${lay.ports_added.join(", ") || "–"}; removed: ${lay.ports_removed.join(", ") || "–"}`) : "",
      ...d.results.map((r: any) => el("div", {}, el("h2", {}, `Results: ${r.sim}`),
        el("table", { class: "data" }, el("tr", {}, el("th", {}, "Metric"), el("th", {}, "A"), el("th", {}, "B"), el("th", {}, "Change")),
          ...r.metrics.filter((m: any) => !m.metric.startsWith("run.")).map((m: any) => el("tr", {}, el("td", { class: "mono small" }, m.metric), el("td", { class: "num" }, fmt(m.before, 4)), el("td", { class: "num" }, fmt(m.after, 4)), el("td", { class: "num" }, fmt(m.change, 4))))))));
  }
}
