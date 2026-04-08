#!/bin/sh
set -eu

OWNER="JosunLP"
REPO="vectizeit"
PROJECT_NAME="vectizeit"
PRIMARY_BINARY="trace"
ALIAS_BINARY="vectizeit"
MODE="install"
VERSION="latest"
INSTALL_DIR=""

usage() {
  cat <<'EOF'
Usage: install.sh [--install|--update|--uninstall] [--version <version>] [--install-dir <dir>]

Examples:
  curl -fsSL https://raw.githubusercontent.com/JosunLP/vectizeit/main/install.sh | sh
  curl -fsSL https://raw.githubusercontent.com/JosunLP/vectizeit/main/install.sh | sh -s -- --update
  curl -fsSL https://raw.githubusercontent.com/JosunLP/vectizeit/main/install.sh | sh -s -- --uninstall
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --install)
      MODE="install"
      ;;
    --update)
      MODE="update"
      ;;
    --uninstall)
      MODE="uninstall"
      ;;
    --version)
      shift
      VERSION="${1:-}"
      if [ -z "$VERSION" ]; then
        echo "[vectizeit] --version requires a value." >&2
        exit 1
      fi
      ;;
    --install-dir)
      shift
      INSTALL_DIR="${1:-}"
      if [ -z "$INSTALL_DIR" ]; then
        echo "[vectizeit] --install-dir requires a value." >&2
        exit 1
      fi
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "[vectizeit] Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

if [ -z "$INSTALL_DIR" ]; then
  if [ -n "${VECTIZEIT_INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$VECTIZEIT_INSTALL_DIR"
  elif [ -d "/usr/local/bin" ] && [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
  else
    INSTALL_DIR="${HOME}/.local/bin"
  fi
fi

case "$(uname -s)" in
  Linux)
    OS="linux"
    ;;
  Darwin)
    OS="darwin"
    ;;
  *)
    echo "[vectizeit] Unsupported operating system. Use install.ps1 on Windows or install from npm." >&2
    exit 1
    ;;
esac

case "$(uname -m)" in
  x86_64|amd64)
    ARCH="x64"
    ;;
  arm64|aarch64)
    ARCH="arm64"
    ;;
  *)
    echo "[vectizeit] Unsupported CPU architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

TARGET=""
ARCHIVE_EXT="tar.gz"

case "$OS:$ARCH" in
  linux:x64)
    TARGET="x86_64-unknown-linux-musl"
    ;;
  darwin:x64)
    TARGET="x86_64-apple-darwin"
    ;;
  darwin:arm64)
    TARGET="aarch64-apple-darwin"
    ;;
  *)
    echo "[vectizeit] No prebuilt release is available for $OS / $ARCH." >&2
    exit 1
    ;;
esac

ASSET_NAME="${PROJECT_NAME}-${TARGET}.${ARCHIVE_EXT}"
CHECKSUMS_NAME="checksums.txt"

release_url() {
  asset="$1"
  if [ "$VERSION" = "latest" ]; then
    printf 'https://github.com/%s/%s/releases/latest/download/%s' "$OWNER" "$REPO" "$asset"
  else
    case "$VERSION" in
      v*) tag="$VERSION" ;;
      *) tag="v$VERSION" ;;
    esac
    printf 'https://github.com/%s/%s/releases/download/%s/%s' "$OWNER" "$REPO" "$tag" "$asset"
  fi
}

download_file() {
  url="$1"
  output="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$output" "$url"
  else
    echo "[vectizeit] curl or wget is required to download release assets." >&2
    exit 1
  fi
}

sha256_file() {
  file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$file" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    echo "[vectizeit] sha256sum or shasum is required to verify downloads." >&2
    exit 1
  fi
}

verify_checksum() {
  archive="$1"
  checksum_file="$2"
  expected="$(awk -v asset="$ASSET_NAME" '$2 == asset || $NF == asset { print $1; exit }' "$checksum_file")"
  if [ -z "$expected" ]; then
    echo "[vectizeit] Could not find a checksum for $ASSET_NAME." >&2
    exit 1
  fi

  actual="$(sha256_file "$archive")"
  if [ "$actual" != "$expected" ]; then
    echo "[vectizeit] Checksum verification failed for $ASSET_NAME." >&2
    echo "[vectizeit] Expected: $expected" >&2
    echo "[vectizeit] Actual:   $actual" >&2
    exit 1
  fi
}

install_file() {
  source_path="$1"
  destination_path="$2"
  if command -v install >/dev/null 2>&1; then
    install "$source_path" "$destination_path"
  else
    cp "$source_path" "$destination_path"
    chmod 755 "$destination_path"
  fi
}

remove_installed_files() {
  rm -f "$INSTALL_DIR/$PRIMARY_BINARY" "$INSTALL_DIR/$ALIAS_BINARY"
  if [ -d "$INSTALL_DIR" ] && [ -z "$(ls -A "$INSTALL_DIR" 2>/dev/null)" ]; then
    rmdir "$INSTALL_DIR" 2>/dev/null || true
  fi
}

if [ "$MODE" = "uninstall" ]; then
  remove_installed_files
  echo "[vectizeit] Removed $PRIMARY_BINARY and $ALIAS_BINARY from $INSTALL_DIR."
  exit 0
fi

TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t vectizeit)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

ARCHIVE_PATH="$TMP_DIR/$ASSET_NAME"
CHECKSUMS_PATH="$TMP_DIR/$CHECKSUMS_NAME"
EXTRACT_DIR="$TMP_DIR/extract"
mkdir -p "$EXTRACT_DIR" "$INSTALL_DIR"

printf '[vectizeit] %s %s for %s...\n' "$( [ "$MODE" = "update" ] && printf 'Updating' || printf 'Installing' )" "$PRIMARY_BINARY" "$TARGET"
download_file "$(release_url "$ASSET_NAME")" "$ARCHIVE_PATH"
download_file "$(release_url "$CHECKSUMS_NAME")" "$CHECKSUMS_PATH"
verify_checksum "$ARCHIVE_PATH" "$CHECKSUMS_PATH"

tar -xzf "$ARCHIVE_PATH" -C "$EXTRACT_DIR"
BINARY_PATH="$(find "$EXTRACT_DIR" -type f -name "$PRIMARY_BINARY" | head -n 1)"
if [ -z "$BINARY_PATH" ]; then
  echo "[vectizeit] The downloaded archive did not contain $PRIMARY_BINARY." >&2
  exit 1
fi

install_file "$BINARY_PATH" "$INSTALL_DIR/$PRIMARY_BINARY"
rm -f "$INSTALL_DIR/$ALIAS_BINARY"
if ln -s "$PRIMARY_BINARY" "$INSTALL_DIR/$ALIAS_BINARY" 2>/dev/null; then
  :
else
  install_file "$INSTALL_DIR/$PRIMARY_BINARY" "$INSTALL_DIR/$ALIAS_BINARY"
fi

printf '[vectizeit] Installed %s and %s in %s\n' "$PRIMARY_BINARY" "$ALIAS_BINARY" "$INSTALL_DIR"
case ":$PATH:" in
  *":$INSTALL_DIR:"*)
    ;;
  *)
    printf '[vectizeit] Add %s to your PATH if it is not already available in new shells.\n' "$INSTALL_DIR"
    ;;
esac
