#!/usr/bin/env bash
# Keep local development and GitHub Actions on the same native prerequisites.
set -euo pipefail

install=false
desktop=false
for argument in "$@"; do
  case "$argument" in
    --install) install=true ;;
    --print) install=false ;;
    --desktop) desktop=true ;;
    --help|-h)
      printf '%s\n' \
        'Usage: bash scripts/setup-linux.sh [--print|--install] [--desktop]' \
        'Prints the Ubuntu build prerequisites by default; --install installs them.' \
        '--desktop also includes the native WebDriver and headless display tools.'
      exit 0
      ;;
    *) printf 'Unknown argument: %s\n' "$argument" >&2; exit 2 ;;
  esac
done

# /etc/os-release is supplied by the operating system, never the repository.
# shellcheck source=/dev/null
source /etc/os-release
if [[ "${ID:-}" != ubuntu ]]; then
  printf 'This setup script supports Ubuntu 22.04, 24.04 and 26.04.\n' >&2
  exit 1
fi
case "${VERSION_ID:-}" in
  22.04|24.04) webdriver=webkit2gtk-driver ;;
  26.04) webdriver=webkitgtk-webdriver ;;
  *) printf 'Unsupported Ubuntu release: %s\n' "${VERSION_ID:-unknown}" >&2; exit 1 ;;
esac

packages=(
  build-essential ca-certificates curl file git pkg-config libssl-dev
  libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev
  libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev
  patchelf xz-utils zstd
)
if "$desktop"; then
  packages+=("$webdriver" xvfb xauth dbus-daemon dbus-x11)
fi

printf 'Ubuntu %s native prerequisites%s\n' "$VERSION_ID" "$(if "$desktop"; then printf ' and desktop smoke tools'; fi)"
if ! "$install"; then
  printf 'sudo apt-get update\n'
  printf 'sudo apt-get install --no-install-recommends -y'
  printf ' %q' "${packages[@]}"
  printf '\n'
  exit 0
fi

privilege=()
if [[ "$EUID" -ne 0 ]]; then
  privilege=(sudo)
fi
"${privilege[@]}" apt-get update
"${privilege[@]}" env DEBIAN_FRONTEND=noninteractive \
  apt-get install --no-install-recommends -y "${packages[@]}"
