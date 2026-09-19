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
# The whole repository is built in a temporary directory and moved into
# place only once it is complete and signed, so a run that fails leaves
# whatever was already being served untouched.
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

# Every package is read and checked before anything is written, so that a
# bad argument cannot cost the repository that is already there.
declare -A seen=()
declare -A archs=()
pool_paths=()
for deb in "$@"; do
  [ -f "$deb" ] || die "$deb: no such file"
  name=$(dpkg-deb -f "$deb" Package)
  version=$(dpkg-deb -f "$deb" Version)
  arch=$(dpkg-deb -f "$deb" Architecture)
  [ "$name" = pomodoro ] || die "$deb is package '$name', not 'pomodoro'"
  [ -n "$version" ] && [ -n "$arch" ] || die "$deb: no version or architecture"
  [ -z "${seen[$version/$arch]:-}" ] \
    || die "$deb: version $version for $arch was given twice"
  seen[$version/$arch]=$deb
  archs[$arch]=1
  # Debian leaves the epoch out of a pool file name.
  pool_paths+=("pool/$COMPONENT/${name:0:1}/$name/${name}_${version#*:}_${arch}.deb")
done

# Refuse to sign with a guess. An apt repository signed by the wrong key
# looks fine until every user's `apt update` fails.
key=${APT_SIGNING_KEY_ID:-}
if [ -z "$key" ]; then
  listing=$(gpg --batch --list-secret-keys --with-colons) \
    || die "could not read the GnuPG keyring"
  mapfile -t secret_keys < <(printf '%s\n' "$listing" | awk -F: '$1 == "sec" { print $5 }')
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

# The output directory is not touched until the very end. Until then
# everything happens in a sibling directory, so the move into place is a
# rename on the same filesystem rather than a copy.
parent=$(dirname "$out")
mkdir -p "$parent"
parent=$(cd "$parent" && pwd)
out="$parent/$(basename "$out")"
work=$(mktemp -d "$out.new.XXXXXX")
release=$(mktemp)
trap 'rm -rf "$work" "$release"' EXIT

i=0
for deb in "$@"; do
  dest="$work/${pool_paths[$i]}"
  mkdir -p "$(dirname "$dest")"
  install -m 0644 "$deb" "$dest"
  i=$((i + 1))
done

cd "$work"
for arch in "${!archs[@]}"; do
  bin="dists/$SUITE/$COMPONENT/binary-$arch"
  mkdir -p "$bin"
  apt-ftparchive --arch "$arch" packages pool > "$bin/Packages"
  [ -s "$bin/Packages" ] || die "no packages were indexed for $arch"
  gzip -9 --keep --no-name "$bin/Packages"
done

# apt-ftparchive reports a package it could not read on stderr and carries
# on, so a truncated .deb would be dropped from the index while the run
# still succeeded: present in the pool, signed for, and uninstallable.
for path in "${pool_paths[@]}"; do
  grep -qxF "Filename: $path" "dists/$SUITE/$COMPONENT/binary-"*/Packages \
    || die "$path is in the pool but not in any index; the .deb is probably corrupt"
done

# Written outside the tree and moved in afterwards, so that the Release file
# does not list a half-written copy of itself.
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
[ -s pomodoro.gpg ] && [ -s pomodoro.asc ] \
  || die "could not export the public key for $key"

# GitHub Pages runs sites through Jekyll unless told not to.
touch .nojekyll

# The keyring is read by apt's unprivileged _apt user, so it is made
# readable explicitly: `sudo tee` would otherwise create it under the
# caller's umask, and a umask of 077 breaks `apt update` on that machine
# alone.
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
sudo chmod a+r /etc/apt/keyrings/pomodoro.gpg
echo "deb [signed-by=/etc/apt/keyrings/pomodoro.gpg] $url $SUITE $COMPONENT" | sudo tee /etc/apt/sources.list.d/pomodoro.list &gt;/dev/null
sudo apt update</pre>
<p>Then install, and from then on receive updates with the rest of the system:</p>
<pre>sudo apt install pomodoro</pre>
</html>
HTML

cd "$parent"
# The old tree is moved aside rather than deleted first, so that the window
# in which neither the old nor the new repository is in place is a single
# rename rather than a recursive delete.
previous=
if [ -e "$out" ]; then
  previous="$out.old.$$"
  mv "$out" "$previous"
fi
mv "$work" "$out"
trap 'rm -f "$release"' EXIT
[ -z "$previous" ] || rm -rf "$previous"

echo "apt repository written to $out, signed by $key:"
( cd "$out" && find . -type f | sort | sed 's|^\./|  |' )
