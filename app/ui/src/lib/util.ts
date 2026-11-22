export function el<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Record<string, any> = {}, ...children: (Node | string | null | undefined | false)[]): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") e.className = v;
    else if (k.startsWith("on") && typeof v === "function") e.addEventListener(k.slice(2), v);
    else if (k === "style" && typeof v === "object") Object.assign(e.style, v);
    else if (k === "html") e.innerHTML = v;
    else e.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children) if (c !== null && c !== undefined && c !== false) e.append(c);
  return e;
}

export function fmt(x: number | null | undefined, digits = 3): string {
  if (x === null || x === undefined || !isFinite(x)) return "–";
  const a = Math.abs(x);
  if (a !== 0 && (a < 1e-3 || a >= 1e5)) return x.toExponential(2);
  return x.toFixed(digits).replace(/\.?0+$/, "") || "0";
}

export function fmtTime(s: number): string {
  if (!isFinite(s)) return "–";
  if (s < 1) return `${(s * 1000).toFixed(0)} ms`;
  if (s < 90) return `${s.toFixed(0)} s`;
  if (s < 5400) return `${(s / 60).toFixed(0)} min`;
  return `${(s / 3600).toFixed(1)} h`;
}

export function fmtBytes(b: number): string {
  if (b < 1e6) return `${(b / 1e3).toFixed(0)} kB`;
  if (b < 1e9) return `${(b / 1e6).toFixed(0)} MB`;
  return `${(b / 1e9).toFixed(2)} GB`;
}

export function um(m: number, d = 3) { return fmt(m * 1e6, d); }

let toastTimer: number | undefined;
export function toast(text: string, kind: "ok" | "warn" | "err" = "warn", ms = 5000) {
  document.querySelector(".toast")?.remove();
  const t = el("div", { class: `toast ${kind}`, role: "status" }, text);
  document.body.append(t);
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => t.remove(), ms);
  t.addEventListener("click", () => t.remove());
}

export function debounce<T extends (...a: any[]) => void>(f: T, ms: number): T {
  let t: number | undefined;
  return ((...a: any[]) => { clearTimeout(t); t = window.setTimeout(() => f(...a), ms); }) as T;
}

/** Keep a canvas's backing store at device pixels. Returns [w, h] in CSS px. */
export function fitCanvas(c: HTMLCanvasElement): [number, number] {
  const r = c.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(r.width * dpr)), h = Math.max(1, Math.round(r.height * dpr));
  if (c.width !== w || c.height !== h) { c.width = w; c.height = h; }
  return [r.width, r.height];
}
