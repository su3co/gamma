// Colour maps matching gamma-render (intensity, diverging, material) per theme.
type Stop = [number, [number, number, number]];

function lut(stops: Stop[]): Uint8Array {
  const out = new Uint8Array(256 * 3);
  for (let i = 0; i < 256; i++) {
    const t = i / 255;
    let k = 0;
    while (k < stops.length - 2 && t > stops[k + 1][0]) k++;
    const [t0, c0] = stops[k], [t1, c1] = stops[k + 1];
    const u = t1 > t0 ? Math.min(1, Math.max(0, (t - t0) / (t1 - t0))) : 0;
    for (let j = 0; j < 3; j++) out[i * 3 + j] = Math.round(c0[j] + u * (c1[j] - c0[j]));
  }
  return out;
}

const cache = new Map<string, Uint8Array>();
export function colormap(kind: "intensity" | "diverging" | "material", dark: boolean): Uint8Array {
  const key = kind + dark;
  if (cache.has(key)) return cache.get(key)!;
  let m: Uint8Array;
  if (kind === "intensity") m = dark
    ? lut([[0, [12, 14, 18]], [0.25, [58, 33, 110]], [0.55, [176, 58, 91]], [0.8, [240, 140, 60]], [1, [252, 236, 170]]])
    : lut([[0, [255, 255, 255]], [0.2, [230, 205, 160]], [0.5, [214, 110, 70]], [0.8, [110, 40, 110]], [1, [22, 25, 31]]]);
  else if (kind === "diverging") {
    const mid: [number, number, number] = dark ? [12, 14, 18] : [255, 255, 255];
    m = lut([[0, [64, 120, 220]], [0.25, [120, 170, 235]], [0.5, mid], [0.75, [240, 150, 100]], [1, [214, 80, 40]]]);
  } else m = dark ? lut([[0, [21, 25, 32]], [1, [214, 200, 160]]]) : lut([[0, [244, 245, 247]], [1, [60, 66, 80]]]);
  cache.set(key, m);
  return m;
}

/** Paint a row-major [u][v] array into ImageData (u along x, v up). */
export function paintMap(data: Float32Array, nu: number, nv: number, kind: "intensity" | "diverging" | "material", dark: boolean, lo?: number, hi?: number): ImageData {
  const img = new ImageData(nu, nv);
  let mn = Infinity, mx = -Infinity;
  for (const x of data) { if (x < mn) mn = x; if (x > mx) mx = x; }
  if (lo !== undefined) mn = lo;
  if (hi !== undefined) mx = hi;
  const amax = Math.max(Math.abs(mn), Math.abs(mx), 1e-30);
  const map = colormap(kind, dark);
  for (let iu = 0; iu < nu; iu++) for (let iv = 0; iv < nv; iv++) {
    const x = data[iu * nv + iv];
    let t: number;
    if (kind === "diverging") t = 0.5 * (x / amax + 1);
    else if (kind === "intensity") t = mx > 0 ? x / mx : 0;
    else t = mx > mn ? (x - mn) / (mx - mn) : 0;
    const k = Math.max(0, Math.min(255, Math.round(t * 255))) * 3;
    const p = ((nv - 1 - iv) * nu + iu) * 4;
    img.data[p] = map[k]; img.data[p + 1] = map[k + 1]; img.data[p + 2] = map[k + 2]; img.data[p + 3] = 255;
  }
  return img;
}
