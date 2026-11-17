#!/usr/bin/env bash
# Bundles the release binary with its assets into dist/iron-tide-<version>-<platform>.(tar.gz|zip).
# Usage: tools/package.sh <platform> [binary-path]
set -euo pipefail

platform="${1:?platform name, e.g. linux-x86_64}"
binary="${2:-target/release/iron-tide}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
name="iron-tide-${version}-${platform}"
stage="dist/${name}"

rm -rf "${stage}"
mkdir -p "${stage}"
cp "${binary}" "${stage}/"
cp README.md "${stage}/"
# Bevy looks for assets next to the executable; previews are Blender renders the game never loads.
cp -R assets "${stage}/"
rm -rf "${stage}/assets/previews"

cd dist
case "${platform}" in
  windows-*) 7z a -tzip -bso0 "${name}.zip" "${name}" ;;
  *) tar -czf "${name}.tar.gz" "${name}" ;;
esac
rm -rf "${name}"
ls -lh "${name}".*
