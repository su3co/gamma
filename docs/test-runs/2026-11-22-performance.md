# Start-up and large layouts — 2026-11-22

Apple M1 Pro, macOS, release build.

## Start-up (tests/ui/smoke.mjs)

```text
hub start 104 ms; dark: project open and layout drawn 1929 ms; light: project open and layout drawn 316 ms
```

"Project open and layout drawn" runs from loading the page in headless
Chromium until the project tree and the layout summary are on screen. The
first run includes Chromium's own cold start, font loading and WebGL shader
compilation; the second run is warm. The desktop window (WebKit) was not
timed.

## One million polygons (crates/gamma-core/examples/stress.rs, then the hub and the browser)

```text
1000000 polygons (89 kB GDS): read+flatten 0.09 s, viewer triangles 0.04 s (64 MB), pick 3.8 ms
hub API: layout summary 0.9 ms; triangle buffer 64 MB in 0.056 s; outline buffer 64 MB in 0.028 s
layout view: 60 zoom steps (one per animation frame) in 975 ms, about 60 frames per second
```

The test layout is 1000 placements of a cell with 1000 rectangles
(2 million triangles), flattened. The frame rate was measured in the
Chromium-based browser pane at 1440×900.
