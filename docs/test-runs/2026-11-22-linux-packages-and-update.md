# Linux packages and update flow — 2026-11-22

## Packages (packaging/linux/build.sh, Ubuntu 22.04 build container, CUDA-enabled gamma)

| Architecture | AppImage | .deb | .rpm |
|---|---|---|---|
| x86_64 | `gamma_0.1.0_amd64.AppImage` 85 MB | `gamma_0.1.0_amd64.deb` 12 MB | `gamma-0.1.0-1.x86_64.rpm` 12 MB |
| aarch64 | `gamma_0.1.0_aarch64.AppImage` 88 MB | `gamma_0.1.0_arm64.deb` 12 MB | `gamma-0.1.0-1.aarch64.rpm` 12 MB |

The x86_64 build ran on an x86_64 machine, the aarch64 build on a GB10
(aarch64) machine, both in Docker.

## Installing the .deb on a clean Ubuntu 22.04 container

```text
installed: Version: 0.1.0
/usr/bin/gamma
/usr/bin/gamma-app
gamma 0.1.0
{"t":"hello","version":"0.1.0","lightsim":"4ed0d651c9...","host_arch":"linux-x86_64","devices":[{"id":"cpu",...}]}
$ gamma-app --version-info     (virtual display)
{"failed_starts":0,"install_kind":"package","probation":false,"rollback_version":null,"version":"0.1.0"}
```

`install_kind: package`: an apt- or dnf-installed gamma leaves updates to the
package manager.

## Update flow of the AppImage (packaging/linux/test-update.sh, x86_64)

```text
== built 0.9.0
== built 0.9.1
== installed: 0.9.0
$ gamma-app --check-update stable
{"available":true,...,"version":"0.9.1"}
$ gamma-app --self-update stable
{"from":"0.9.0","to":"0.9.1","updated":true}
== after update: 0.9.1, on probation
$ gamma-app --rollback
{"restored":"0.9.0"}
== manual rollback: 0.9.0
gamma: this version did not start properly twice; going back to 0.9.0
== automatic rollback after failed starts: 0.9.0
{"error":"update failed, nothing was replaced: The signature verification failed"}
== tampered signature refused, installation unchanged
ALL UPDATE CHECKS PASSED (Linux x86_64)
```

Not run: the same on aarch64 (the AppImage builds there; the test was not
repeated), and the apt/dnf repositories against a real package host.
