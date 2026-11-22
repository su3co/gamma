#!/usr/bin/env bash
# Build an apt and a dnf repository from the .deb and .rpm files under DIR:
#   packaging/linux/make-repos.sh <artifacts dir> <out dir>
# With REPO_GPG_KEY_ID set (key in the runner's keyring) the metadata is
# signed: Release.gpg/InRelease for apt, repomd.xml.asc for dnf. Serve
# <out>/apt and <out>/rpm from any static host; see docs/install.md.
set -euo pipefail
SRC=$1; OUT=$2
command -v apt-ftparchive >/dev/null || sudo apt-get install -y apt-utils >/dev/null
command -v createrepo_c >/dev/null || sudo apt-get install -y createrepo-c >/dev/null
mkdir -p "$OUT/apt/pool" "$OUT/rpm"
find "$SRC" -name "*.deb" -exec cp {} "$OUT/apt/pool/" \;
find "$SRC" -name "*.rpm" -exec cp {} "$OUT/rpm/" \;
(
  cd "$OUT/apt"
  for arch in amd64 arm64; do
    mkdir -p "dists/stable/main/binary-$arch"
    apt-ftparchive --arch "$arch" packages pool > "dists/stable/main/binary-$arch/Packages"
    gzip -kf "dists/stable/main/binary-$arch/Packages"
  done
  apt-ftparchive -o APT::FTPArchive::Release::Origin=gamma -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable -o APT::FTPArchive::Release::Components=main \
    -o "APT::FTPArchive::Release::Architectures=amd64 arm64" release dists/stable > dists/stable/Release
  if [ -n "${REPO_GPG_KEY_ID:-}" ]; then
    gpg --batch --yes -u "$REPO_GPG_KEY_ID" -abs -o dists/stable/Release.gpg dists/stable/Release
    gpg --batch --yes -u "$REPO_GPG_KEY_ID" --clearsign -o dists/stable/InRelease dists/stable/Release
  fi
)
createrepo_c -q "$OUT/rpm"
if [ -n "${REPO_GPG_KEY_ID:-}" ]; then gpg --batch --yes -u "$REPO_GPG_KEY_ID" -ab "$OUT/rpm/repodata/repomd.xml"; fi
echo "repositories in $OUT (apt: $(ls "$OUT/apt/pool" | wc -l) packages, rpm: $(ls "$OUT/rpm"/*.rpm 2>/dev/null | wc -l) packages)"
