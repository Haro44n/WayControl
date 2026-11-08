#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cargo fetch --locked --manifest-path "$project_root/Cargo.toml"
cd "$project_root/packaging/arch"
exec makepkg "$@"
