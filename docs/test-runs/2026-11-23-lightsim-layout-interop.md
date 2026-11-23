# gamma and lightsim's tool-neutral layout layer — 2026-11-23

gamma at 92f0867, lightsim at 849fd1a (Rust engine pinned in gamma, Python
package installed from the same commit), macOS arm64, CPU.

## A project from a plain GDS file

lightsim keeps copies of gamma's MMI fixture (`mmi.gds`, `mmi.ports.json`,
`stack.toml`) in `python/tests/fixtures/gamma/`. From those files:

```text
gamma init mmi-neutral --gds mmi.gds --stack stack.toml --ports mmi.ports.json
gamma build sim:sparams      # sims/sparams.py calls lightsim.layout.simulation
sim:sparams: ok in 2.3 s
sim:sparams (preview): ok in 1.7 s
gamma setup sparams
sparams: 1258 x 176 x 66 = 14.61 M cells, 288303 steps of 0.040 fs, about 0.68 GB
  source src_o1 (mode)
  monitor m_o1 (mode)
  monitor m_o2 (mode)
  monitor m_o3 (mode)
```

For a run that fits a laptop CPU, the script was switched to 2-D
(`is_3d=False`, effective-index levels), rebuilt and run:

```text
gamma run sparams --device cpu
  step 6804/154797  decay 1.4e-5  425 Mcells/s
done: results/sparams/<run>
gamma export sparams --format touchstone -o neutral.s3p
```

`lightsim.layout.sparameters` on the same three files with `is_3d=False`
(lightsim's own runs, all three sources), against gamma's Touchstone file
read back with `lightsim.SParameters.from_touchstone`:

| Entry at 1550 nm | lightsim | gamma | max \|difference\| over 21 wavelengths |
|---|---|---|---|
| S(o1, o1) | 0.00003, −0.0722 rad | 0.00003, −0.0722 rad | 4.1e-15 |
| S(o2, o1) | 0.49606, −0.6388 rad | 0.49606, −0.6388 rad | 6.0e-14 |
| S(o3, o1) | 0.49655, −0.6386 rad | 0.49655, −0.6386 rad | 5.4e-14 |

(|S|², phase in lightsim's exp(−iωt) convention.) The same comparison with
lightsim's engine at the previous pin (4ed0d651) gave the same figures. The
spec written by lightsim's layout layer carries the new PML key
`"stabilize": true`, which reaches gamma's worker unchanged.

## S-parameter files

The MMI run of the quick start (3-D, 21 wavelengths) exported by gamma as
Touchstone and as INTERCONNECT text, then read with lightsim's
`SParameters` readers:

- `from_touchstone` and `from_lumerical_dat` give the same matrix
  (`allclose`), ports `o1@0`, `o2@0`, `o3@0`; |S21|² = 0.4941 at 1550 nm.
- lightsim's `to_touchstone` and `to_lumerical_dat`, given the data read
  back, write the same numbers as gamma's files (relative 1e-11) and the same
  INTERCONNECT block headers; the Touchstone comments differ only in the
  first line (which program wrote the file) and gamma's title line.
- Touchstone holds the complex conjugate (exp(+jωt)), as lightsim writes
  it; INTERCONNECT text keeps exp(−iωt), with the phase unwrapped over
  frequency.

## CUDA build

The Linux x86_64 CUDA build (`scripts/build-cuda.sh`, CUDA 13) compiles with
lightsim at 849fd1a. The Rust tests on macOS pass, including the check that
gamma's step-by-step runner reproduces lightsim's `run_cpu` bit for bit.
