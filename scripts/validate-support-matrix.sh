#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

nixpkgs_rev="$(python3 -c 'import json; print(json.load(open("flake.lock"))["nodes"]["nixpkgs"]["locked"]["rev"])')"
if (( $# > 0 )); then
  files=("$@")
else
  files=(docs/support-matrix.examples/*.json)
fi

exec nix shell "github:NixOS/nixpkgs/${nixpkgs_rev}#check-jsonschema" -c \
  check-jsonschema --schemafile docs/support-matrix.schema.json "${files[@]}"
