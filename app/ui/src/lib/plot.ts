// Line plots on a 2-D canvas: axes with nice ticks, optional log y, legend,
// and a crosshair readout under the pointer.

import { css } from "./store";
import { fitCanvas, fmt } from "./util";

export interface Series { name: string; y: ArrayLike<number> }
export interface PlotSpec { x: ArrayLike<number>; series: Series[]; xlabel: string; ylabel: string; logY?: boolean; title?: string }

const SERIES_VARS = ["--accent", "--monitor", "--select", "--warn", "--ink", "--field-lo"];

function ticks(lo: number, hi: number, n: number): number[] {
  const span = Math.max(hi - lo, 1e-300), raw = span / n;
  const p = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = [1, 2, 5, 10].map((m) => m * p).find((s) => s >= raw) || p;
  const out: number[] = [];
  for (let t = Math.ceil(lo / step) * step; t <= hi + step * 1e-9; t += step) out.push(Math.abs(t) < step * 1e-9 ? 0 : t);
  return out;
}

export class Plot {
  spec: PlotSpec | null = null;
  private hover: number | null = null;
  constructor(public canvas: HTMLCanvasElement) {
    canvas.addEventListener("mousemove", (e) => { const r = canvas.getBoundingClientRect(); this.hover = e.clientX - r.left; this.draw(); });
    canvas.addEventListener("mouseleave", () => { this.hover = null; this.draw(); });
    new ResizeObserver(() => this.draw()).observe(canvas);
  }
  set(spec: PlotSpec | null) { this.spec = spec; this.draw(); }
  draw() {
    const c = this.canvas;
    const [w, h] = fitCanvas(c);
    const g = c.getContext("2d")!;
    const dpr = window.devicePixelRatio || 1;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    const s = this.spec;
    if (!s || !s.x.length) return;
    const L = 64, R = w - 14, T = (s.title ? 30 : 16) + 22, B = h - 38;
    const ty = (v: number) => (s.logY ? Math.log10(Math.max(v, 1e-300)) : v);
    let ylo = Infinity, yhi = -Infinity;
    for (const ser of s.series) for (let i = 0; i < ser.y.length; i++) { const v = ser.y[i]; if (isFinite(v) && (!s.logY || v > 0)) { ylo = Math.min(ylo, ty(v)); yhi = Math.max(yhi, ty(v)); } }
    if (!isFinite(ylo)) { ylo = 0; yhi = 1; }
    if (yhi - ylo < 1e-12) { yhi += 0.5; ylo -= 0.5; }
    const pad = 0.06 * (yhi - ylo); ylo -= pad; yhi += pad;
    let xlo = Infinity, xhi = -Infinity;
    for (let i = 0; i < s.x.length; i++) { xlo = Math.min(xlo, s.x[i]); xhi = Math.max(xhi, s.x[i]); }
    if (xhi <= xlo) xhi = xlo + 1;
    const px = (x: number) => L + ((x - xlo) / (xhi - xlo)) * (R - L);
    const py = (y: number) => B - ((ty(y) - ylo) / (yhi - ylo)) * (B - T);
    const rule = css("--rule"), ink3 = css("--ink-3"), ink2 = css("--ink-2");
    g.font = "11px 'IBM Plex Mono', monospace";
    g.lineWidth = 1;
    g.fillStyle = ink3;
    g.strokeStyle = rule;
    g.textAlign = "center";
    for (const t of ticks(xlo, xhi, Math.max(3, Math.floor((R - L) / 90)))) { const x = px(t); g.beginPath(); g.moveTo(x, T); g.lineTo(x, B); g.stroke(); g.fillText(fmt(t), x, B + 15); }
    g.textAlign = "right";
    for (const t of ticks(ylo, yhi, Math.max(3, Math.floor((B - T) / 50)))) { const y = B - ((t - ylo) / (yhi - ylo)) * (B - T); g.beginPath(); g.moveTo(L, y); g.lineTo(R, y); g.stroke(); g.fillText(s.logY ? `1e${t.toFixed(0)}` : fmt(t), L - 6, y + 4); }
    g.fillStyle = ink2;
    g.textAlign = "center";
    g.font = "12px 'IBM Plex Mono', monospace";
    g.fillText(s.xlabel, (L + R) / 2, h - 8);
    g.textAlign = "left";
    if (s.ylabel) g.fillText(s.ylabel, 8, T - 26);
    if (s.title) { g.font = "700 13px Archivo, sans-serif"; g.fillStyle = css("--ink"); g.fillText(s.title, L, 18); }
    s.series.forEach((ser, k) => {
      const col = css(SERIES_VARS[k % SERIES_VARS.length]);
      g.strokeStyle = col; g.lineWidth = 1.8; g.beginPath();
      let pen = false;
      for (let i = 0; i < ser.y.length && i < s.x.length; i++) {
        const v = ser.y[i];
        if (!isFinite(v) || (s.logY && v <= 0)) { pen = false; continue; }
        const X = px(s.x[i]), Y = py(v);
        if (pen) g.lineTo(X, Y); else g.moveTo(X, Y);
        pen = true;
      }
      g.stroke();
      if (s.x.length === 1) { g.fillStyle = col; g.beginPath(); g.arc(px(s.x[0]), py(ser.y[0]), 3, 0, 7); g.fill(); }
      // legend: one row above the plot, right-aligned
      g.font = "11px 'IBM Plex Mono', monospace"; g.textAlign = "right";
      let xr = R;
      for (let j = s.series.length - 1; j > k; j--) xr -= g.measureText(s.series[j].name).width + 26;
      g.fillStyle = col; g.fillRect(xr - g.measureText(ser.name).width - 16, T - 15, 10, 2.5);
      g.fillText(ser.name, xr, T - 10);
    });
    if (this.hover !== null && this.hover > L && this.hover < R) {
      const xv = xlo + ((this.hover - L) / (R - L)) * (xhi - xlo);
      let bi = 0, bd = Infinity;
      for (let i = 0; i < s.x.length; i++) { const d = Math.abs(s.x[i] - xv); if (d < bd) { bd = d; bi = i; } }
      const X = px(s.x[bi]);
      g.strokeStyle = ink3; g.setLineDash([3, 3]); g.beginPath(); g.moveTo(X, T); g.lineTo(X, B); g.stroke(); g.setLineDash([]);
      g.textAlign = "left"; g.fillStyle = css("--ink"); g.font = "11px 'IBM Plex Mono', monospace";
      const lines = [`${fmt(s.x[bi], 4)}`, ...s.series.map((ser) => `${ser.name}: ${fmt(ser.y[bi], 4)}`)];
      const bx = X + 8 > R - 160 ? X - 168 : X + 8;
      g.fillStyle = css("--ground"); g.globalAlpha = 0.9; g.fillRect(bx - 4, T + 4, 164, 15 * lines.length + 6); g.globalAlpha = 1;
      g.fillStyle = css("--ink");
      lines.forEach((l, i) => g.fillText(l, bx, T + 18 + 15 * i));
    }
  }
}

/** A 2-D map with pan and zoom: an image in world coordinates, plus overlays. */
export interface MapOverlay { min: [number, number]; max: [number, number]; label: string; color: string; ref?: string; dashed?: boolean }

export class MapView {
  image: HTMLCanvasElement | null = null;
  extent: [number, number, number, number] = [0, 0, 1, 1]; // x0, y0, x1, y1 (world units)
  overlays: MapOverlay[] = [];
  lines: { u: number[]; v: number[] } | null = null;
  view = { cx: 0.5, cy: 0.5, s: 100 }; // world centre, px per unit
  units = "µm";
  scale = 1e6; // world units shown = m * scale
  onPick: ((x: number, y: number, ov: MapOverlay | null) => void) | null = null;
  onHover: ((x: number, y: number) => void) | null = null;
  highlight: string | null = null;
  /** Pixels on the right covered by a panel; fit() centres in the rest. */
  insetRight = 0;
  private drag: { x: number; y: number; cx: number; cy: number } | null = null;
  constructor(public canvas: HTMLCanvasElement) {
    const c = canvas;
    new ResizeObserver(() => this.draw()).observe(c);
    c.addEventListener("wheel", (e) => {
      e.preventDefault();
      const [wx, wy] = this.toWorld(e.offsetX, e.offsetY);
      const k = Math.exp(-e.deltaY * 0.0015);
      this.view.s *= k;
      this.view.cx = wx - (wx - this.view.cx) / k;
      this.view.cy = wy - (wy - this.view.cy) / k;
      this.draw();
    }, { passive: false });
    c.addEventListener("pointerdown", (e) => { c.setPointerCapture(e.pointerId); this.drag = { x: e.offsetX, y: e.offsetY, cx: this.view.cx, cy: this.view.cy }; });
    c.addEventListener("pointermove", (e) => {
      const [wx, wy] = this.toWorld(e.offsetX, e.offsetY);
      this.onHover?.(wx, wy);
      if (!this.drag) return;
      this.view.cx = this.drag.cx - (e.offsetX - this.drag.x) / this.view.s;
      this.view.cy = this.drag.cy + (e.offsetY - this.drag.y) / this.view.s;
      this.draw();
    });
    c.addEventListener("pointerup", (e) => {
      const moved = this.drag && Math.hypot(e.offsetX - this.drag.x, e.offsetY - this.drag.y) > 3;
      this.drag = null;
      if (!moved && this.onPick) {
        const [wx, wy] = this.toWorld(e.offsetX, e.offsetY);
        const hit = this.overlays.filter((o) => wx >= o.min[0] - 4 / this.view.s && wx <= o.max[0] + 4 / this.view.s && wy >= o.min[1] - 4 / this.view.s && wy <= o.max[1] + 4 / this.view.s);
        hit.sort((a, b) => (a.max[0] - a.min[0]) * (a.max[1] - a.min[1]) - (b.max[0] - b.min[0]) * (b.max[1] - b.min[1]));
        this.onPick(wx, wy, hit.find((o) => o.ref) || null);
      }
    });
  }
  toWorld(px: number, py: number): [number, number] {
    const r = this.canvas.getBoundingClientRect();
    return [this.view.cx + (px - r.width / 2) / this.view.s, this.view.cy - (py - r.height / 2) / this.view.s];
  }
  toPx(x: number, y: number): [number, number] {
    const r = this.canvas.getBoundingClientRect();
    return [r.width / 2 + (x - this.view.cx) * this.view.s, r.height / 2 - (y - this.view.cy) * this.view.s];
  }
  fit(margin = 60) {
    const r = this.canvas.getBoundingClientRect();
    const [x0, y0, x1, y1] = this.extent;
    const avail = Math.max(r.width - this.insetRight, r.width * 0.4);
    this.view.s = Math.min((avail - 2 * margin) / Math.max(x1 - x0, 1e-12), (r.height - 2 * margin) / Math.max(y1 - y0, 1e-12));
    // centre in the uncovered part
    this.view.cx = (x0 + x1) / 2 + (r.width - avail) / 2 / this.view.s; this.view.cy = (y0 + y1) / 2;
    this.draw();
  }
  draw() {
    const c = this.canvas;
    const [w, h] = fitCanvas(c);
    const g = c.getContext("2d")!;
    const dpr = window.devicePixelRatio || 1;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    if (this.image) {
      const [x0, y0, x1, y1] = this.extent;
      const [a, b] = this.toPx(x0, y1), [cc, d] = this.toPx(x1, y0);
      g.imageSmoothingEnabled = (cc - a) / this.image.width < 2;
      g.drawImage(this.image, a, b, cc - a, d - b);
    }
    if (this.lines && this.view.s > 0) {
      // mesh lines when they are at least 5 px apart
      g.strokeStyle = css("--ink-3"); g.globalAlpha = 0.35; g.lineWidth = 0.5;
      const [x0, y0, x1, y1] = this.extent;
      const u = this.lines.u, v = this.lines.v;
      const minStep = (arr: number[]) => { let m = Infinity; for (let i = 1; i < arr.length; i++) m = Math.min(m, arr[i] - arr[i - 1]); return m; };
      if (minStep(u) * this.view.s > 5 || minStep(v) * this.view.s > 5) {
        g.beginPath();
        for (const x of u) { const [px] = this.toPx(x, 0); if (px < 0 || px > w) continue; const [, pa] = this.toPx(0, y1), [, pb] = this.toPx(0, y0); g.moveTo(px, pa); g.lineTo(px, pb); }
        for (const y of v) { const [, py] = this.toPx(0, y); if (py < 0 || py > h) continue; const [pa] = this.toPx(x0, 0), [pb] = this.toPx(x1, 0); g.moveTo(pa, py); g.lineTo(pb, py); }
        g.stroke();
      }
      g.globalAlpha = 1;
    }
    g.font = "11px 'IBM Plex Mono', monospace";
    for (const o of this.overlays) {
      const [a, b] = this.toPx(o.min[0], o.max[1]), [cc, d] = this.toPx(o.max[0], o.min[1]);
      const sel = o.ref && o.ref === this.highlight;
      g.strokeStyle = sel ? css("--select") : o.color; g.lineWidth = sel ? 2.5 : 1.4;
      g.setLineDash(o.dashed ? [5, 4] : []);
      g.strokeRect(a, b, Math.max(cc - a, 1), Math.max(d - b, 1));
      g.setLineDash([]);
      g.fillStyle = sel ? css("--select") : o.color;
      if (o.label) g.fillText(o.label, a + 3, b - 4);
    }
    // scale bar
    const target = 110 / this.view.s;
    const pw = Math.pow(10, Math.floor(Math.log10(target)));
    const len = [1, 2, 5, 10].map((m) => m * pw).find((l) => l >= target * 0.6) || pw;
    const px = len * this.view.s;
    g.strokeStyle = css("--ink-2"); g.fillStyle = css("--ink-2"); g.lineWidth = 1.5;
    g.beginPath(); g.moveTo(w - 20 - px, h - 22); g.lineTo(w - 20, h - 22); g.stroke();
    g.textAlign = "center"; g.fillText(`${fmt(len)} ${this.units}`, w - 20 - px / 2, h - 28); g.textAlign = "left";
  }
}

export function canvasFromImageData(img: ImageData): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = img.width; c.height = img.height;
  c.getContext("2d")!.putImageData(img, 0, 0);
  return c;
}
