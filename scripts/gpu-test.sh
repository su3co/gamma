#!/usr/bin/env bash
# GPU checks of a CUDA build of gamma (run on a machine with NVIDIA GPUs):
#  1. the worker reports the GPUs;
#  2. the fixture simulation on every GPU agrees with the CPU run;
#  3. throughput on a 4.6 M-cell photonic simulation (the MMI example spec,
#     if it has been built) on each GPU.
# Prints a Markdown report; exits non-zero on a mismatch.
set -euo pipefail
cd "$(dirname "$0")/.."
G=${GAMMA:-target/release/gamma}
W=$(mktemp -d)
echo "# GPU test, $(date -u +%Y-%m-%d)"
echo
$G worker info | python3 -c "
import json,sys; h=json.loads(sys.stdin.read())
print(f\"gamma {h['version']}, lightsim {h['lightsim'][:10]}, {h['host_arch']}, {h['precision']}\")
for d in h['devices']: print(f\"- {d['id']}: {d['name']}\")"
GPUS=${GAMMA_GPUS:-}
[ -n "$GPUS" ] || GPUS=$($G worker info | python3 -c "import json,sys; print(' '.join(d['id'] for d in json.loads(sys.stdin.read())['devices'] if d['id'].startswith('cuda')))")
[ -n "$GPUS" ] || { echo "no CUDA GPU found"; exit 1; }
run() { $G worker run "$1" --device "$2" --out "$3" --slice none > "$3.log"; tail -1 "$3.log"; }
echo
echo "## Fixture: GPU against CPU"
run tests/fixtures/wire/build/sims/wire.json cpu "$W/cpu.json" >/dev/null
for d in $GPUS; do
  run tests/fixtures/wire/build/sims/wire.json "$d" "$W/$d.json" >/dev/null
  python3 - "$W/cpu.json" "$W/$d.json" "$d" <<'PY'
import json, sys
a, b = (json.load(open(p)) for p in sys.argv[1:3])
fa, fb = a["flux"]["through"], b["flux"]["through"]
err = max(abs(x - y) / max(abs(x), 1e-300) for x, y in zip(fa, fb))
ok = err < 1e-3
print(f"- {sys.argv[3]}: flux relative difference {err:.1e} ({'ok' if ok else 'MISMATCH'})")
sys.exit(0 if ok else 1)
PY
done
SPEC=${GAMMA_PERF_SPEC:-examples/mmi-splitter/build/sims/through.json}
if [ -f "$SPEC" ]; then
  echo
  echo "## Throughput: $(basename "$SPEC")"
  for d in $GPUS; do
    run "$SPEC" "$d" "$W/perf.json" | python3 -c "import json,sys; m=json.loads(sys.stdin.read()); print(f\"- $d: {m['backend']}: {m['steps']} steps in {m['seconds']:.1f} s, {m['mcells_per_s']/1000:.2f} Gcells/s\")"
  done
fi
rm -rf "$W"
