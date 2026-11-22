// Settings: theme, updates (channel, check, install, roll back), remote
// simulation hosts, agent, about.

import { inTauri } from "./lib/api";
import { store } from "./lib/store";
import { el, toast } from "./lib/util";

async function tauri<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

/** Tell the shell the new version started properly (ends an update's probation). */
export function markHealthy() {
  if (inTauri()) setTimeout(() => tauri("mark_healthy").catch(() => {}), 3000);
}

export async function startupUpdateCheck() {
  if (!inTauri()) return;
  try {
    const cfg = await store.hub.get("/api/config");
    if (!cfg.updates?.check) return;
    const u = await tauri<any>("update_check", { channel: cfg.updates.channel });
    if (u?.available) toast(`gamma ${u.version} is available (${cfg.updates.channel}). Install it from Settings.`, "ok", 10000);
  } catch { /* offline or no feed: silent */ }
}

export async function openSettings(hubInfo: any) {
  const info = hubInfo ?? (await store.hub.get("/api/hub"));
  const cfg = await store.hub.get("/api/config");
  const remotes: any[] = await store.hub.get("/api/remotes");
  const app = inTauri() ? await tauri<any>("app_info").catch(() => null) : null;
  const close = () => box.remove();
  const channel = el("select", {}, el("option", { value: "stable", selected: cfg.updates.channel === "stable" }, "stable"), el("option", { value: "beta", selected: cfg.updates.channel === "beta" }, "beta")) as HTMLSelectElement;
  const check = el("input", { type: "checkbox", checked: cfg.updates.check }) as HTMLInputElement;
  const upd = el("div", { class: "small serif muted" });
  const save = async () => { cfg.updates.channel = channel.value; cfg.updates.check = check.checked; await store.hub.put("/api/config", cfg); };
  channel.onchange = save; check.onchange = save;
  const checkNow = async () => {
    upd.textContent = "Checking…";
    try {
      const u = await tauri<any>("update_check", { channel: channel.value });
      if (!u.available) { upd.textContent = `gamma ${app?.version} is the latest ${channel.value} version.`; return; }
      upd.replaceChildren(el("div", {}, `gamma ${u.version} is available.`), u.notes ? el("pre", { class: "mono small", style: { whiteSpace: "pre-wrap" } }, u.notes) : "",
        el("button", { class: "link accent", onclick: async () => { upd.textContent = "Downloading and verifying the signature…"; try { await tauri("update_install", { channel: channel.value }); } catch (e: any) { upd.textContent = String(e); } } }, "Install and restart"));
    } catch (e: any) { upd.textContent = `Could not check: ${e}`; }
  };
  const rname = el("input", { placeholder: "name" }) as HTMLInputElement;
  const rssh = el("input", { placeholder: "user@host or SSH alias" }) as HTMLInputElement;
  const rlaunch = el("input", { placeholder: "optional job-queue prefix" }) as HTMLInputElement;
  const rlist = el("div");
  const renderRemotes = (list: any[]) => rlist.replaceChildren(...list.map((r) => el("div", { style: { display: "flex", gap: "12px", alignItems: "baseline", margin: "4px 0" } },
    el("span", { style: { fontWeight: "700" } }, r.name), el("span", { class: "mono small muted" }, r.launcher ? "via job queue" : "ssh"),
    el("button", { class: "link small", onclick: async () => { toast(`Testing ${r.name}…`, "ok", 2000); try { const t = await store.hub.post(`/api/remotes/${encodeURIComponent(r.name)}/test`); toast(t.gamma ? `${r.name}: gamma ${t.gamma.version}, ${t.gamma.devices.map((d: any) => d.name.replace("NVIDIA ", "")).join(", ")}` : t.message, t.gamma ? "ok" : "warn", 9000); } catch (e: any) { toast(e.message, "err", 9000); } } }, "Test"),
    el("button", { class: "link small", onclick: async () => { await store.hub.del(`/api/remotes/${encodeURIComponent(r.name)}`); renderRemotes(await store.hub.get("/api/remotes")); } }, "Remove"))));
  renderRemotes(remotes);
  const box = el("div", { class: "modal", role: "dialog", "aria-label": "Settings", onclick: (e: Event) => { if (e.target === box) close(); } }, el("div", {},
    el("h2", {}, "Settings"),
    el("div", { class: "row" }, el("span", {}, "Theme"), el("div", {}, el("button", { class: "link", onclick: () => store.setTheme("dark") }, "Dark"), "  ", el("button", { class: "link", onclick: () => store.setTheme("light") }, "Light"))),
    el("h2", { style: { fontSize: "16px", marginTop: "18px" } }, "Updates"),
    inTauri() ? el("div", {},
      el("div", { class: "row" }, el("span", {}, "Channel"), channel),
      el("div", { class: "row" }, el("span", {}, "Check automatically"), el("label", {}, check, " at start-up")),
      el("div", { class: "row" }, el("span", {}, "Installed"), el("span", { class: "mono" }, `${app?.version ?? "?"}${app?.install_kind ? " · " + app.install_kind : ""}`)),
      app?.install_kind === "package" ? el("p", { class: "small serif muted" }, "Installed from a system package: your package manager updates gamma.") : el("div", { style: { display: "flex", gap: "16px" } },
        el("button", { class: "link accent", onclick: checkNow }, "Check now"),
        app?.rollback_version ? el("button", { class: "link", onclick: async () => { if (confirm(`Go back to gamma ${app.rollback_version}? gamma restarts.`)) await tauri("update_rollback"); } }, `Go back to ${app.rollback_version}`) : null),
      upd) : el("p", { class: "small serif muted" }, "Updates are managed by the desktop app."),
    el("h2", { style: { fontSize: "16px", marginTop: "18px" } }, "Remote simulation hosts"),
    el("p", { class: "small serif muted" }, "Hosts reached over SSH with your own keys. Install gamma on a host with `gamma remote install <name> --binary <linux build>`."),
    rlist,
    el("div", { style: { display: "grid", gridTemplateColumns: "1fr 1.4fr", gap: "6px", marginTop: "8px" } }, rname, rssh, rlaunch,
      el("button", { class: "link accent", onclick: async () => { try { await store.hub.post("/api/remotes", { name: rname.value, ssh: rssh.value, launcher: rlaunch.value || null }); renderRemotes(await store.hub.get("/api/remotes")); rname.value = rssh.value = rlaunch.value = ""; } catch (e: any) { toast(e.message, "err"); } } }, "Add host")),
    el("h2", { style: { fontSize: "16px", marginTop: "18px" } }, "About"),
    el("p", { class: "small mono muted" }, `gamma ${info.version} · lightsim ${String(info.lightsim).slice(0, 10)} · ${info.platform} · ${info.precision}${info.cuda ? " · CUDA" : ""}`),
    el("p", { class: "small serif muted" }, "gamma sends nothing anywhere except update checks, which you can turn off above."),
    el("div", { style: { textAlign: "right", marginTop: "12px" } }, el("button", { class: "link", onclick: close }, "Close"))));
  document.body.append(box);
}
