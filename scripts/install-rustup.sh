#!/bin/sh
# docs/testing.md: restores rustup on the self-hosted Apple-silicon runner.
# The download runs only after it matches the pinned SHA-256 (specq repository security).
set -eu
version=1.29.3
sha256=63752a1919a3a4793173ec70f8f212621a7ce38ba6a246095984482f4de20f9f
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
curl --proto '=https' --tlsv1.2 -sSf -o "$dir/rustup-init" \
  "https://static.rust-lang.org/rustup/archive/$version/aarch64-apple-darwin/rustup-init"
(cd "$dir" && echo "$sha256  rustup-init" | shasum -a 256 -c -)
chmod +x "$dir/rustup-init"
"$dir/rustup-init" -y --no-modify-path --default-toolchain stable
