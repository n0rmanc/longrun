#!/bin/sh
set -eu

repository=${LONGRUN_REPOSITORY:-n0rmanc/longrun}
version=${LONGRUN_VERSION:-latest}
install_dir=${LONGRUN_INSTALL_DIR:-"$HOME/.local/bin"}

os=${LONGRUN_OS:-$(uname -s)}
arch=${LONGRUN_ARCH:-$(uname -m)}
case "$os/$arch" in
  Darwin/arm64 | Darwin/aarch64) target=aarch64-apple-darwin ;;
  Darwin/x86_64) target=x86_64-apple-darwin ;;
  Linux/x86_64 | Linux/amd64) target=x86_64-unknown-linux-gnu ;;
  Linux/aarch64 | Linux/arm64) target=aarch64-unknown-linux-gnu ;;
  *)
    printf 'longrun: unsupported platform %s/%s\n' "$os" "$arch" >&2
    exit 1
    ;;
esac

if [ -n "${LONGRUN_BASE_URL:-}" ]; then
  base_url=$LONGRUN_BASE_URL
elif [ "$version" = latest ]; then
  base_url="https://github.com/$repository/releases/latest/download"
else
  case "$version" in
    v*) ;;
    *) version="v$version" ;;
  esac
  base_url="https://github.com/$repository/releases/download/$version"
fi

archive="longrun-$target.tar.gz"
checksum="$archive.sha256"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/longrun-install.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

curl -fsSL "$base_url/$archive" -o "$tmp/$archive"
curl -fsSL "$base_url/$checksum" -o "$tmp/$checksum"
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp" && sha256sum -c "$checksum")
elif command -v shasum >/dev/null 2>&1; then
  (cd "$tmp" && shasum -a 256 -c "$checksum")
else
  printf 'longrun: need sha256sum or shasum to verify the release\n' >&2
  exit 1
fi

mkdir "$tmp/extract"
tar -xzf "$tmp/$archive" -C "$tmp/extract"
if [ ! -f "$tmp/extract/longrun" ]; then
  printf 'longrun: release archive does not contain longrun\n' >&2
  exit 1
fi

mkdir -p "$install_dir"
install -m 0755 "$tmp/extract/longrun" "$install_dir/longrun"
printf 'Installed longrun to %s/longrun\n' "$install_dir"
