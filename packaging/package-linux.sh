#!/usr/bin/env bash
set -euo pipefail

# Run from the repository root after cargo build --release --locked.
if [[ ! -f Cargo.toml || ! -x target/release/nodiform ]]; then
    echo "Run this from the repository root after building the release executable." >&2
    exit 1
fi

architecture="$(uname -m)"
output="dist/nodiform-linux-${architecture}.tar.gz"
mkdir -p dist

# GNU tar transforms give the archive one containing directory without staging
# another copy of the binary. FFmpeg and system graphics libraries are not bundled.
tar -czf "$output" \
    --transform='s,^target/release/nodiform$,nodiform,' \
    --transform='s,^,nodiform/,' \
    target/release/nodiform README.md CHANGELOG.md examples docs packaging/nodiform.desktop

echo "Created ${output}"
