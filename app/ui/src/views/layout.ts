// Layout viewer: polygons triangulated by the hub, drawn with WebGL2 (one
// draw call per layer plus outlines), ports and labels on a 2-D overlay.

import { store, css, rgb } from "../lib/store";
import { el, fitCanvas, fmt } from "../lib/util";

const VS = `#version 300 es
layout(location=0) in vec2 p;
uniform vec2 c; uniform vec2 s;
void main(){ gl_Position = vec4((p - c) * s, 0.0, 1.0); }`;
const FS = `#version 300 es
precision mediump float; uniform vec4 col; out vec4 o; void main(){ o = col; }`;

interface LayerGL { key: string; vao: WebGLVertexArrayObject; n: number; lineVao: WebGLVertexArrayObject; nl: number; color: [number, number, number]; z: number; bufs: WebGLBuffer[] }

export class LayoutView {
  root: HTMLElement;
  canvas = el("canvas", { class: "fill", "aria-label": "Layout view", tabindex: "0" });
  overlay = el("canvas", { class: "fill overlay" });
  gl: WebGL2RenderingContext;
  prog!: WebGLProgram;
  layers: LayerGL[] = [];
  view = { cx: 0, cy: 0, s: 20 };
  version = -1;
  measure: [number, number][] = [];
  measureMode = false;
  hoverText = "";
  private drag: { x: number; y: number; cx: number; cy: number } | null = null;
  private raf = 0;

  constructor() {
    this.root = el("div", { style: { position: "absolute", inset: "0" } });
    const tools = el("div", { class: "toolbar" },
      el("span", { class: "title" }, "Layout"),
      el("button", { class: "link", onclick: () => this.fit(), title: "Fit (F)" }, "Fit"),
      el("button", { class: "link", id: "measure-btn", onclick: () => this.toggleMeasure(), title: "Measure (M)" }, "Measure"),
      el("button", { class: "link", onclick: () => store.hub.post(store.p("/build"), { target: "layout" }).catch(() => {}), title: "Rebuild the layout from its source" }, "Rebuild"),
      el("span", { class: "muted small mono", id: "layout-meta" }),
    );
    this.root.append(this.canvas, this.overlay, tools);
    this.gl = this.canvas.getContext("webgl2", { antialias: true, premultipliedAlpha: false })!;
    this.initGL();
    this.bind();
    new ResizeObserver(() => this.schedule()).observe(this.canvas);
    store.on("layout", () => this.load());
    store.on("theme", () => { this.load(true); });
    store.on("selection", () => this.schedule());
    store.on("hidden", () => this.schedule());
  }

  initGL() {
    const g = this.gl;
    const sh = (t: number, src: string) => { const s = g.createShader(t)!; g.shaderSource(s, src); g.compileShader(s); return s; };
    const p = g.createProgram()!;
    g.attachShader(p, sh(g.VERTEX_SHADER, VS)); g.attachShader(p, sh(g.FRAGMENT_SHADER, FS)); g.linkProgram(p);
    this.prog = p;
  }

  toggleMeasure() { this.measureMode = !this.measureMode; this.measure = []; document.getElementById("measure-btn")!.classList.toggle("accent", this.measureMode); this.schedule(); }

  async load(force = false) {
    if (!force && this.version === store.layoutVersion) return;
    this.version = store.layoutVersion;
    const g = this.gl;
    for (const l of this.layers) { l.bufs.forEach((b) => g.deleteBuffer(b)); g.deleteVertexArray(l.vao); g.deleteVertexArray(l.lineVao); }
    this.layers = [];
    const lay = store.layout;
    const meta = document.getElementById("layout-meta");
    if (!lay) { this.schedule(); return; }
    if (meta) meta.textContent = `${lay.top.length > 42 ? lay.top.slice(0, 40) + "…" : lay.top} · ${lay.polygons} polygons · ${fmt(lay.bbox.max[0] - lay.bbox.min[0], 2)} × ${fmt(lay.bbox.max[1] - lay.bbox.min[1], 2)} µm`;
    const firstLoad = this.layers.length === 0 && this.view.s === 20;
    await Promise.all(lay.layers.map(async (l: any) => {
      const key = `${l.layer[0]}/${l.layer[1]}`;
      const [mesh, outline] = await Promise.all([store.hub.bytes(store.p(`/layout/mesh?layer=${key}`)), store.hub.bytes(store.p(`/layout/outline?layer=${key}`))]);
      const dv = new DataView(mesh);
      const nv = dv.getUint32(0, true), ni = dv.getUint32(4, true);
      const pos = new Float32Array(mesh, 8, nv * 2);
      const idx = new Uint32Array(mesh, 8 + nv * 8, ni);
      const nl = new DataView(outline).getUint32(0, true);
      const lines = new Float32Array(outline, 4, nl * 2);
      const vao = g.createVertexArray()!; g.bindVertexArray(vao);
      const vb = g.createBuffer()!; g.bindBuffer(g.ARRAY_BUFFER, vb); g.bufferData(g.ARRAY_BUFFER, pos, g.STATIC_DRAW);
      g.enableVertexAttribArray(0); g.vertexAttribPointer(0, 2, g.FLOAT, false, 0, 0);
      const ib = g.createBuffer()!; g.bindBuffer(g.ELEMENT_ARRAY_BUFFER, ib); g.bufferData(g.ELEMENT_ARRAY_BUFFER, idx, g.STATIC_DRAW);
      const lvao = g.createVertexArray()!; g.bindVertexArray(lvao);
      const lb = g.createBuffer()!; g.bindBuffer(g.ARRAY_BUFFER, lb); g.bufferData(g.ARRAY_BUFFER, lines, g.STATIC_DRAW);
      g.enableVertexAttribArray(0); g.vertexAttribPointer(0, 2, g.FLOAT, false, 0, 0);
      g.bindVertexArray(null);
      this.layers.push({ key, vao, n: ni, lineVao: lvao, nl, color: rgb(l.color), z: l.zmax ?? 0, bufs: [vb, ib, lb] });
    }));
    this.layers.sort((a, b) => a.z - b.z);
    if (firstLoad || force === false) { if (firstLoad) this.fit(); }
    this.schedule();
  }

  fit() {
    const lay = store.layout; if (!lay) return;
    const r = this.canvas.getBoundingClientRect();
    const b = lay.bbox;
    this.view.cx = (b.min[0] + b.max[0]) / 2; this.view.cy = (b.min[1] + b.max[1]) / 2;
    this.view.s = Math.min((r.width - 120) / Math.max(b.max[0] - b.min[0], 1e-3), (r.height - 140) / Math.max(b.max[1] - b.min[1], 1e-3));
    this.schedule();
  }

  zoomTo(b: { min: number[]; max: number[] }) {
    const r = this.canvas.getBoundingClientRect();
    this.view.cx = (b.min[0] + b.max[0]) / 2; this.view.cy = (b.min[1] + b.max[1]) / 2;
    const w = Math.max(b.max[0] - b.min[0], 0.5), h = Math.max(b.max[1] - b.min[1], 0.5);
    this.view.s = Math.min(this.view.s * 4, Math.min(r.width / (w * 3), r.height / (h * 3)));
    this.schedule();
  }

  toWorld(px: number, py: number): [number, number] {
    const r = this.canvas.getBoundingClientRect();
    return [this.view.cx + (px - r.width / 2) / this.view.s, this.view.cy - (py - r.height / 2) / this.view.s];
  }
  toPx(x: number, y: number): [number, number] {
    const r = this.canvas.getBoundingClientRect();
    return [r.width / 2 + (x - this.view.cx) * this.view.s, r.height / 2 - (y - this.view.cy) * this.view.s];
  }

  bind() {
    const c = this.canvas;
    c.addEventListener("wheel", (e) => {
      e.preventDefault();
      const [wx, wy] = this.toWorld(e.offsetX, e.offsetY);
      const k = Math.exp(-e.deltaY * 0.0015);
      this.view.s *= k;
      this.view.cx = wx - (wx - this.view.cx) / k; this.view.cy = wy - (wy - this.view.cy) / k;
      this.schedule();
    }, { passive: false });
    c.addEventListener("pointerdown", (e) => { c.setPointerCapture(e.pointerId); this.drag = { x: e.offsetX, y: e.offsetY, cx: this.view.cx, cy: this.view.cy }; c.focus(); });
    c.addEventListener("pointermove", (e) => {
      const [wx, wy] = this.toWorld(e.offsetX, e.offsetY);
      store.emit("cursor", `x ${fmt(wx, 3)} µm  y ${fmt(wy, 3)} µm`);
      if (this.drag) {
        this.view.cx = this.drag.cx - (e.offsetX - this.drag.x) / this.view.s;
        this.view.cy = this.drag.cy + (e.offsetY - this.drag.y) / this.view.s;
        this.schedule();
      } else if (this.measureMode && this.measure.length === 1) { this.hoverText = `${wx},${wy}`; this.schedule(); }
    });
    c.addEventListener("pointerup", async (e) => {
      const moved = this.drag && Math.hypot(e.offsetX - this.drag.x, e.offsetY - this.drag.y) > 3;
      this.drag = null;
      if (moved) return;
      const [wx, wy] = this.toWorld(e.offsetX, e.offsetY);
      if (this.measureMode) {
        if (this.measure.length >= 2) this.measure = [];
        this.measure.push(this.snap(wx, wy));
        this.schedule();
        return;
      }
      const tol = 6 / this.view.s;
      try {
        const hit = await store.hub.get(store.p(`/layout/pick?x=${wx}&y=${wy}&tol=${tol}`));
        store.select(hit ? { reference: hit.reference, description: hit.description, bbox: hit.bbox } : { reference: `point:${fmt(wx, 4)},${fmt(wy, 4)}`, description: `point (${fmt(wx, 4)}, ${fmt(wy, 4)}) µm in the layout` });
      } catch { /* no layout */ }
    });
    c.addEventListener("keydown", (e) => {
      const step = 60 / this.view.s;
      if (e.key === "f" || e.key === "F") this.fit();
      else if (e.key === "m" || e.key === "M") this.toggleMeasure();
      else if (e.key === "ArrowLeft") this.view.cx -= step;
      else if (e.key === "ArrowRight") this.view.cx += step;
      else if (e.key === "ArrowUp") this.view.cy += step;
      else if (e.key === "ArrowDown") this.view.cy -= step;
      else if (e.key === "+" || e.key === "=") this.view.s *= 1.25;
      else if (e.key === "-") this.view.s /= 1.25;
      else return;
      e.preventDefault();
      this.schedule();
    });
  }

  /** Snap to a port centre within 8 px. */
  snap(x: number, y: number): [number, number] {
    for (const p of store.layout?.ports ?? []) if (Math.hypot(p.x - x, p.y - y) * this.view.s < 8) return [p.x, p.y];
    return [x, y];
  }

  schedule() { cancelAnimationFrame(this.raf); this.raf = requestAnimationFrame(() => this.draw()); }

  draw() {
    const g = this.gl;
    fitCanvas(this.canvas);
    const [w, h] = fitCanvas(this.overlay);
    g.viewport(0, 0, this.canvas.width, this.canvas.height);
    const bg = rgb(css("--ground"));
    g.clearColor(bg[0], bg[1], bg[2], 1); g.clear(g.COLOR_BUFFER_BIT);
    g.useProgram(this.prog);
    g.enable(g.BLEND); g.blendFunc(g.SRC_ALPHA, g.ONE_MINUS_SRC_ALPHA);
    const uc = g.getUniformLocation(this.prog, "c"), us = g.getUniformLocation(this.prog, "s"), ucol = g.getUniformLocation(this.prog, "col");
    g.uniform2f(uc, this.view.cx, this.view.cy);
    g.uniform2f(us, (2 * this.view.s) / w, (2 * this.view.s) / h);
    const dark = store.theme === "dark";
    for (const l of this.layers) {
      if (store.hidden.has(l.key)) continue;
      g.uniform4f(ucol, l.color[0], l.color[1], l.color[2], dark ? 0.55 : 0.6);
      g.bindVertexArray(l.vao); g.drawElements(g.TRIANGLES, l.n, g.UNSIGNED_INT, 0);
      g.uniform4f(ucol, l.color[0], l.color[1], l.color[2], 1);
      g.bindVertexArray(l.lineVao); g.drawArrays(g.LINES, 0, l.nl);
    }
    g.bindVertexArray(null);
    this.drawOverlay(w, h);
  }

  drawOverlay(w: number, h: number) {
    const c = this.overlay.getContext("2d")!;
    const dpr = window.devicePixelRatio || 1;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.clearRect(0, 0, w, h);
    const lay = store.layout;
    if (!lay) return;
    const acc = css("--accent"), sel = css("--select"), ink2 = css("--ink-2");
    c.font = "11px 'IBM Plex Mono', monospace";
    for (const p of lay.ports) {
      const [x, y] = this.toPx(p.x, p.y);
      const a = (p.orientation * Math.PI) / 180, len = 16;
      const dx = Math.cos(a) * len, dy = -Math.sin(a) * len;
      const isSel = store.selection?.reference === `port:${p.name}`;
      c.strokeStyle = isSel ? sel : acc; c.fillStyle = isSel ? sel : acc; c.lineWidth = isSel ? 2.5 : 1.8;
      // port width as a bar across the waveguide
      const hw = (p.width / 2) * this.view.s, nx = -Math.sin(a), ny = -Math.cos(a);
      c.beginPath(); c.moveTo(x - nx * hw, y - ny * hw); c.lineTo(x + nx * hw, y + ny * hw);
      c.moveTo(x, y); c.lineTo(x + dx, y + dy);
      c.moveTo(x + dx * 0.6 + dy * 0.3, y + dy * 0.6 - dx * 0.3); c.lineTo(x + dx, y + dy); c.lineTo(x + dx * 0.6 - dy * 0.3, y + dy * 0.6 + dx * 0.3);
      c.stroke();
      c.textAlign = "center"; c.fillText(p.name, x + dx * 1.7, y + dy * 1.7 + 4);
    }
    const s = store.selection;
    if (s?.bbox && !s.reference.startsWith("port:")) {
      const [x0, y0] = this.toPx(s.bbox.min[0], s.bbox.max[1]), [x1, y1] = this.toPx(s.bbox.max[0], s.bbox.min[1]);
      c.strokeStyle = sel; c.lineWidth = 2; c.setLineDash([6, 4]);
      c.strokeRect(x0 - 3, y0 - 3, x1 - x0 + 6, y1 - y0 + 6); c.setLineDash([]);
    }
    if (s?.reference.startsWith("point:")) {
      const [px, py] = s.reference.slice(6).split(",").map(Number);
      const [x, y] = this.toPx(px, py);
      c.strokeStyle = sel; c.lineWidth = 1.5; c.beginPath(); c.moveTo(x - 7, y); c.lineTo(x + 7, y); c.moveTo(x, y - 7); c.lineTo(x, y + 7); c.stroke();
    }
    if (this.measure.length) {
      const pts = [...this.measure];
      if (pts.length === 1 && this.hoverText) pts.push(this.hoverText.split(",").map(Number) as [number, number]);
      if (pts.length === 2) {
        const [a, b] = pts.map((p) => this.toPx(p[0], p[1]));
        c.strokeStyle = sel; c.lineWidth = 1.5; c.beginPath(); c.moveTo(a[0], a[1]); c.lineTo(b[0], b[1]); c.stroke();
        const d = Math.hypot(pts[1][0] - pts[0][0], pts[1][1] - pts[0][1]);
        c.fillStyle = sel; c.textAlign = "left";
        c.fillText(`${fmt(d, 4)} µm  (dx ${fmt(pts[1][0] - pts[0][0], 4)}, dy ${fmt(pts[1][1] - pts[0][1], 4)})`, b[0] + 8, b[1] - 8);
      }
    }
    // scale bar
    const target = 110 / this.view.s, pw = Math.pow(10, Math.floor(Math.log10(target)));
    const len = [1, 2, 5, 10].map((m) => m * pw).find((l) => l >= target * 0.6) || pw;
    const px = len * this.view.s;
    c.strokeStyle = ink2; c.fillStyle = ink2; c.lineWidth = 1.5;
    c.beginPath(); c.moveTo(w - 20 - px, h - 22); c.lineTo(w - 20, h - 22); c.stroke();
    c.textAlign = "center"; c.fillText(len >= 1 ? `${fmt(len)} µm` : `${fmt(len * 1000)} nm`, w - 20 - px / 2, h - 28);
  }

  emptyMessage(): HTMLElement | null {
    if (store.layout) return null;
    return el("div", { class: "empty-view" }, el("div", {},
      el("h2", {}, "No layout yet"),
      el("p", {}, store.info?.layout_error || "This project's layout has not been built. Build it from its source, or ask the agent to design one."),
      el("p", {}, el("button", { class: "link accent", onclick: () => store.hub.post(store.p("/build"), { target: "layout" }).catch((e) => alert(e.message)) }, "Build the layout"))));
  }
}
