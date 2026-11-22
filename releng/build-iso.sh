#!/usr/bin/env bash
# Build the Arctix ISO with the installer baked in.
#
# Run it as your normal user; it asks for sudo only for mkarchiso:
#   ./build-iso.sh
#
# Steps: test + build arctix-install, copy it into airootfs/usr/local/bin,
# then run mkarchiso with a fresh work dir. The ISO lands in ../out.
# Set ARCTIX_RELEASE=1 for the xz-compressed release image (see profiledef.sh).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(dirname "$HERE")"
WORK="${WORK:-$HOME/arctix-work}"

if [[ $EUID -eq 0 ]]; then
  echo "build-iso: run this as your normal user, it asks for sudo itself" >&2
  exit 1
fi

echo ":: testing and building arctix-install"
(cd "$REPO/installer" && cargo test --quiet && cargo build --release --quiet)
install -Dm755 "$REPO/installer/target/release/arctix-install" "$HERE/airootfs/usr/local/bin/arctix-install"

echo ":: building the ISO (needs sudo)"
sudo rm -rf "$WORK"
sudo --preserve-env=ARCTIX_RELEASE mkarchiso -v -w "$WORK" -o "$REPO/out" "$HERE"
sudo rm -rf "$WORK"

echo ":: done"
ls -lt "$REPO/out" | head -n 3
