// Client of the gamma hub: HTTP for requests, a WebSocket for events.

export interface HubAddress { url: string; token: string }

declare global { interface Window { __TAURI_INTERNALS__?: unknown } }

export const inTauri = () => typeof window !== "undefined" && !!window.__TAURI_INTERNALS__;

export async function findHub(): Promise<HubAddress> {
  const q = new URLSearchParams(location.search);
  if (q.get("project")) sessionStorage.setItem("gamma-project", q.get("project")!);
  if (q.get("token")) {
    const url = q.get("hub") || location.origin;
    sessionStorage.setItem("gamma-hub", JSON.stringify({ url, token: q.get("token") }));
    history.replaceState(null, "", location.pathname);
    return { url, token: q.get("token")! };
  }
  const saved = sessionStorage.getItem("gamma-hub");
  if (saved) return JSON.parse(saved);
  if (inTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<HubAddress>("hub_address");
  }
  throw new Error("No hub address: open gamma from the desktop app, or use the address `gamma open` prints.");
}

export class HubError extends Error {}

export class Hub {
  constructor(public addr: HubAddress) {}
  private h(extra: Record<string, string> = {}) {
    return { Authorization: `Bearer ${this.addr.token}`, "x-gamma-actor": "person", ...extra };
  }
  private async check(r: Response) {
    if (r.ok) return r;
    let msg = `${r.status} ${r.statusText}`;
    try { const j = await r.json(); if (j.error) msg = j.error; } catch { /* not json */ }
    throw new HubError(msg);
  }
  async get<T = any>(path: string): Promise<T> {
    return (await this.check(await fetch(this.addr.url + path, { headers: this.h() }))).json();
  }
  async bytes(path: string): Promise<ArrayBuffer> {
    return (await this.check(await fetch(this.addr.url + path, { headers: this.h() }))).arrayBuffer();
  }
  async text(path: string): Promise<string> {
    return (await this.check(await fetch(this.addr.url + path, { headers: this.h() }))).text();
  }
  async post<T = any>(path: string, body: unknown = {}): Promise<T> {
    return (await this.check(await fetch(this.addr.url + path, { method: "POST", headers: this.h({ "Content-Type": "application/json" }), body: JSON.stringify(body) }))).json();
  }
  async put<T = any>(path: string, body: unknown): Promise<T> {
    return (await this.check(await fetch(this.addr.url + path, { method: "PUT", headers: this.h({ "Content-Type": "application/json" }), body: JSON.stringify(body) }))).json();
  }
  async del<T = any>(path: string): Promise<T> {
    return (await this.check(await fetch(this.addr.url + path, { method: "DELETE", headers: this.h() }))).json();
  }
  imageUrl(path: string) {
    return `${this.addr.url}${path}${path.includes("?") ? "&" : "?"}token=${this.addr.token}`;
  }
  /** Subscribe to a project's events; reconnects and resumes after the last sequence number. */
  events(project: string, onEvent: (e: HubEvent) => void, onState: (up: boolean) => void): () => void {
    let last = -1;
    let ws: WebSocket | null = null;
    let closed = false;
    let delay = 500;
    const open = () => {
      const base = this.addr.url.replace(/^http/, "ws");
      const after = last >= 0 ? `&after=${last}` : "";
      ws = new WebSocket(`${base}/api/p/${project}/ws?token=${this.addr.token}${after}`);
      ws.onopen = () => { delay = 500; onState(true); };
      ws.onmessage = (m) => {
        const e = JSON.parse(m.data) as HubEvent;
        if (e.seq > 0) last = Math.max(last, e.seq);
        onEvent(e);
      };
      ws.onclose = () => {
        onState(false);
        if (!closed) setTimeout(open, delay = Math.min(delay * 2, 8000));
      };
    };
    this.get<HubEvent[]>(`/api/p/${project}/events?limit=1`).then((ev) => { last = ev.length ? ev[ev.length - 1].seq : 0; open(); }).catch(() => open());
    return () => { closed = true; ws?.close(); };
  }
}

export interface HubEvent { seq: number; time: string; actor: string; kind: string; data: any }

export const enc = encodeURIComponent;

export function b64f32(s: string): Float32Array {
  const bin = atob(s);
  const buf = new ArrayBuffer(bin.length);
  const u8 = new Uint8Array(buf);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return new Float32Array(buf);
}

export function b64u16(s: string): Uint16Array {
  const bin = atob(s);
  const buf = new ArrayBuffer(bin.length);
  const u8 = new Uint8Array(buf);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return new Uint16Array(buf);
}
