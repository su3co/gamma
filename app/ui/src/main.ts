// gamma's window: top navigation, project tree, viewers, inspector and the
// agent conversation, all as clients of the hub.

import { Hub, findHub, inTauri } from "./lib/api";
import { store, type Tab } from "./lib/store";
import { el, toast } from "./lib/util";
import { LayoutView } from "./views/layout";
import { View3D } from "./views/view3d";
import { SetupView } from "./views/setup";
import { RunView } from "./views/run";
import { ResultsView } from "./views/results";
import { CircuitView } from "./views/circuit";
import { HistoryView } from "./views/history";
import { Tree } from "./panels/tree";
import { Inspector } from "./panels/inspector";
import { AgentPanel } from "./panels/agent";
import { openSettings, startupUpdateCheck, markHealthy } from "./settings";

const TABS: [Tab, string][] = [["layout", "Layout"], ["3d", "3-D"], ["setup", "Set-up"], ["run", "Run"], ["results", "Results"], ["circuit", "Circuit"], ["history", "History"]];

async function boot() {
  document.documentElement.dataset.theme = store.theme;
  const app = document.getElementById("app")!;
  let addr;
  try { addr = await findHub(); } catch (e: any) { app.replaceChildren(fatal("gamma cannot reach its hub", e.message)); return; }
  store.hub = new Hub(addr);
  let info: any;
  try { info = await store.hub.get("/api/hub"); } catch (e: any) { app.replaceChildren(fatal("gamma cannot reach its hub", `${e.message}. Start it with \`gamma hub\`.`)); return; }
  const q = new URLSearchParams(location.search);
  const want = q.get("project") || sessionStorage.getItem("gamma-project") || localStorage.getItem("gamma-last-project");
  let project = info.projects.find((p: any) => p.root === want) || null;
  if (!project && want) project = await store.hub.post("/api/open", { path: want }).catch(() => null);
  if (!project) { app.replaceChildren(startScreen(info)); markHealthy(); return; }
  await openProject(project, info);
}

function fatal(title: string, text: string) {
  return el("div", { class: "empty-view", style: { gridRow: "1 / -1" } }, el("div", {}, el("h2", {}, title), el("p", {}, text)));
}

function startScreen(info: any) {
  const path = el("input", { type: "text", placeholder: "/path/to/project", style: { width: "min(460px, 80vw)", background: "var(--ground-2)", border: "1px solid var(--rule)", padding: "8px 10px", borderRadius: "3px", font: "13px var(--mono)" } }) as HTMLInputElement;
  const open = async (p: string) => {
    try { const pr = await store.hub.post("/api/open", { path: p }); await openProject(pr, info); }
    catch (e: any) { toast(e.message, "err", 8000); }
  };
  const create = async () => {
    try { const pr = await store.hub.post("/api/init", { path: path.value }); await openProject(pr, info); }
    catch (e: any) { toast(e.message, "err", 8000); }
  };
  return el("div", { class: "empty-view start", style: { gridRow: "1 / -1" } }, el("div", { style: { textAlign: "left", maxWidth: "640px" } },
    el("h2", { style: { fontSize: "44px" } }, "gamma"),
    el("p", {}, "Design, simulate and test photonic integrated circuits, with lightsim on this machine's CPU, its GPUs, or GPU servers over SSH, and with the agent of your choice working alongside you."),
    el("div", { style: { margin: "22px 0 10px", display: "flex", gap: "16px", alignItems: "center", flexWrap: "wrap" } }, path,
      el("button", { class: "link accent", onclick: () => open(path.value) }, "Open"), el("button", { class: "link", onclick: create }, "Create project here")),
    info.recent?.length ? el("div", {}, el("p", { class: "muted small", style: { margin: "18px 0 6px" } }, "Recent"), ...info.recent.map((r: string) => el("div", {}, el("button", { class: "link mono", onclick: () => open(r) }, r)))) : null));
}

async function openProject(project: any, hubInfo: any) {
  store.project = project.id;
  localStorage.setItem("gamma-last-project", project.root);
  const app = document.getElementById("app")!;
  const nav = el("nav", { class: "tabs", role: "tablist", "aria-label": "Views" });
  const tabs = TABS.map(([t, label], i) => {
    const a = el("a", { role: "tab", tabindex: "0", "aria-selected": "false", "data-tab": t, onclick: () => store.setTab(t), onkeydown: (e: KeyboardEvent) => { if (e.key === "Enter") store.setTab(t); } }, label, el("span", { class: "key" }, String(i + 1)));
    nav.append(a);
    return a;
  });
  const themeBtn = el("button", { class: "link opt", onclick: () => store.setTheme(store.theme === "dark" ? "light" : "dark") }, store.theme === "dark" ? "Light" : "Dark");
  store.on("theme", () => (themeBtn.textContent = store.theme === "dark" ? "Light" : "Dark"));
  const header = el("header", { class: "top" },
    el("div", { class: "name", title: project.root }, el("span", { class: "app" }, "gamma"), project.name),
    nav, el("div", { class: "spacer" }),
    el("div", { class: "links" },
      el("button", { class: "link opt", onclick: () => palette(), title: "Commands (⌘K / Ctrl-K)" }, "Commands"),
      el("button", { class: "link", onclick: runChecks }, "Check"),
      themeBtn,
      el("button", { class: "link", onclick: () => openSettings(hubInfo) }, "Settings")));
  const tree = new Tree();
  const inspector = new Inspector();
  const agent = new AgentPanel();
  const view = el("section", { class: "view", "aria-label": "Viewer" });
  const status = el("footer", { class: "status" });
  const conn = el("span", {}, "●"), cursor = el("span", {}), msg = el("span", { class: "grow" }), job = el("span", {});
  status.append(conn, msg, job, cursor, el("span", {}, `gamma ${hubInfo.version}`));
  app.replaceChildren(header, el("main", { class: "work" }, el("aside", { class: "left" }, tree.root), view, el("aside", { class: "right" }, inspector.root, agent.root)), status);

  const views = { layout: new LayoutView(), "3d": new View3D(), setup: new SetupView(), run: new RunView(), results: new ResultsView(), circuit: new CircuitView(), history: new HistoryView() };
  const show = (t: Tab) => {
    tabs.forEach((a) => a.setAttribute("aria-selected", String(a.dataset.tab === t)));
    const v = (views as any)[t];
    view.replaceChildren(v.root);
    if (t === "layout") { const e = views.layout.emptyMessage(); if (e) view.append(e); views.layout.load(); views.layout.schedule(); }
    if (t === "3d") { views["3d"].load(); views["3d"].schedule(); }
    if (t === "setup") views.setup.load();
    if (t === "run") views.run.render();
    if (t === "results") { views.results.renderRuns(); views.results.load(); }
    if (t === "circuit") views.circuit.load();
    if (t === "history") views.history.load();
    localStorage.setItem("gamma-tab", t);
  };
  store.on("tab", show);
  store.on("layout", () => { if (store.tab === "layout") show("layout"); });
  store.on("zoom-to", (b) => setTimeout(() => views.layout.zoomTo(b), 50));
  store.on("cursor", (t) => (cursor.textContent = t));
  store.on("event", (e) => {
    if (e.kind === "build.started") msg.textContent = `building ${e.data.target}…`;
    else if (e.kind === "build.done") { msg.textContent = e.data.ok ? `built ${e.data.target} in ${e.data.seconds.toFixed(1)} s` : `build of ${e.data.target} failed`; msg.className = `grow ${e.data.ok ? "ok" : "err"}`; if (!e.data.ok) toast(`${e.data.script} failed:\n${e.data.stderr}`, "err", 12000); }
    else if (e.kind === "error") { msg.textContent = e.data.message; msg.className = "grow err"; }
    else if (e.kind === "files.changed") { msg.textContent = `changed: ${e.data.files.slice(0, 3).join(", ")}${e.data.files.length > 3 ? "…" : ""}`; msg.className = "grow"; }
    else if (e.kind === "sim.progress") { const p = e.data.progress; job.textContent = `job ${(100 * p.step / p.steps).toFixed(0)}%`; }
    else if (e.kind === "sim.done") job.textContent = "";
    else if (e.kind === "checkpoint") { msg.textContent = `checkpoint ${e.data.n}: ${e.data.message}`; msg.className = "grow"; }
  });
  store.hub.events(store.project, (e) => store.handle(e), (up) => { store.connected = up; conn.className = up ? "ok" : "err"; conn.title = up ? "connected to the hub" : "reconnecting to the hub"; });
  await Promise.all([store.refreshInfo(), store.refreshLayout(), store.refreshSims(), store.refreshRuns(), store.refreshJobs()]);
  agent.init();
  inspector.render();
  const t0 = (localStorage.getItem("gamma-tab") as Tab) || "layout";
  store.setTab(TABS.find(([t]) => t === t0) ? t0 : "layout");
  window.addEventListener("keydown", (e) => {
    const typing = (e.target as HTMLElement).matches("input, textarea, select");
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); palette(); return; }
    if (typing) return;
    if (/^[1-7]$/.test(e.key) && !e.metaKey && !e.ctrlKey) store.setTab(TABS[Number(e.key) - 1][0]);
    else if (e.key === "Escape") store.select(null);
    else if (e.key === "/") { e.preventDefault(); agent.input.focus(); }
  });
  markHealthy();
  startupUpdateCheck();
}

async function runChecks() {
  try {
    const r = await store.hub.get(store.p("/check"));
    const f = r.findings;
    if (!f.length) { toast("No findings: design rules and consistency checks pass.", "ok"); return; }
    toast(f.slice(0, 8).map((x: any) => `${x.severity}: ${x.message}`).join("\n") + (f.length > 8 ? `\n… ${f.length - 8} more` : ""), r.errors ? "err" : "warn", 15000);
    const first = f.find((x: any) => x.bbox);
    if (first) store.select({ reference: first.object ?? "check", description: first.message, bbox: first.bbox });
  } catch (e: any) { toast(e.message, "err"); }
}

function palette() {
  if (document.querySelector(".palette")) return;
  const cmds: [string, string, () => void][] = [
    ...TABS.map(([t, l], i) => [`Go to ${l}`, String(i + 1), () => store.setTab(t)] as [string, string, () => void]),
    ["Build everything", "", () => store.hub.post(store.p("/build"), { target: "all" }).catch((e) => toast(e.message, "err"))],
    ["Run checks", "", runChecks],
    ["Create checkpoint", "", () => store.hub.post(store.p("/checkpoints"), { message: "Checkpoint" }).then(() => toast("Checkpoint created", "ok", 2500))],
    ["Toggle theme", "", () => store.setTheme(store.theme === "dark" ? "light" : "dark")],
    ["Ask the agent", "/", () => (document.querySelector(".agent textarea") as HTMLElement)?.focus()],
    ["Clear selection", "Esc", () => store.select(null)],
    ["Settings", "", () => openSettings(null)],
    ...store.sims.map((s) => [`Run ${s.name} on the CPU`, "", () => store.hub.post(store.p(`/sims/${encodeURIComponent(s.name)}/run`), { device: "cpu" }).then(() => store.setTab("run")).catch((e) => toast(e.message, "err"))] as [string, string, () => void]),
  ];
  const input = el("input", { placeholder: "Type a command", "aria-label": "Command" }) as HTMLInputElement;
  const list = el("ul", { role: "listbox" });
  let idx = 0;
  const box = el("div", { class: "palette", onclick: (e: Event) => { if (e.target === box) box.remove(); } }, el("div", {}, input, list));
  const render = () => {
    const f = cmds.filter((c) => c[0].toLowerCase().includes(input.value.toLowerCase()));
    idx = Math.min(idx, Math.max(0, f.length - 1));
    list.replaceChildren(...f.map((c, i) => el("li", { role: "option", "aria-selected": String(i === idx), onclick: () => { box.remove(); c[2](); } }, c[0], el("kbd", { class: "mono" }, c[1]))));
    return f;
  };
  input.addEventListener("input", () => { idx = 0; render(); });
  input.addEventListener("keydown", (e) => {
    const f = render();
    if (e.key === "ArrowDown") { idx = Math.min(idx + 1, f.length - 1); render(); e.preventDefault(); }
    else if (e.key === "ArrowUp") { idx = Math.max(idx - 1, 0); render(); e.preventDefault(); }
    else if (e.key === "Enter") { box.remove(); f[idx]?.[2](); }
    else if (e.key === "Escape") box.remove();
  });
  document.body.append(box);
  render();
  input.focus();
}

boot();
export { inTauri };
