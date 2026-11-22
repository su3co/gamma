// 3-D view of the extruded layer stack (WebGL2, orbit camera).

import { store, css, rgb } from "../lib/store";
import { el, fitCanvas } from "../lib/util";

const VS = `#version 300 es
layout(location=0) in vec3 p; layout(location=1) in vec3 n;
uniform mat4 mvp; uniform vec3 zs; out vec3 vn;
void main(){ vn = n; gl_Position = mvp * vec4(p.xy, p.z * zs.x, 1.0); }`;
const FS = `#version 300 es
precision mediump float; in vec3 vn; uniform vec4 col; uniform vec3 light; out vec4 o;
void main(){ float d = abs(dot(normalize(vn), light)); o = vec4(col.rgb * (0.38 + 0.62 * d), col.a); }`;

type M4 = Float32Array;
function persp(fov: number, a: number, n: number, f: number): M4 { const t = 1 / Math.tan(fov / 2); return new Float32Array([t / a, 0, 0, 0, 0, t, 0, 0, 0, 0, (f + n) / (n - f), -1, 0, 0, (2 * f * n) / (n - f), 0]); }
function mul(a: M4, b: M4): M4 { const o = new Float32Array(16); for (let i = 0; i < 4; i++) for (let j = 0; j < 4; j++) { let s = 0; for (let k = 0; k < 4; k++) s += a[k * 4 + j] * b[i * 4 + k]; o[i * 4 + j] = s; } return o; }
function lookAt(e: number[], c: number[], u: number[]): M4 {
  const z = norm(sub(e, c)), x = norm(cross(u, z)), y = cross(z, x);
  return new Float32Array([x[0], y[0], z[0], 0, x[1], y[1], z[1], 0, x[2], y[2], z[2], 0, -dot(x, e), -dot(y, e), -dot(z, e), 1]);
}
const sub = (a: number[], b: number[]) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: number[], b: number[]) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: number[], b: number[]) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const norm = (a: number[]) => { const l = Math.hypot(a[0], a[1], a[2]) || 1; return [a[0] / l, a[1] / l, a[2] / l]; };

export class View3D {
  root: HTMLElement;
  canvas = el("canvas", { class: "fill", "aria-label": "3-D view", tabindex: "0" });
  gl: WebGL2RenderingContext;
  prog!: WebGLProgram;
  meshes: { vao: WebGLVertexArrayObject; n: number; color: [number, number, number]; bufs: WebGLBuffer[] }[] = [];
  cam = { az: -0.6, el: 0.75, dist: 40, tx: 0, ty: 0 };
  zscale = 4;
  version = -1;
  center = [0, 0];
  private drag: { x: number; y: number; az: number; el: number; tx: number; ty: number; pan: boolean } | null = null;
  private raf = 0;

  constructor() {
    this.root = el("div", { style: { position: "absolute", inset: "0" } });
    const z = el("input", { type: "range", min: "1", max: "20", step: "0.5", value: String(this.zscale), "aria-label": "Vertical exaggeration" }) as HTMLInputElement;
    const zl = el("span", { class: "mono small" }, `${this.zscale}×`);
    z.addEventListener("input", () => { this.zscale = Number(z.value); zl.textContent = `${this.zscale}×`; this.schedule(); });
    this.root.append(this.canvas, el("div", { class: "toolbar" }, el("span", { class: "title" }, "3-D"),
      el("button", { class: "link", onclick: () => this.reset() }, "Reset view"),
      el("label", {}, "z exaggeration", z, zl),
      el("span", { class: "muted small" }, "drag to orbit · shift-drag to pan · scroll to zoom")));
    this.gl = this.canvas.getContext("webgl2", { antialias: true })!;
    const g = this.gl;
    const sh = (t: number, s: string) => { const x = g.createShader(t)!; g.shaderSource(x, s); g.compileShader(x); return x; };
    this.prog = g.createProgram()!; g.attachShader(this.prog, sh(g.VERTEX_SHADER, VS)); g.attachShader(this.prog, sh(g.FRAGMENT_SHADER, FS)); g.linkProgram(this.prog);
    new ResizeObserver(() => this.schedule()).observe(this.canvas);
    this.bind();
    store.on("layout", () => this.load());
    store.on("theme", () => this.schedule());
    store.on("hidden", () => this.load(true));
    store.on("selection", () => this.schedule());
  }

  /** A wireframe box around the selected object (layout coordinates, z over the patterned layers). */
  private selectionBox(): Float32Array | null {
    const b = store.selection?.bbox;
    if (!b || !store.layout) return null;
    const zs = store.layout.layers.filter((l: any) => l.zmin !== null && l.zmin !== undefined);
    const z0 = Math.min(0, ...zs.map((l: any) => l.zmin)) - 0.02, z1 = Math.max(0.22, ...zs.map((l: any) => l.zmax)) + 0.02;
    const pad = 0.05;
    const [x0, y0, x1, y1] = [b.min[0] - pad, b.min[1] - pad, b.max[0] + pad, b.max[1] + pad];
    const c = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    const v: number[] = [];
    for (let i = 0; i < 4; i++) {
      const [a, d] = [c[i], c[(i + 1) % 4]];
      v.push(a[0], a[1], z0, d[0], d[1], z0, a[0], a[1], z1, d[0], d[1], z1, a[0], a[1], z0, a[0], a[1], z1);
    }
    return new Float32Array(v);
  }

  reset() { const b = store.layout?.bbox; if (!b) return; this.cam = { az: -0.6, el: 0.75, dist: 1.4 * Math.max(b.max[0] - b.min[0], b.max[1] - b.min[1]), tx: 0, ty: 0 }; this.schedule(); }

  async load(force = false) {
    if (!force && this.version === store.layoutVersion) return;
    const first = this.version === -1;
    this.version = store.layoutVersion;
    const g = this.gl;
    for (const m of this.meshes) { m.bufs.forEach((b) => g.deleteBuffer(b)); g.deleteVertexArray(m.vao); }
    this.meshes = [];
    const lay = store.layout; if (!lay) { this.schedule(); return; }
    this.center = [(lay.bbox.min[0] + lay.bbox.max[0]) / 2, (lay.bbox.min[1] + lay.bbox.max[1]) / 2];
    for (const l of lay.layers) {
      if (l.zmin === null || l.zmin === undefined) continue;
      const key = `${l.layer[0]}/${l.layer[1]}`;
      if (store.hidden.has(key)) continue;
      const buf = await store.hub.bytes(store.p(`/layout/mesh3d?layer=${key}`));
      const dv = new DataView(buf);
      const nv = dv.getUint32(0, true), ni = dv.getUint32(4, true);
      const pos = new Float32Array(buf, 8, nv * 3), nrm = new Float32Array(buf, 8 + nv * 12, nv * 3), idx = new Uint32Array(buf, 8 + nv * 24, ni);
      const vao = g.createVertexArray()!; g.bindVertexArray(vao);
      const b1 = g.createBuffer()!; g.bindBuffer(g.ARRAY_BUFFER, b1); g.bufferData(g.ARRAY_BUFFER, pos, g.STATIC_DRAW); g.enableVertexAttribArray(0); g.vertexAttribPointer(0, 3, g.FLOAT, false, 0, 0);
      const b2 = g.createBuffer()!; g.bindBuffer(g.ARRAY_BUFFER, b2); g.bufferData(g.ARRAY_BUFFER, nrm, g.STATIC_DRAW); g.enableVertexAttribArray(1); g.vertexAttribPointer(1, 3, g.FLOAT, false, 0, 0);
      const b3 = g.createBuffer()!; g.bindBuffer(g.ELEMENT_ARRAY_BUFFER, b3); g.bufferData(g.ELEMENT_ARRAY_BUFFER, idx, g.STATIC_DRAW);
      g.bindVertexArray(null);
      this.meshes.push({ vao, n: ni, color: rgb(l.color), bufs: [b1, b2, b3] });
    }
    if (first) this.reset();
    this.schedule();
  }

  bind() {
    const c = this.canvas;
    c.addEventListener("pointerdown", (e) => { c.setPointerCapture(e.pointerId); this.drag = { x: e.clientX, y: e.clientY, ...this.cam, pan: e.shiftKey || e.button === 1 }; });
    c.addEventListener("pointermove", (e) => {
      if (!this.drag) return;
      const dx = e.clientX - this.drag.x, dy = e.clientY - this.drag.y;
      if (this.drag.pan) { const k = this.cam.dist / 600; this.cam.tx = this.drag.tx - dx * k * Math.cos(this.cam.az) - dy * k * Math.sin(this.cam.az); this.cam.ty = this.drag.ty - dx * k * Math.sin(this.cam.az) + dy * k * Math.cos(this.cam.az); }
      else { this.cam.az = this.drag.az - dx * 0.008; this.cam.el = Math.max(0.05, Math.min(1.55, this.drag.el + dy * 0.008)); }
      this.schedule();
    });
    c.addEventListener("pointerup", () => (this.drag = null));
    c.addEventListener("wheel", (e) => { e.preventDefault(); this.cam.dist *= Math.exp(e.deltaY * 0.0015); this.schedule(); }, { passive: false });
    c.addEventListener("keydown", (e) => {
      if (e.key === "ArrowLeft") this.cam.az -= 0.1; else if (e.key === "ArrowRight") this.cam.az += 0.1;
      else if (e.key === "ArrowUp") this.cam.el = Math.min(1.55, this.cam.el + 0.1); else if (e.key === "ArrowDown") this.cam.el = Math.max(0.05, this.cam.el - 0.1);
      else if (e.key === "f" || e.key === "F") this.reset(); else return;
      e.preventDefault(); this.schedule();
    });
  }

  schedule() { cancelAnimationFrame(this.raf); this.raf = requestAnimationFrame(() => this.draw()); }

  draw() {
    const g = this.gl;
    const [w, h] = fitCanvas(this.canvas);
    g.viewport(0, 0, this.canvas.width, this.canvas.height);
    const bg = rgb(css("--ground"));
    g.clearColor(bg[0], bg[1], bg[2], 1); g.clear(g.COLOR_BUFFER_BIT | g.DEPTH_BUFFER_BIT);
    g.enable(g.DEPTH_TEST);
    g.useProgram(this.prog);
    const c = [this.center[0] + this.cam.tx, this.center[1] + this.cam.ty, 0];
    const eye = [c[0] + this.cam.dist * Math.cos(this.cam.el) * Math.cos(this.cam.az), c[1] + this.cam.dist * Math.cos(this.cam.el) * Math.sin(this.cam.az), this.cam.dist * Math.sin(this.cam.el)];
    const mvp = mul(persp(0.7, w / h, this.cam.dist * 0.01, this.cam.dist * 10), lookAt(eye, c, [0, 0, 1]));
    g.uniformMatrix4fv(g.getUniformLocation(this.prog, "mvp"), false, mvp);
    g.uniform3f(g.getUniformLocation(this.prog, "zs"), this.zscale, 0, 0);
    const l = norm([0.4, -0.5, 0.9]);
    g.uniform3f(g.getUniformLocation(this.prog, "light"), l[0], l[1], l[2]);
    const ucol = g.getUniformLocation(this.prog, "col");
    for (const m of this.meshes) { g.uniform4f(ucol, m.color[0], m.color[1], m.color[2], 1); g.bindVertexArray(m.vao); g.drawElements(g.TRIANGLES, m.n, g.UNSIGNED_INT, 0); }
    const box = this.selectionBox();
    if (box) {
      const sel = rgb(css("--select"));
      const vao = g.createVertexArray()!; g.bindVertexArray(vao);
      const vb = g.createBuffer()!; g.bindBuffer(g.ARRAY_BUFFER, vb); g.bufferData(g.ARRAY_BUFFER, box, g.STREAM_DRAW);
      g.enableVertexAttribArray(0); g.vertexAttribPointer(0, 3, g.FLOAT, false, 0, 0);
      g.disableVertexAttribArray(1); g.vertexAttrib3f(1, 0, 0, 1);
      g.uniform4f(ucol, sel[0] / 0.885, sel[1] / 0.885, sel[2] / 0.885, 1); // undo the shading (normal +z)
      g.disable(g.DEPTH_TEST);
      g.drawArrays(g.LINES, 0, box.length / 3);
      g.enable(g.DEPTH_TEST);
      g.deleteBuffer(vb); g.deleteVertexArray(vao);
    }
    g.bindVertexArray(null);
  }
}
