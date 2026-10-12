#!/usr/bin/env bash
# Install or upgrade figcraft-server on a Debian or Ubuntu machine, behind Caddy (TLS), as a
# server open to the public. See docs/server.md ("Running a public server").
#
#   sudo ./install.sh --host figcraft.example.com --email you@example.com
#
# It installs the binary (from this folder when run from a release archive; built from the source
# tree around this script when it sits in one, e.g. a copy of the repository; else the latest
# release for this machine checked against its SHA256SUMS.txt, else built from a fresh clone; builds
# add a temporary swap file when the machine has under 2 GB of memory), makes the `figcraft` user, its
# data directory and /etc/figcraft-server/server.toml (only when missing: your edits are kept),
# installs the systemd unit and the Caddy site, opens SSH, 80 and 443 in ufw and starts it all.
# Safe to run again: it upgrades the binary and restarts the server only when something changed.
#
# Options:
#   --host HOST        the DNS name people use (its A/AAAA records must point at this machine)
#   --email EMAIL      for Let's Encrypt and, in a new config, the abuse contact shown to users
#   --terms URL        in a new config: your terms of use (https://...)
#   --private          in a new config: only people with the server key publish (not open)
#   --version X.Y.Z    install this release (default: the latest)
#   --from-source      build from source even when a release archive exists
#   --ref REF          the git branch or tag to clone and build (default: vX.Y.Z with --version,
#                      else main; a source tree around this script is built as it is)
#   --backups          also enable the nightly backup timer (stops the server for a few seconds)
#   --no-firewall      leave ufw alone
#   -h, --help         this text
set -euo pipefail

REPO="storytold/figcraft"
CONFIG_DIR=/etc/figcraft-server
CONFIG="$CONFIG_DIR/server.toml"
DATA=/var/lib/figcraft-server
BIN=/usr/local/bin/figcraft-server
BUILD=/var/cache/figcraft-build
SERVICE=figcraft-server
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# The source tree this script sits in (packaging/server/ of a copy or checkout), if any.
LOCAL_SRC=""
if [ -f "$HERE/../../Cargo.toml" ] && [ -d "$HERE/../../apps/figcraft-server" ]; then
  LOCAL_SRC="$(cd "$HERE/../.." && pwd)"
fi

HOST=""
EMAIL=""
TERMS=""
OPEN=true
VERSION=""
FROM_SOURCE=false
REF=""
BACKUPS=false
FIREWALL=true

say() { printf '==> %s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --host) HOST="${2:-}"; shift 2 ;;
    --email) EMAIL="${2:-}"; shift 2 ;;
    --terms) TERMS="${2:-}"; shift 2 ;;
    --private) OPEN=false; shift ;;
    --version) VERSION="${2:-}"; shift 2 ;;
    --from-source) FROM_SOURCE=true; shift ;;
    --ref) REF="${2:-}"; shift 2 ;;
    --backups) BACKUPS=true; shift ;;
    --no-firewall) FIREWALL=false; shift ;;
    -h | --help) sed -n '2,26p' "$0"; exit 0 ;;
    *) die "unknown option $1 (see --help)" ;;
  esac
done

[ "$(id -u)" -eq 0 ] || die "run it as root (sudo ./install.sh ...)"
# shellcheck source=/dev/null
. /etc/os-release
case " ${ID:-} ${ID_LIKE:-} " in
  *" debian "* | *" ubuntu "*) ;;
  *) die "this script is for Debian and Ubuntu (see docs/server.md for other systems)" ;;
esac

ask() { # ask VAR "question": read an answer from the terminal when the value wasn't given
  local var="$1" question="$2" answer
  if [ -z "${!var}" ]; then
    [ -t 0 ] || die "give $3 (no terminal to ask on)"
    read -r -p "$question " answer
    printf -v "$var" '%s' "$answer"
  fi
}
ask HOST "DNS name people will use (e.g. figcraft.example.com):" "--host"
ask EMAIL "Email for Let's Encrypt and the abuse contact shown to users:" "--email"
case "$HOST" in
  "" | *[!A-Za-z0-9.-]* | .* | *. ) die "--host must be a DNS name like figcraft.example.com" ;;
esac
case "$EMAIL" in
  *@*.*) ;;
  *) die "--email must be an email address" ;;
esac
case "$TERMS" in
  "" | https://* | http://*) ;;
  *) die "--terms must be a web address (https://...)" ;;
esac
case "$EMAIL$TERMS" in
  *[[:space:]\"\\]*) die "--email and --terms can't contain spaces, quotes or backslashes" ;;
esac

export DEBIAN_FRONTEND=noninteractive
say "Packages"
apt-get update -qq
apt-get install -y -qq curl ca-certificates tar caddy ufw >/dev/null

ARCH="$(uname -m)"
case "$ARCH" in
  amd64) ARCH=x86_64 ;;
  arm64) ARCH=aarch64 ;;
esac

# --- The binary -----------------------------------------------------------------------------
NEW_BIN=""
FILES="$HERE" # where the unit and the Caddyfile come from

from_release() { # download and check the release archive; sets NEW_BIN and FILES
  local version="$VERSION" name dir
  if [ -z "$version" ]; then
    version="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p' | head -n 1)" || return 1
    [ -n "$version" ] || return 1
  fi
  name="figcraft-server-$version-linux-$ARCH"
  dir="$BUILD/release"
  rm -rf "$dir"
  mkdir -p "$dir"
  say "Downloading $name"
  curl -fsSL -o "$dir/$name.tar.gz" "https://github.com/$REPO/releases/download/v$version/$name.tar.gz" || return 1
  curl -fsSL -o "$dir/SHA256SUMS.txt" "https://github.com/$REPO/releases/download/v$version/SHA256SUMS.txt" || return 1
  (cd "$dir" && grep " $name.tar.gz\$" SHA256SUMS.txt | sha256sum -c --quiet -) || die "the download doesn't match SHA256SUMS.txt"
  tar -C "$dir" -xzf "$dir/$name.tar.gz"
  NEW_BIN="$dir/$name/figcraft-server"
  FILES="$dir/$name"
}

SWAP=""
cleanup() {
  if [ -n "$SWAP" ]; then
    swapoff "$SWAP" 2>/dev/null || true
    rm -f "$SWAP"
  fi
}
trap cleanup EXIT

from_source() { # build the source tree around this script, else a fresh clone; sets NEW_BIN and FILES
  local mem_kb src ref="$REF"
  if [ -z "$ref" ]; then
    if [ -n "$VERSION" ]; then ref="v$VERSION"; else ref=main; fi
  fi
  apt-get install -y -qq build-essential git pkg-config >/dev/null
  mkdir -p "$BUILD"
  export RUSTUP_HOME="$BUILD/rustup" CARGO_HOME="$BUILD/cargo" CARGO_TARGET_DIR="$BUILD/target"
  if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
    curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable --no-modify-path >/dev/null
  else
    "$CARGO_HOME/bin/rustup" update stable >/dev/null
  fi
  if [ -n "$LOCAL_SRC" ]; then
    src="$LOCAL_SRC"
    say "Building the source in $src: this takes a while"
  else
    src="$BUILD/src"
    say "Building from a clone ($ref): this takes a while"
    if [ -d "$src/.git" ]; then
      git -C "$src" fetch --quiet --tags origin
    else
      rm -rf "$src"
      git clone --quiet "https://github.com/$REPO" "$src" || die "can't clone $REPO (a private repository: copy the source to this machine and run its packaging/server/install.sh)"
    fi
    # A branch as it is upstream now, or a tag.
    git -C "$src" checkout --quiet --force --detach "origin/$ref" 2>/dev/null ||
      git -C "$src" checkout --quiet --force --detach "$ref"
  fi
  # The compiler needs about 2 GB: add a temporary swap file on smaller machines.
  mem_kb="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo)"
  if [ "$mem_kb" -lt 2000000 ] && [ -z "$(swapon --show --noheadings)" ]; then
    SWAP="$BUILD/swapfile"
    say "Adding a temporary 2 GB swap file for the build"
    fallocate -l 2G "$SWAP" 2>/dev/null || dd if=/dev/zero of="$SWAP" bs=1M count=2048 status=none
    chmod 600 "$SWAP"
    mkswap "$SWAP" >/dev/null
    swapon "$SWAP"
    export CARGO_BUILD_JOBS=1
  fi
  (cd "$src" && "$CARGO_HOME/bin/cargo" build --release --locked -p figcraft-server)
  cleanup
  SWAP=""
  NEW_BIN="$CARGO_TARGET_DIR/release/figcraft-server"
  FILES="$src/packaging/server"
}

if [ -x "$HERE/figcraft-server" ] && [ "$FROM_SOURCE" = false ]; then
  NEW_BIN="$HERE/figcraft-server" # run from an unpacked release archive
elif [ -n "$LOCAL_SRC" ] && [ -z "$VERSION" ]; then
  from_source # the source copied to (or checked out on) this machine, as it is
elif [ "$FROM_SOURCE" = true ] || ! from_release; then
  [ "$FROM_SOURCE" = true ] || say "No release archive for linux-$ARCH: building from source instead"
  from_source
fi
[ -x "$NEW_BIN" ] || die "no figcraft-server binary to install"
# The unit files and the Caddyfile beside this script when it has them (a checkout or an archive).
[ -f "$HERE/figcraft-backup.timer" ] && FILES="$HERE"
for f in figcraft-server.service Caddyfile figcraft-backup.service figcraft-backup.timer; do
  [ -f "$FILES/$f" ] || die "$FILES/$f is missing"
done

CHANGED=false
if ! cmp -s "$NEW_BIN" "$BIN"; then
  say "Installing $("$NEW_BIN" --version)"
  install -m 755 "$NEW_BIN" "$BIN.new"
  mv -f "$BIN.new" "$BIN"
  CHANGED=true
fi

# --- User, data and settings ----------------------------------------------------------------
id -u figcraft >/dev/null 2>&1 || useradd --system --home-dir "$DATA" --shell /usr/sbin/nologin figcraft
install -d -o figcraft -g figcraft -m 700 "$DATA"
install -d -m 755 "$CONFIG_DIR"
if [ ! -f "$CONFIG" ]; then
  say "Writing $CONFIG"
  {
    printf '# figcraft-server settings (docs/server.md, "Configuration"). install.sh never overwrites this file.\n'
    printf 'listen = "127.0.0.1:47900"\n'
    printf 'data = "%s"\n' "$DATA"
    printf 'public_url = "wss://%s/"\n' "$HOST"
    printf '# Caddy on this machine: its X-Forwarded-For names the client.\n'
    printf 'trusted_proxy = "127.0.0.1"\n'
    printf '# Anyone may publish without the key (false: only people with the key).\n'
    printf 'open_publish = %s\n' "$OPEN"
    printf '# Shown to people where they publish: how to reach you about abuse, and your terms.\n'
    printf 'operator_contact = "%s"\n' "$EMAIL"
    printf 'terms_url = "%s"\n' "$TERMS"
    printf '# Files published without the key are deleted after this many days unopened.\n'
    printf 'idle_days = 30\n'
    printf 'max_file_mb = 16\n'
    printf '# All files together, and per address (see docs/server.md for every limit).\n'
    printf 'max_total_mb = 10240\n'
    printf 'max_files_per_ip = 20\n'
    printf 'max_mb_per_ip = 256\n'
  } >"$CONFIG.new"
  chmod 644 "$CONFIG.new"
  mv -f "$CONFIG.new" "$CONFIG"
  CHANGED=true
fi
"$BIN" --config "$CONFIG" check >/dev/null ||
  die "$CONFIG isn't right for this figcraft-server (see above; a release older than open hosting needs --from-source)"

install_file() { # install_file SOURCE TARGET MODE: true when it changed
  if cmp -s "$1" "$2"; then return 1; fi
  install -m "$3" "$1" "$2"
}

# --- systemd --------------------------------------------------------------------------------
UNITS=false
install_file "$FILES/figcraft-server.service" /etc/systemd/system/figcraft-server.service 644 && UNITS=true
install_file "$FILES/figcraft-backup.service" /etc/systemd/system/figcraft-backup.service 644 && UNITS=true
install_file "$FILES/figcraft-backup.timer" /etc/systemd/system/figcraft-backup.timer 644 && UNITS=true
if [ "$UNITS" = true ]; then
  systemctl daemon-reload
  CHANGED=true
fi
systemctl enable --quiet "$SERVICE"
if [ "$CHANGED" = true ] || ! systemctl is-active --quiet "$SERVICE"; then
  say "Starting figcraft-server"
  systemctl restart "$SERVICE"
fi
if [ "$BACKUPS" = true ]; then
  systemctl enable --now --quiet figcraft-backup.timer
fi

# --- Caddy ----------------------------------------------------------------------------------
MARK="# figcraft-server (install.sh)"
CADDYFILE=/etc/caddy/Caddyfile
OWN=true
# Anything but the packaged example site (or ours) is someone else's: leave it, add a file of ours.
if [ -f "$CADDYFILE" ] && ! grep -qF "$MARK" "$CADDYFILE" && grep -qvE '^\s*(#|$|:80|\{|\}|root|file_server)' "$CADDYFILE"; then
  CADDYFILE=/etc/caddy/figcraft-server.caddy
  OWN=false
fi
SITE="$(mktemp)"
{
  printf '%s: written by packaging/server/install.sh (run it again to update).\n' "$MARK"
  # Global options belong at the top of the main Caddyfile only.
  [ "$OWN" = false ] || printf '{\n\temail %s\n}\n\n' "$EMAIL"
  sed -e '/^#/d' -e "s/figcraft\.example\.com/$HOST/" "$FILES/Caddyfile" | sed -e '/./,$!d'
} >"$SITE"
[ "$OWN" = true ] || say "Caddy already serves other sites: add 'import figcraft-server.caddy' to /etc/caddy/Caddyfile"
if install_file "$SITE" "$CADDYFILE" 644; then
  caddy validate --config /etc/caddy/Caddyfile --adapter caddyfile >/dev/null 2>&1 || die "Caddy doesn't accept /etc/caddy/Caddyfile"
  systemctl reload caddy || systemctl restart caddy
fi
rm -f "$SITE"
systemctl enable --now --quiet caddy

# --- Firewall -------------------------------------------------------------------------------
if [ "$FIREWALL" = true ]; then
  say "Firewall: SSH, 80 and 443"
  ufw allow 22/tcp >/dev/null
  # The port this SSH session came in on, if it isn't 22.
  if [ -n "${SSH_CONNECTION:-}" ]; then
    ufw allow "${SSH_CONNECTION##* }/tcp" >/dev/null
  fi
  ufw allow 80/tcp >/dev/null
  ufw allow 443/tcp >/dev/null
  ufw --force enable >/dev/null
fi

# --- Done -----------------------------------------------------------------------------------
for _ in $(seq 1 50); do
  curl -fs http://127.0.0.1:47900/healthz >/dev/null && break
  sleep 0.2
done
curl -fs http://127.0.0.1:47900/healthz >/dev/null || die "figcraft-server isn't answering: journalctl -u $SERVICE"
ADMIN="sudo -u figcraft figcraft-server --config $CONFIG"
cat <<EOF

figcraft-server is running behind Caddy.

  Publish:   in FigCraft, Share > Publish to server..., address wss://$HOST/
             $(if [ "$OPEN" = true ]; then echo "(open to the public: no key needed; each file gets an owner token)"; else echo "(with the key: $ADMIN key)"; fi)
  Check:     curl https://$HOST/healthz      (once DNS points here and Caddy has its certificate)
  Logs:      journalctl -u $SERVICE -f

  Moderate:  $ADMIN files list --sort size
             $ADMIN files remove FILE
             $ADMIN reports
             $ADMIN block-ip ADDRESS      (unblock-ip, blocked)
  Server key: $ADMIN key
  Settings:  $CONFIG (then: systemctl restart $SERVICE)
  Backups:   systemctl enable --now figcraft-backup.timer   (nightly, to /var/backups/figcraft-server)
  Upgrade:   run this script again

You are responsible for what people publish here: keep the abuse contact and terms current.
Files are not end-to-end encrypted: whoever runs the server can read them.
EOF
