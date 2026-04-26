#!/usr/bin/env sh
set -eu

repo="apolonuss/meterline"
version="latest"
install_dir="${HOME}/.local/bin"
from_source="0"

log() {
  printf 'meterline: %s\n' "$1"
}

usage() {
  cat <<'EOF'
Usage: install.sh [options]

Options:
  --version VERSION   Install a release tag, for example v0.1.0.
  --dir DIR           Install directory. Default: $HOME/.local/bin.
  --from-source       Build with cargo instead of downloading a release asset.
  -h, --help          Show this help.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      version="${2:?missing value for --version}"
      shift 2
      ;;
    --dir)
      install_dir="${2:?missing value for --dir}"
      shift 2
      ;;
    --from-source)
      from_source="1"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

detect_asset() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux) os_part="linux" ;;
    Darwin) os_part="macos" ;;
    *) printf 'Unsupported operating system: %s\n' "$os" >&2; exit 1 ;;
  esac

  case "$arch" in
    x86_64|amd64) arch_part="x86_64" ;;
    arm64|aarch64) arch_part="aarch64" ;;
    *) printf 'Unsupported architecture: %s\n' "$arch" >&2; exit 1 ;;
  esac

  printf 'meterline-%s-%s.tar.gz' "$os_part" "$arch_part"
}

download() {
  url="$1"
  dest="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$dest"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$url" -O "$dest"
  else
    return 1
  fi
}

release_url() {
  asset="$1"
  if [ "$version" = "latest" ]; then
    printf 'https://github.com/%s/releases/latest/download/%s' "$repo" "$asset"
  else
    tag="$version"
    case "$tag" in
      v*) ;;
      *) tag="v$tag" ;;
    esac
    printf 'https://github.com/%s/releases/download/%s/%s' "$repo" "$tag" "$asset"
  fi
}

install_from_release() {
  asset="$(detect_asset)"
  url="$(release_url "$asset")"
  tmp="${TMPDIR:-/tmp}/meterline-install-$$"
  archive="$tmp/$asset"
  extract="$tmp/extract"

  rm -rf "$tmp"
  mkdir -p "$extract" "$install_dir"

  log "downloading $asset"
  if ! download "$url" "$archive"; then
    log "release install unavailable"
    rm -rf "$tmp"
    return 1
  fi

  if ! tar -xzf "$archive" -C "$extract"; then
    log "could not extract release archive"
    rm -rf "$tmp"
    return 1
  fi

  binary="$(find "$extract" -type f -name meterline -perm -111 | head -n 1 || true)"
  if [ -z "$binary" ]; then
    binary="$(find "$extract" -type f -name meterline | head -n 1 || true)"
  fi
  if [ -z "$binary" ]; then
    log "release archive did not contain meterline"
    rm -rf "$tmp"
    return 1
  fi

  cp "$binary" "$install_dir/meterline"
  chmod +x "$install_dir/meterline"
  rm -rf "$tmp"
  return 0
}

install_from_source() {
  if ! command -v cargo >/dev/null 2>&1; then
    cat >&2 <<'EOF'
No Meterline release asset was available, and Rust/Cargo was not found.

Install Rust from https://rustup.rs, then rerun this installer, or download a
prebuilt Meterline release once one is published.
EOF
    exit 1
  fi

  tmp="${TMPDIR:-/tmp}/meterline-cargo-$$"
  rm -rf "$tmp"
  mkdir -p "$tmp" "$install_dir"

  log "building from source with cargo"
  cargo install --git "https://github.com/$repo" --locked --root "$tmp"
  cp "$tmp/bin/meterline" "$install_dir/meterline"
  chmod +x "$install_dir/meterline"
  rm -rf "$tmp"
}

if [ "$from_source" = "0" ] && install_from_release; then
  :
else
  install_from_source
fi

case ":$PATH:" in
  *":$install_dir:"*) ;;
  *)
    log "$install_dir is not on PATH"
    log "add this to your shell profile: export PATH=\"$install_dir:\$PATH\""
    ;;
esac

log "installed to $install_dir/meterline"
log "try: meterline init"
