// The conversation with the agent (Claude Code, Codex) working in the project.

import { store } from "../lib/store";
import { el, toast } from "../lib/util";

export class AgentPanel {
  root = el("div", { class: "agent" });
  log = el("div", { class: "log", "aria-live": "polite", "aria-label": "Conversation" });
  input = el("textarea", { placeholder: "Ask the agent to design, simulate or explain…", title: "Enter sends, Shift-Enter starts a new line", "aria-label": "Message to the agent" }) as HTMLTextAreaElement;
  chips = el("div", { class: "chips" });
  status = el("span", { class: "muted small" });
  provider = el("select", { "aria-label": "Agent", style: { background: "var(--ground-2)", border: "1px solid var(--rule)", borderRadius: "3px", font: "12px var(--mono)" } }) as HTMLSelectElement;
  refs: { reference: string; description: string }[] = [];
  private live: HTMLElement | null = null;
  private seen = new Set<number>();

  constructor() {
    const form = el("form", {}, this.chips, this.input, el("div", { style: { display: "flex", justifyContent: "space-between", alignItems: "center" } }, this.status, el("button", { class: "link accent", type: "submit" }, "Send")));
    form.addEventListener("submit", (e) => { e.preventDefault(); this.send(); });
    this.input.addEventListener("keydown", (e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); this.send(); } });
    this.root.append(el("div", { class: "head" }, el("h2", {}, "Agent"), this.provider, el("span", { class: "grow" }),
      el("button", { class: "link small", onclick: () => this.start(), title: "Start a new session" }, "New session"),
      el("button", { class: "link small", onclick: () => store.hub.post(store.p("/agent/stop")).catch(() => {}) }, "Stop")), this.log, form);
    store.on("event", (e) => this.onEvent(e));
    store.on("ask", (s) => { this.addRef(s); this.input.focus(); });
  }

  async init() {
    const v = await store.hub.get(store.p("/agent")).catch(() => null);
    const prov = v?.providers ?? [];
    this.provider.replaceChildren(...prov.map((p: any) => el("option", { value: p.id, disabled: !p.installed }, `${p.name}${p.installed ? "" : " (not installed)"}`)));
    const inst = prov.find((p: any) => p.installed);
    if (inst) this.provider.value = inst.id;
    this.status.textContent = v?.session ? `${v.session.provider} session ${v.session.busy ? "working" : "ready"}` : inst ? "no session yet" : "no agent CLI found on PATH";
    const past = await store.hub.get(store.p("/events?kinds=agent.message,agent.tool,agent.started,agent.result&limit=200")).catch(() => []);
    this.log.replaceChildren();
    for (const e of past) this.onEvent(e);
    if (!past.length) this.log.append(el("div", { class: "msg system" }, "The agent works in this project's folder with gamma's tools. Its edits show up in the viewers as it makes them; every turn ends in a checkpoint."));
  }

  addRef(s: { reference: string; description: string }) {
    if (!this.refs.find((r) => r.reference === s.reference)) this.refs.push({ reference: s.reference, description: s.description });
    this.renderChips();
  }
  renderChips() {
    this.chips.replaceChildren(...this.refs.map((r) => el("span", { class: "mono", style: { color: "var(--select)" } }, r.reference, " ", el("button", { class: "link small", "aria-label": `Remove ${r.reference}`, onclick: (e: Event) => { e.preventDefault(); this.refs = this.refs.filter((x) => x !== r); this.renderChips(); } }, "×"))));
  }

  async start() {
    try { await store.hub.post(store.p("/agent/start"), { provider: this.provider.value }); this.status.textContent = `${this.provider.value} session started`; }
    catch (e: any) { toast(e.message, "err", 8000); }
  }

  async send() {
    let text = this.input.value.trim();
    if (!text) return;
    if (this.refs.length) text += "\n\nSelected in gamma:\n" + this.refs.map((r) => `- ${r.reference}: ${r.description}`).join("\n");
    this.input.value = ""; this.refs = []; this.renderChips();
    try {
      const st = await store.hub.get(store.p("/agent"));
      if (!st.session) await store.hub.post(store.p("/agent/start"), { provider: this.provider.value });
      await store.hub.post(store.p("/agent/send"), { text });
      this.status.textContent = "working…";
    } catch (e: any) { toast(e.message, "err", 8000); }
  }

  private msg(cls: string, text: string) {
    const m = el("div", { class: `msg ${cls}` });
    // object references become chips that select the object
    const parts = text.split(/((?:port|poly|layer|cell|monitor|source|sim|point|run):[\w./#,:-]+)/g);
    for (const p of parts) {
      if (/^(port|poly|layer|cell|monitor|source|sim|point|run):/.test(p)) m.append(el("span", { class: "refchip", role: "button", tabindex: "0", onclick: () => store.select({ reference: p, description: p }) }, p));
      else m.append(p);
    }
    this.log.append(m);
    this.log.scrollTop = this.log.scrollHeight;
    return m;
  }

  onEvent(e: any) {
    if (e.seq && this.seen.has(e.seq)) return;
    if (e.seq) this.seen.add(e.seq);
    const d = e.data;
    switch (e.kind) {
      case "agent.delta":
        if (!this.live) this.live = this.msg("assistant", "");
        this.live.textContent += d.text; this.log.scrollTop = this.log.scrollHeight; break;
      case "agent.message":
        if (d.role === "assistant" && this.live) { this.live.remove(); this.live = null; }
        this.msg(d.role === "user" ? "user" : d.role, d.text); break;
      case "agent.tool": {
        const name = String(d.name).replace(/^mcp__gamma__/, "");
        const arg = d.input && typeof d.input === "object" ? Object.entries(d.input).filter(([, v]) => typeof v !== "object").map(([k, v]) => `${k}=${String(v).slice(0, 40)}`).join(" ") : "";
        const m = el("div", { class: "msg tool" }, el("b", {}, name), " ", arg);
        this.log.append(m); this.log.scrollTop = this.log.scrollHeight; break;
      }
      case "agent.started": this.msg("system", `${d.provider} session started`); this.status.textContent = `${d.provider} session ready`; break;
      case "agent.result": this.status.textContent = d.is_error ? "the turn ended with an error" : "ready"; if (this.live) { this.live = null; } break;
      case "agent.turn_end": this.status.textContent = "ready"; break;
      case "checkpoint": if (e.actor.startsWith("agent:")) this.msg("system", `checkpoint ${d.n}: ${d.message}`); break;
      case "agent.ended": this.status.textContent = "the agent exited"; break;
    }
  }
}
