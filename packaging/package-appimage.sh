#!/usr/bin/env bash
set -euo pipefail

# Run after cargo build --release --locked. No host libraries or drivers are
# bundled: this x86_64 package targets current Bazzite, not older distributions.
repo_root="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
if [[ "$(uname -m)" != x86_64 || ! -x target/release/nodiform ]]; then
    echo "Build target/release/nodiform on x86_64 Linux before packaging." >&2
    exit 1
fi
for tool in curl sha256sum install awk readelf sed timeout; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing packaging dependency: $tool" >&2
        exit 1
    fi
done
if ! readelf -h target/release/nodiform | grep -q 'Machine:.*Advanced Micro Devices X86-64'; then
    echo "The release executable must target x86_64 Linux." >&2
    exit 1
fi

version="$(awk '/^\[package\]/{in_package=1;next} /^\[/{in_package=0} in_package && /^version[[:space:]]*=/{gsub(/\"/, "", $3);print $3;exit}' Cargo.toml)"
if [[ -z "$version" || ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.+-][A-Za-z0-9.+-]+)?$ ]]; then
    echo "Cannot determine the application version from Cargo.toml." >&2
    exit 1
fi
if [[ "$(timeout 10s target/release/nodiform --version)" != "Nodiform $version" ]]; then
    echo "Release executable version does not match Cargo.toml. Rebuild it first." >&2
    exit 1
fi

filename="Nodiform-x86_64.AppImage"
output="$repo_root/dist/$filename"
checksum_output="$repo_root/dist/SHA256SUMS"
for existing in "$output" "$output.zsync" "$checksum_output"; do
    if [[ -e "$existing" || -L "$existing" ]]; then
        echo "Refusing to overwrite $existing. Move the old artifact aside first." >&2
        exit 1
    fi
done
mkdir -p target/appimage-tools dist
build_dir="$(mktemp -d "$repo_root/target/nodiform-appimage.XXXXXX")"
cleanup() {
    # Only remove this invocation's validated, mktemp-created staging directory.
    case "$build_dir" in
        "$repo_root"/target/nodiform-appimage.??????) rm -rf -- "$build_dir" ;;
        *) echo "Unexpected staging path; leaving it untouched: $build_dir" >&2 ;;
    esac
}
trap cleanup EXIT

# Digests published by GitHub's official release-asset API. Fixed release tags,
# not continuous/latest URLs; a changed upstream download fails verification.
# https://api.github.com/repos/AppImage/appimagetool/releases/tags/1.9.1
# https://api.github.com/repos/AppImage/type2-runtime/releases/tags/20251108
appimagetool="$repo_root/target/appimage-tools/appimagetool-1.9.1-x86_64.AppImage"
runtime="$repo_root/target/appimage-tools/runtime-20251108-x86_64"
fetch_verified() {
    local url="$1" destination="$2" digest="$3"
    if [[ ! -f "$destination" ]]; then
        local download_file
        download_file="$(mktemp "$build_dir/download.XXXXXX")"
        curl --fail --location --silent --show-error --retry 3 \
            --proto '=https' --tlsv1.2 "$url" --output "$download_file"
        printf '%s  %s\n' "$digest" "$download_file" | sha256sum --check --status
        install -m 0755 "$download_file" "$destination"
    fi
    if ! printf '%s  %s\n' "$digest" "$destination" | sha256sum --check --status; then
        echo "Checksum mismatch for $destination. Refusing to execute it." >&2
        exit 1
    fi
}
fetch_verified \
    'https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage' \
    "$appimagetool" 'ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0'
fetch_verified \
    'https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64' \
    "$runtime" '2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d'

app_dir="$build_dir/Nodiform.AppDir"
install -D -m 0755 target/release/nodiform "$app_dir/usr/bin/nodiform"
install -m 0755 packaging/AppRun "$app_dir/AppRun"
install -m 0644 packaging/nodiform-appimage.desktop "$app_dir/nodiform.desktop"
install -m 0644 packaging/nodiform.svg "$app_dir/nodiform.svg"
ln -s nodiform.svg "$app_dir/.DirIcon"
install -D -m 0644 packaging/nodiform.svg "$app_dir/usr/share/icons/hicolor/scalable/apps/nodiform.svg"
install -D -m 0644 packaging/nodiform-appimage.desktop "$app_dir/usr/share/applications/nodiform.desktop"
install -D -m 0644 README.md "$app_dir/usr/share/doc/nodiform/README.md"
cp -R examples docs "$app_dir/usr/share/doc/nodiform/"

# Use extract-and-run for the build tool so packaging works without /dev/fuse.
# FFmpeg remains an optional host dependency, required only for recording.
# 'latest' follows published non-prerelease GitHub releases with these assets.
update_info='gh-releases-zsync|bodee-angus|nodiform|latest|Nodiform-x86_64.AppImage.zsync'
(
    # appimagetool writes the .zsync file into its current working directory.
    cd "$build_dir"
    ARCH=x86_64 VERSION="$version" APPIMAGE_EXTRACT_AND_RUN=1 \
        "$appimagetool" --no-appstream --runtime-file "$runtime" \
        --updateinformation "$update_info" "$app_dir" "$filename"
)
if [[ ! -s "$build_dir/$filename.zsync" ]]; then
    echo "appimagetool did not create update metadata. Install zsync and retry." >&2
    exit 1
fi
# Keep this URL tied to the same version as the binary. The embedded update
# channel locates the latest zsync metadata; its payload URL must be immutable.
release_url="https://github.com/bodee-angus/nodiform/releases/download/v$version/$filename"
# Only alter the text header before its first blank line, preserving binary
# rolling-checksum bytes below it (GNU sed supports embedded NUL bytes).
sed -i "1,/^$/s|^URL:.*|URL: $release_url|" "$build_dir/$filename.zsync"
if [[ "$(sed '/^$/q' "$build_dir/$filename.zsync" | awk '/^Filename:/{print $2}')" != "$filename" ]]; then
    echo "Unexpected filename in zsync metadata." >&2
    exit 1
fi
if [[ "$(sed '/^$/q' "$build_dir/$filename.zsync" | awk '/^URL:/{print $2}')" != "$release_url" ]]; then
    echo "The zsync payload URL failed verification." >&2
    exit 1
fi
if [[ "$("$build_dir/$filename" --appimage-updateinformation)" != "$update_info" ]]; then
    echo "The AppImage's embedded update information failed verification." >&2
    exit 1
fi
(
    cd "$build_dir"
    sha256sum "$filename" "$filename.zsync" > SHA256SUMS
)

# Publishing happens only after all validations succeed, without replacing an
# existing release artifact. GNU mv -n also guards against a concurrent build.
for suffix in '' .zsync; do
    mv -n -- "$build_dir/$filename$suffix" "$output$suffix"
    if [[ -e "$build_dir/$filename$suffix" ]]; then
        echo "Another build created $output$suffix; it was not overwritten." >&2
        exit 1
    fi
done
mv -n -- "$build_dir/SHA256SUMS" "$checksum_output"
if [[ -e "$build_dir/SHA256SUMS" ]]; then
    echo "Another build created $checksum_output; it was not overwritten." >&2
    exit 1
fi
printf 'Created %s (version %s), plus .zsync and SHA256SUMS\n' "$output" "$version"
