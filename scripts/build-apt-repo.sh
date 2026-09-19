#!/usr/bin/env bash
# Build a signed apt repository out of one or more Pomodoro .deb files.
#
#   scripts/build-apt-repo.sh <output-dir> <package.deb>...
#
# The output directory is a complete static site: serve it from any web
# server (the release workflow puts it on GitHub Pages) and
#
#   deb [signed-by=/etc/apt/keyrings/pomodoro.gpg] <url> stable main
#
# is a working apt source. Layout:
#
#   pomodoro.gpg, pomodoro.asc         the public key, binary and armoured
#   pool/main/p/pomodoro/*.deb         the packages
#   dists/stable/...                   the indexes apt reads, and their signature
#
# Signing uses the secret key in the current GnuPG home. Environment:
#
#   APT_SIGNING_KEY_ID       which key to sign with; needed only when the
#                            keyring holds more than one secret key
#   APT_SIGNING_PASSPHRASE   the key's passphrase, if it has one
#   APT_REPO_URL             the address the repository will be served from;
#                            only used for the instructions in index.html
set -euo pipefail

SUITE=stable
COMPONENT=main
DEFAULT_URL=https://smoke-hux.github.io/pomodoro

die() { echo "build-apt-repo: $*" >&2; exit 1; }

[ $# -ge 2 ] || die "usage: $0 <output-dir> <package.deb>..."
for tool in dpkg-deb apt-ftparchive gpg gzip; do
  command -v "$tool" >/dev/null || die "$tool is not installed"
done

out=$1
shift
url=${APT_REPO_URL:-$DEFAULT_URL}
url=${url%/}

# Refuse to sign with a guess. An apt repository signed by the wrong key
# looks fine until every user's `apt update` fails.
key=${APT_SIGNING_KEY_ID:-}
if [ -z "$key" ]; then
  mapfile -t secret_keys < <(gpg --batch --list-secret-keys --with-colons | awk -F: '$1 == "sec" { print $5 }')
  case ${#secret_keys[@]} in
    0) die "no secret key in the GnuPG home; import the signing key first" ;;
    1) key=${secret_keys[0]} ;;
    *) die "several secret keys found; set APT_SIGNING_KEY_ID to the one to sign with" ;;
  esac
fi

gpg_sign=(gpg --batch --yes --local-user "$key")
if [ -n "${APT_SIGNING_PASSPHRASE:-}" ]; then
  gpg_sign+=(--pinentry-mode loopback --passphrase-fd 3)
fi
sign() {
  if [ -n "${APT_SIGNING_PASSPHRASE:-}" ]; then
    "${gpg_sign[@]}" "$@" 3<<<"$APT_SIGNING_PASSPHRASE"
  else
    "${gpg_sign[@]}" "$@"
  fi
}

rm -rf "$out"
mkdir -p "$out"
out=$(cd "$out" && pwd)

# Packages go into the pool under their canonical Debian file name, whatever
# the bundler called them (Tauri writes Pomodoro_0.1.0_amd64.deb).
declare -A archs=()
for deb in "$@"; do
  [ -f "$deb" ] || die "$deb: no such file"
  name=$(dpkg-deb -f "$deb" Package)
  version=$(dpkg-deb -f "$deb" Version)
  arch=$(dpkg-deb -f "$deb" Architecture)
  [ "$name" = pomodoro ] || die "$deb is package '$name', not 'pomodoro'"
  dir="$out/pool/$COMPONENT/${name:0:1}/$name"
  mkdir -p "$dir"
  dest="$dir/${name}_${version}_${arch}.deb"
  [ ! -e "$dest" ] || die "$deb: version $version for $arch was given twice"
  install -m 0644 "$deb" "$dest"
  archs[$arch]=1
done

cd "$out"
for arch in "${!archs[@]}"; do
  bin="dists/$SUITE/$COMPONENT/binary-$arch"
  mkdir -p "$bin"
  apt-ftparchive --arch "$arch" packages pool > "$bin/Packages"
  [ -s "$bin/Packages" ] || die "no packages were indexed for $arch"
  gzip -9 --keep --no-name "$bin/Packages"
done

# Written beside the tree and moved in afterwards, so that the Release file
# does not list a half-written copy of itself.
release=$(mktemp)
trap 'rm -f "$release"' EXIT
apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=Pomodoro \
  -o APT::FTPArchive::Release::Label=Pomodoro \
  -o APT::FTPArchive::Release::Suite="$SUITE" \
  -o APT::FTPArchive::Release::Codename="$SUITE" \
  -o APT::FTPArchive::Release::Architectures="$(printf '%s\n' "${!archs[@]}" | sort | xargs)" \
  -o APT::FTPArchive::Release::Components="$COMPONENT" \
  -o APT::FTPArchive::Release::Description="Pomodoro focus timer" \
  release "dists/$SUITE" > "$release"
install -m 0644 "$release" "dists/$SUITE/Release"

sign --clearsign --output "dists/$SUITE/InRelease" "dists/$SUITE/Release"
sign --armor --detach-sign --output "dists/$SUITE/Release.gpg" "dists/$SUITE/Release"

gpg --batch --yes --export "$key" > pomodoro.gpg
gpg --batch --yes --armor --export "$key" > pomodoro.asc
[ -s pomodoro.gpg ] || die "could not export the public key for $key"

# GitHub Pages runs sites through Jekyll unless told not to.
touch .nojekyll

cat > index.html <<HTML
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Pomodoro apt repository</title>
<style>
  body { font: 16px/1.5 system-ui, sans-serif; max-width: 44rem; margin: 3rem auto; padding: 0 1rem; }
  pre { background: #f3f3f3; padding: 1rem; overflow-x: auto; }
  @media (prefers-color-scheme: dark) { body { background: #161616; color: #eee; } pre { background: #262626; } a { color: #9cf; } }
</style>
<h1>Pomodoro apt repository</h1>
<p>Packages of <a href="https://github.com/smoke-hux/pomodoro">Pomodoro</a>, a local-first focus timer, for Ubuntu 22.04 or newer on x86-64. Add the repository once:</p>
<pre>sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL $url/pomodoro.gpg | sudo tee /etc/apt/keyrings/pomodoro.gpg &gt;/dev/null
echo "deb [signed-by=/etc/apt/keyrings/pomodoro.gpg] $url $SUITE $COMPONENT" | sudo tee /etc/apt/sources.list.d/pomodoro.list
sudo apt update</pre>
<p>Then install, and from then on receive updates with the rest of the system:</p>
<pre>sudo apt install pomodoro</pre>
</html>
HTML

echo "apt repository written to $out, signed by $key:"
find . -type f | sort | sed 's|^\./|  |'
