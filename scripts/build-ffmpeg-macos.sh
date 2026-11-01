#!/usr/bin/env bash
# Build the pinned LGPL-only macOS arm64 ffmpeg sidecar.
#
# This script is intentionally invoked through scripts/fetch-ffmpeg.mjs so the
# Tauri build and release workflow share one cache/stamp path. All downloadable
# inputs are pinned by scripts/ffmpeg.lock.json and verified before use.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOCK_PATH="$ROOT/scripts/ffmpeg.lock.json"
TARGET="macos-arm64"
STAMP_TEMPLATE=""
OUTPUT=""

usage() {
  echo "usage: $0 --lock <path> --stamp-template <path> --output <path> [--target macos-arm64]" >&2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lock)
      LOCK_PATH="$2"
      shift 2
      ;;
    --stamp-template)
      STAMP_TEMPLATE="$2"
      shift 2
      ;;
    --output)
      OUTPUT="$2"
      shift 2
      ;;
    --target)
      TARGET="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$STAMP_TEMPLATE" || -z "$OUTPUT" ]]; then
  usage
  exit 2
fi
if [[ "$TARGET" != "macos-arm64" ]]; then
  echo "unsupported target: $TARGET" >&2
  exit 2
fi
if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "macOS arm64 host required; got $(uname -s)/$(uname -m)" >&2
  exit 1
fi
for tool in node shasum tar curl make clang pkg-config; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "missing build dependency: $tool" >&2
    exit 1
  fi
done
if [[ ! -f "$LOCK_PATH" || ! -f "$STAMP_TEMPLATE" ]]; then
  echo "lock or stamp template not found" >&2
  exit 1
fi

json_get() {
  node -e '
    const fs = require("node:fs");
    const [file, path] = process.argv.slice(1);
    let value = JSON.parse(fs.readFileSync(file, "utf8"));
    for (const key of path.split(".")) value = value?.[key];
    if (value === undefined || value === null) process.exit(2);
    process.stdout.write(String(value));
  ' "$LOCK_PATH" "$TARGET.$1"
}

json_array() {
  node -e '
    const fs = require("node:fs");
    const [file, path] = process.argv.slice(1);
    let value = JSON.parse(fs.readFileSync(file, "utf8"));
    for (const key of path.split(".")) value = value?.[key];
    if (!Array.isArray(value) || value.some((item) => typeof item !== "string")) process.exit(2);
    for (const item of value) console.log(item);
  ' "$LOCK_PATH" "$TARGET.$1"
}

require_field() {
  local value
  value="$(json_get "$1")" || {
    echo "lock field missing: $TARGET.$1" >&2
    exit 1
  }
  printf '%s' "$value"
}

FFMPEG_URL="$(require_field ffmpeg.url)"
FFMPEG_SHA256="$(require_field ffmpeg.sha256)"
FFMPEG_SIZE="$(require_field ffmpeg.archiveSize)"
FFMPEG_ARCHIVE="$(require_field ffmpeg.archive)"
FFMPEG_DIRECTORY="$(require_field ffmpeg.directory)"
LAME_URL="$(require_field lame.url)"
LAME_SHA256="$(require_field lame.sha256)"
LAME_SIZE="$(require_field lame.archiveSize)"
LAME_ARCHIVE="$(require_field lame.archive)"
LAME_DIRECTORY="$(require_field lame.directory)"
DEPLOYMENT_TARGET="$(require_field deploymentTarget)"

if [[ ! "$FFMPEG_SHA256" =~ ^[0-9a-f]{64}$ || ! "$LAME_SHA256" =~ ^[0-9a-f]{64}$ ]]; then
  echo "source SHA256 in lock is invalid" >&2
  exit 1
fi

CACHE_ROOT="$ROOT/.cache/ffmpeg/$TARGET"
SOURCE_CACHE="$CACHE_ROOT/sources"
BUILD_KEY="$(shasum -a 256 "$STAMP_TEMPLATE" | awk '{print $1}')"
BUILD_ROOT="$CACHE_ROOT/build/$BUILD_KEY"
DEPS_PREFIX="$BUILD_ROOT/deps"
INSTALL_PREFIX="$BUILD_ROOT/install"
FFMPEG_SRC="$BUILD_ROOT/$FFMPEG_DIRECTORY"
LAME_SRC="$BUILD_ROOT/$LAME_DIRECTORY"

if [[ "${BILIDOWN_FFMPEG_FORCE:-0}" == "1" ]]; then
  rm -rf "$BUILD_ROOT"
fi
mkdir -p "$SOURCE_CACHE" "$BUILD_ROOT"

file_size() {
  stat -f %z "$1"
}

sha256_file() {
  shasum -a 256 "$1" | awk '{print $1}'
}

ensure_archive() {
  local url="$1"
  local sha="$2"
  local size="$3"
  local archive="$4"
  local dest="$SOURCE_CACHE/$archive"
  local partial="$dest.partial.$$"

  if [[ -f "$dest" ]]; then
    if [[ "$(file_size "$dest")" == "$size" && "$(sha256_file "$dest")" == "$sha" ]]; then
      echo "[build-ffmpeg-macos] reuse $(basename "$dest")"
      return
    fi
    echo "[build-ffmpeg-macos] cached archive failed integrity check; downloading again" >&2
  fi

  rm -f "$partial"
  echo "[build-ffmpeg-macos] download $url"
  if ! curl -fL --retry 3 --retry-delay 2 --silent --show-error --output "$partial" "$url"; then
    rm -f "$partial"
    return 1
  fi
  if [[ "$(file_size "$partial")" != "$size" ]]; then
    echo "archive size mismatch for $archive: expected $size, got $(file_size "$partial")" >&2
    rm -f "$partial"
    return 1
  fi
  if [[ "$(sha256_file "$partial")" != "$sha" ]]; then
    echo "archive SHA256 mismatch for $archive" >&2
    rm -f "$partial"
    return 1
  fi
  mv -f "$partial" "$dest"
}

ensure_archive "$FFMPEG_URL" "$FFMPEG_SHA256" "$FFMPEG_SIZE" "$FFMPEG_ARCHIVE"
ensure_archive "$LAME_URL" "$LAME_SHA256" "$LAME_SIZE" "$LAME_ARCHIVE"

if [[ ! -d "$FFMPEG_SRC" ]]; then
  echo "[build-ffmpeg-macos] extract $FFMPEG_ARCHIVE"
  tar -xJf "$SOURCE_CACHE/$FFMPEG_ARCHIVE" -C "$BUILD_ROOT"
fi
if [[ ! -d "$LAME_SRC" ]]; then
  echo "[build-ffmpeg-macos] extract $LAME_ARCHIVE"
  tar -xzf "$SOURCE_CACHE/$LAME_ARCHIVE" -C "$BUILD_ROOT"
fi
if [[ ! -d "$FFMPEG_SRC" || ! -d "$LAME_SRC" ]]; then
  echo "source archive did not contain the expected directories" >&2
  exit 1
fi

export MACOSX_DEPLOYMENT_TARGET="$DEPLOYMENT_TARGET"
export CC=clang
export CXX=clang++
COMMON_CFLAGS="-arch arm64 -mmacosx-version-min=$DEPLOYMENT_TARGET -O2"
COMMON_LDFLAGS="-arch arm64 -mmacosx-version-min=$DEPLOYMENT_TARGET"

if [[ ! -f "$DEPS_PREFIX/lib/libmp3lame.a" ]]; then
  echo "[build-ffmpeg-macos] build static LAME"
  (
    cd "$LAME_SRC"
    ./configure \
      --prefix="$DEPS_PREFIX" \
      --build=aarch64-apple-darwin \
      --host=aarch64-apple-darwin \
      --enable-static \
      --disable-shared \
      CFLAGS="$COMMON_CFLAGS" \
      LDFLAGS="$COMMON_LDFLAGS"
    make -j"$(sysctl -n hw.ncpu)"
    make install
  )
fi
if [[ ! -f "$DEPS_PREFIX/lib/libmp3lame.a" ]]; then
  echo "LAME static library was not produced" >&2
  exit 1
fi

CONFIGURE_FLAGS=()
while IFS= read -r flag; do
  CONFIGURE_FLAGS+=("$flag")
done < <(json_array configure)
if [[ ${#CONFIGURE_FLAGS[@]} -eq 0 ]]; then
  echo "lock configure list is empty" >&2
  exit 1
fi

echo "[build-ffmpeg-macos] configure FFmpeg"
(
  cd "$FFMPEG_SRC"
  PKG_CONFIG_PATH="$DEPS_PREFIX/lib/pkgconfig" \
  PKG_CONFIG_LIBDIR="$DEPS_PREFIX/lib/pkgconfig" \
    ./configure \
      --prefix="$INSTALL_PREFIX" \
      --cc=clang \
      --arch=arm64 \
      --target-os=darwin \
      --extra-cflags="$COMMON_CFLAGS -I$DEPS_PREFIX/include" \
      --extra-ldflags="$COMMON_LDFLAGS -L$DEPS_PREFIX/lib" \
      --pkg-config-flags=--static \
      "${CONFIGURE_FLAGS[@]}"
  echo "[build-ffmpeg-macos] compile FFmpeg"
  make -j"$(sysctl -n hw.ncpu)"
  make install
)

BUILT_BIN="$INSTALL_PREFIX/bin/ffmpeg"
if [[ ! -x "$BUILT_BIN" ]]; then
  echo "FFmpeg build did not produce an executable" >&2
  exit 1
fi
if [[ "$(lipo -archs "$BUILT_BIN" 2>/dev/null || file "$BUILT_BIN")" != *arm64* ]]; then
  echo "built FFmpeg is not arm64" >&2
  exit 1
fi
NON_SYSTEM_DEPS="$(
  otool -L "$BUILT_BIN" |
    tail -n +2 |
    awk '{print $1}' |
    grep -Ev '^(/System/|/usr/lib/)' || true
)"
if [[ -n "$NON_SYSTEM_DEPS" ]]; then
  echo "built FFmpeg links to a non-system library:" >&2
  printf '%s\n' "$NON_SYSTEM_DEPS" >&2
  exit 1
fi

mkdir -p "$(dirname "$OUTPUT")"
TEMP_OUTPUT="$OUTPUT.tmp.$$"
rm -f "$TEMP_OUTPUT"
trap 'rm -f "$TEMP_OUTPUT"' EXIT
cp "$BUILT_BIN" "$TEMP_OUTPUT"
chmod +x "$TEMP_OUTPUT"
mv -f "$TEMP_OUTPUT" "$OUTPUT"
cp "$STAMP_TEMPLATE" "$OUTPUT.build.json"
trap - EXIT

echo "[build-ffmpeg-macos] output -> ${OUTPUT#$ROOT/} ($(du -h "$OUTPUT" | awk '{print $1}'))"
