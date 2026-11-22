# Update flow, macOS arm64 — 2026-11-22

`scripts/test-update.sh` on Apple silicon (macOS 27.2), test signing key,
local feed on 127.0.0.1. Versions 0.9.0 and 0.9.1 were built for the test only.

```text
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
ALL UPDATE CHECKS PASSED (Darwin arm64)
```

Not covered here: the same flow driven from the Settings dialog (the window
calls the same Rust commands), and a Developer ID signed and notarised build
(needs the owner's signing identity).
