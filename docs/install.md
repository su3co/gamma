# Installing and updating gamma

gamma has two parts in one package: the desktop app (window) and the `gamma`
program (hub, CLI, MCP server, simulation worker). The app starts the hub
itself; the program is also on your PATH for the terminal and for agents.

## macOS (Apple silicon, macOS 13 or later)

1. Open `gamma_<version>_aarch64.dmg` and drag gamma to Applications.
2. Start gamma. The `gamma` program is inside the app; for the terminal:

   ```bash
   sudo ln -sf /Applications/gamma.app/Contents/MacOS/gamma /usr/local/bin/gamma
   ```

**Updates** are automatic: at start-up gamma checks its update feed (Settings
→ Updates: channel stable or beta, automatic check on or off, Check now).
Every update is an archive signed with SU3 Labs' update key; gamma refuses
one whose signature does not verify. Before installing, gamma keeps a copy of
the current version; a new version that fails to start twice in a row is
replaced by the kept one at the next launch, and Settings offers "Go back to
<version>" by hand.

macOS has no NVIDIA GPUs: simulations run on the CPU engine or on remote GPU
hosts ([remote.md](remote.md)).

## Linux (x86_64 and aarch64)

### AppImage (any distribution, updates itself)

```bash
chmod +x gamma_<version>_amd64.AppImage
./gamma_<version>_amd64.AppImage
```

The AppImage updates itself like the macOS app (signed archives, channels,
rollback). Needs WebKitGTK 4.1 (present on current desktop distributions).

### Debian and Ubuntu (apt)

```bash
sudo apt install ./gamma_<version>_amd64.deb        # or arm64
```

With the gamma apt repository configured, `apt upgrade` keeps it current:

```bash
curl -fsSL <repository>/gamma.gpg | sudo tee /usr/share/keyrings/gamma.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/gamma.gpg] <repository>/apt stable main" | sudo tee /etc/apt/sources.list.d/gamma.list
sudo apt update && sudo apt install gamma
```

### Fedora, RHEL, openSUSE (dnf, zypper)

```bash
sudo dnf install ./gamma-<version>-1.x86_64.rpm      # or aarch64
```

Repository: a `/etc/yum.repos.d/gamma.repo` with `baseurl=<repository>/rpm`,
`gpgcheck=1` and `gpgkey=<repository>/gamma.gpg`.

A package installed by apt or dnf does not replace itself: the app reports
that the package manager handles updates.

`<repository>` is the address the release is published at (not yet chosen;
see [release.md](release.md)).

### NVIDIA GPUs

The Linux packages contain the CUDA-enabled `gamma` program. It loads the
NVIDIA driver at run time (CUDA 13, driver 580 or newer) and runs on the CPU
when there is none. `gamma worker info` lists the GPUs it sees.

## GPU servers without a desktop

Copy the headless program (`gamma-linux-x86_64-cuda` or
`gamma-linux-aarch64-cuda` from the release) to the server with
`gamma remote install` from your computer ([remote.md](remote.md)), or put it
anywhere on the server's PATH.

## From source

Requirements: Rust 1.99.1, Node.js 20+, git; on Linux the WebKitGTK 4.1 and
GTK 3 development packages; for NVIDIA GPUs CUDA 13 headers.

```bash
git clone https://github.com/su3co/gamma && cd gamma
cargo build --release -p gamma-cli                 # the gamma program (CPU)
scripts/build-cuda.sh                              # the gamma program with CUDA (Linux)
scripts/package.sh                                 # the desktop app and installers for this platform
packaging/linux/build.sh                           # Linux packages in a container (Docker)
```

## Privacy

gamma sends nothing anywhere by itself except update checks, which can be
turned off. The hub listens on 127.0.0.1 only and requires a token that only
your user can read. Agents use your own credentials; remote hosts use your
own SSH keys.
