#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

binary_name="eventlake"
output_path="${EVENTLAKE_PREBUILT_BINARY:-deploy/prebuilt/eventlake}"
cargo_target_dir="${CARGO_TARGET_DIR:-target}"
cargo_args=(build --release --locked)

if [[ -n "${EVENTLAKE_CARGO_TARGET:-}" ]]; then
  cargo_args+=(--target "$EVENTLAKE_CARGO_TARGET")
  built_binary="$cargo_target_dir/$EVENTLAKE_CARGO_TARGET/release/$binary_name"
else
  built_binary="$cargo_target_dir/release/$binary_name"
fi

cargo "${cargo_args[@]}"

install -d -m 0755 "$(dirname "$output_path")"
install -m 0755 "$built_binary" "$output_path"

printf 'Built prebuilt Docker binary: %s\n' "$output_path"
