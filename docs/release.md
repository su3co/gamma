# Releasing gamma

Releases are built by `.github/workflows/release.yml` from a tag `vX.Y.Z`
(`vX.Y.Z-beta.N` goes to the beta channel). The workflow builds and signs
every package, writes the update feed and a draft GitHub release; publishing
it is a person's decision.

## What the workflow needs (owner's decisions)

| Secret or variable | Purpose | Status |
|---|---|---|
| `MACOS_CERTIFICATE`, `MACOS_CERTIFICATE_PASSWORD` | Developer ID Application certificate (.p12, base64) | to set |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: <company> (<team>)` | to set |
| `APPLE_ID`, `APPLE_APP_PASSWORD`, `APPLE_TEAM_ID` | notarisation | to set |
| `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) | minisign key that signs update archives | to create |
| `UPDATER_PUBLIC_KEY` | its public key, compiled into the app | to create |
| `GAMMA_UPDATE_BASE` (variable) | where feeds are served: `<base>/<channel>/latest.json` | to choose |
| `REPO_GPG_KEY_ID` + key | signs the apt and dnf repository metadata | to create |

The macOS job fails closed when any signing secret is missing. Create the
update key with `npx tauri signer generate -w gamma-updater.key` (keep it
offline; losing it means existing installations can no longer update).

## Steps

1. Update `CHANGELOG.md` (the section of the version becomes the release
   notes and the feed's notes) and the version in `Cargo.toml`.
2. Tag and push: `git tag v0.1.0 && git push origin v0.1.0`.
3. Check the draft release: macOS dmg (stapled, Gatekeeper accepted), Linux
   AppImage/deb/rpm for x86_64 and aarch64, headless `gamma-linux-*-cuda`,
   `update-feed-<version>.tar.gz`, `linux-repos.tar.gz`.
4. Publish: upload the feed folder to `GAMMA_UPDATE_BASE` and the
   repositories to the package host, then publish the GitHub release.

## Channels and rollback

Each channel is a folder with its own `latest.json`. Promoting a beta to
stable is copying its feed entry. To withdraw a bad release, put the previous
version's `latest.json` back: installations that already updated can go back
from Settings, and a version that does not start is rolled back
automatically ([install.md](install.md)).

## Testing the update flow

`scripts/test-update.sh` (macOS) and `packaging/linux/test-update.sh` (Linux,
in the build container) build two versions with a test key, serve a local
feed and run update, manual rollback, automatic rollback after failed starts
and a tampered signature. Records in [test-runs/](test-runs/).

## Self-hosted runners (proposal)

GPU tests and Linux aarch64 builds can run on the owner's machines as
self-hosted GitHub Actions runners (one per GPU machine, labels
`self-hosted, linux, gpu, x64|arm64`), in a container, as an unprivileged
user, registered to this repository only, picking up only `gpu.yml` and
release jobs. The repository is public, so the runners must only take
manually started workflows (never `pull_request` from forks; set "Require
approval for all outside collaborators" for workflow runs). Installing them
needs the owner's approval; until then
`scripts/gpu-test.sh` is run by hand through the machines' job queue and the
results are recorded in `docs/test-runs/`.
