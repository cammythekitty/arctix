#!/usr/bin/env bash
# Headless QEMU harness for testing arctix-install against a throwaway disk image.
# It never touches a real drive: the VM only sees the qcow2 file and the ISO.
#
#   scripts/vm-test.sh up          boot the live ISO with the target image attached, wait for ssh
#   scripts/vm-test.sh push        cargo build --release and copy the binary + sample config in
#   scripts/vm-test.sh run 'cmd'   run a bash command in the VM (or pipe a script on stdin)
#   scripts/vm-test.sh ssh         interactive shell in the VM
#   scripts/vm-test.sh install     push, then install with --yes and TEST_PASSWORD (default arctix-test)
#   scripts/vm-test.sh install-unplugged  start the install, hot-unplug the network card so the
#                                   install runs with no network device, then plug it back in
#   scripts/vm-test.sh boot-disk   boot the installed disk only (no ISO), for boot tests
#   scripts/vm-test.sh down        power the VM off
#   scripts/vm-test.sh reset       power off and recreate a blank target image
#   scripts/vm-test.sh status      is the VM running?
#   scripts/vm-test.sh screenshot [out.png]   grab the VM screen (default ~/arctix-test/screen.png)
#   scripts/vm-test.sh type 'text'  type text on the VM keyboard (\n = Enter)
#   scripts/vm-test.sh key ret      press keys by QEMU name (ret, ctrl-alt-f2, ...)
#
# Environment:
#   BIOS=1        boot with SeaBIOS instead of UEFI
#   NET=restrict  network link up but no internet (QEMU restrict=on); ssh still works
#   NET=0         no network device at all (-nic none). No ssh; for boot tests only
#   DISPLAY_VM=1  show a GTK window instead of running headless
#   VGA=std       headless video: std (default), virtio (2D virtio-gpu) or gl (virgl via
#                 egl-headless, GPU accelerated, but screenshots do not work with it)
#   ISO=path      ISO to boot (default: newest in ../out)
#   DISK_SIZE=20G size used by reset
#   MEM=6G CPUS=4 SSH_PORT=2222
#   KB=dk         layout the guest uses for `type` (us default; use dk when typing into an installed
#                 system that was set up with the dk keymap)
#
# Access works through cloud-init (enabled on the live ISO): a seed image adds a
# throwaway ssh key for the live user. Key, seed, image and logs live in ~/arctix-test.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALLER="$(dirname "$HERE")"
REPO="$(dirname "$INSTALLER")"
WORK="${ARCTIX_TEST_DIR:-$HOME/arctix-test}"
DISK="$WORK/target.qcow2"
KEY="$WORK/id_test"
SEED="$WORK/seed.iso"
TEST_PASSWORD="${TEST_PASSWORD:-arctix-test}"
PIDFILE="$WORK/qemu.pid"
MONSOCK="$WORK/mon.sock"
SERIAL="$WORK/serial.log"
MEM="${MEM:-6G}"
CPUS="${CPUS:-4}"
SSH_PORT="${SSH_PORT:-2222}"
DISK_SIZE="${DISK_SIZE:-20G}"
OVMF="/usr/share/edk2/x64/OVMF.4m.fd"

SSH_OPTS=(-i "$KEY" -p "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
          -o LogLevel=ERROR -o ConnectTimeout=5)

die() { echo "vm-test: $*" >&2; exit 1; }
say() { echo "vm-test: $*"; }

running() { [[ -f "$PIDFILE" ]] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; }

default_iso() {
  local iso
  iso="$(ls -t "$REPO"/out/*.iso 2>/dev/null | head -n1 || true)"
  [[ -n "$iso" ]] || die "no ISO in $REPO/out, build one or set ISO=..."
  echo "$iso"
}

ensure_disk() {
  mkdir -p "$WORK"
  [[ -f "$DISK" ]] || { qemu-img create -q -f qcow2 "$DISK" "$DISK_SIZE"; say "created blank $DISK ($DISK_SIZE)"; }
}

ensure_seed() {
  mkdir -p "$WORK"
  [[ -f "$KEY" ]] || ssh-keygen -q -t ed25519 -N '' -C arctix-installer-test -f "$KEY"
  [[ -f "$SEED" && "$SEED" -nt "$KEY.pub" && "$SEED" -nt "$0" ]] && return
  local pub dir
  pub="$(cat "$KEY.pub")"
  dir="$(mktemp -d)"
  cat > "$dir/user-data" <<EOF
#cloud-config
users:
  - name: arctix
    ssh_authorized_keys:
      - $pub
runcmd:
  - [sh, -c, 'install -d -m 700 -o arctix -g arctix /home/arctix/.ssh && echo "$pub" >> /home/arctix/.ssh/authorized_keys && chown arctix:arctix /home/arctix/.ssh/authorized_keys && chmod 600 /home/arctix/.ssh/authorized_keys']
  - [systemctl, start, sshd]
EOF
  printf 'instance-id: arctix-test\nlocal-hostname: arctix\n' > "$dir/meta-data"
  genisoimage -quiet -output "$SEED" -volid CIDATA -joliet -rock "$dir/user-data" "$dir/meta-data"
  rm -rf "$dir"
}

# Common QEMU arguments. $1 = "iso" to attach the live ISO + seed, "disk" for the target only.
qemu_start() {
  running && die "VM already running (pid $(cat "$PIDFILE")), use: $0 down"
  local args=(-name arctix-test -enable-kvm -m "$MEM" -smp "$CPUS" -cpu host
              -pidfile "$PIDFILE" -serial "file:$SERIAL"
              -monitor "unix:$MONSOCK,server,nowait"
              -drive "file=$DISK,if=none,id=target"
              -device "virtio-blk-pci,drive=target,bootindex=2")
  if [[ "${DISPLAY_VM:-0}" == 1 ]]; then
    args+=(-device virtio-vga-gl -display gtk,gl=on)
  else
    case "${VGA:-std}" in
      std)    args+=(-vga std -display none) ;;
      virtio) args+=(-device virtio-vga -display none) ;;
      gl)     args+=(-device virtio-vga-gl -display egl-headless) ;;
      *)      die "VGA must be std, virtio or gl" ;;
    esac
  fi
  [[ "${BIOS:-0}" == 1 ]] || args+=(-bios "$OVMF")
  case "${NET:-1}" in
    0)        args+=(-nic none) ;;
    restrict) args+=(-netdev "user,id=n0,restrict=on,hostfwd=tcp:127.0.0.1:$SSH_PORT-:22"
                     -device "virtio-net-pci,netdev=n0,id=nic0") ;;
    *)        args+=(-netdev "user,id=n0,hostfwd=tcp:127.0.0.1:$SSH_PORT-:22"
                     -device "virtio-net-pci,netdev=n0,id=nic0") ;;
  esac
  if [[ "$1" == iso ]]; then
    local iso="${ISO:-$(default_iso)}"
    # Explicit boot order: once a disk has a working install, firmware would
    # otherwise boot it instead of the ISO.
    args+=(-drive "file=$iso,media=cdrom,if=none,id=iso" -device "ide-cd,drive=iso,bus=ide.0,bootindex=1"
           -drive "file=$SEED,media=cdrom,if=none,id=seed" -device "ide-cd,drive=seed,bus=ide.1")
    say "booting $(basename "$iso") ($([[ "${BIOS:-0}" == 1 ]] && echo bios || echo uefi))"
  else
    say "booting the installed disk ($([[ "${BIOS:-0}" == 1 ]] && echo bios || echo uefi))"
  fi
  rm -f "$SERIAL"
  qemu-system-x86_64 "${args[@]}" -daemonize
}

wait_ssh() {
  [[ "${NET:-1}" == 0 ]] && { say "NET=0, no ssh"; return; }
  say "waiting for ssh on port $SSH_PORT"
  for _ in $(seq 1 60); do
    running || die "QEMU exited, see $SERIAL"
    if ssh "${SSH_OPTS[@]}" -o BatchMode=yes arctix@127.0.0.1 true 2>/dev/null; then
      say "ssh is up"; return
    fi
    sleep 5
  done
  die "ssh did not come up within 5 minutes"
}

# The live user's shell is fish, so commands always go through bash.
vm_run() {
  if [[ $# -gt 0 ]]; then
    ssh "${SSH_OPTS[@]}" arctix@127.0.0.1 bash -s <<< "$*"
  else
    ssh "${SSH_OPTS[@]}" arctix@127.0.0.1 bash -s
  fi
}

# Start the install in the background, pull the network card out from under it,
# wait for the done marker on the serial port, then plug the card back in.
install_unplugged() {
  push
  vm_run "rm -f ~/install.out; nohup sh -c 'echo $TEST_PASSWORD | sudo ./arctix-install install arctix.toml --yes --password-stdin > ~/install.out 2>&1; echo \"ARCTIX-INSTALL-DONE \$?\" | sudo tee /dev/ttyS0 >/dev/null' >/dev/null 2>&1 &"
  echo "device_del nic0" | monitor
  say "network card unplugged, install running without a network device"
  for _ in $(seq 1 300); do
    grep -q ARCTIX-INSTALL-DONE "$SERIAL" 2>/dev/null && break
    running || die "QEMU exited"
    sleep 2
  done
  grep -a ARCTIX-INSTALL-DONE "$SERIAL" | tail -n1 | tr -d '\r' || die "install did not finish within 10 minutes"
  echo "device_add virtio-net-pci,netdev=n0,id=nic0" | monitor
  say "network card plugged back in"
  wait_ssh
  vm_run 'tail -n 30 ~/install.out'
}

push() {
  (cd "$INSTALLER" && cargo build --release --quiet)
  scp -q -P "$SSH_PORT" "${SSH_OPTS[@]:0:2}" "${SSH_OPTS[@]:4}" \
    "$INSTALLER/target/release/arctix-install" "$INSTALLER/examples/arctix.toml" arctix@127.0.0.1:
  say "pushed arctix-install and arctix.toml"
}

# Send HMP monitor commands, one per line on stdin, and print the replies.
# The code goes in with -c so that stdin stays free for the commands.
read -r -d '' MONITOR_PY <<'PY' || true
import re, socket, sys, time
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.settimeout(5)
def drain():
    buf = b""
    try:
        while not buf.endswith(b"(qemu) "):
            d = s.recv(65536)
            if not d:
                break
            buf += d
    except socket.timeout:
        pass
    return re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", buf.decode(errors="replace"))
drain()
for cmd in sys.stdin.read().splitlines():
    s.sendall(cmd.encode() + b"\n")
    lines = [l.strip() for l in drain().splitlines()[1:]]
    lines = [l for l in lines if l and not l.startswith("(qemu)")]
    if lines:
        print("\n".join(lines))
    if cmd.startswith("sendkey"):
        time.sleep(0.03)
PY
monitor() {
  running || die "VM is not running"
  python3 -c "$MONITOR_PY" "$MONSOCK"
}

# Turn text into sendkey commands (US layout).
keys_for() {
  KB="${KB:-us}" python3 - "$1" <<'PY'
import os, sys
text = sys.argv[1].encode().decode("unicode_escape")
if os.environ["KB"] == "dk":
    # Danish layout: where these characters sit, as US key names.
    dk = {"/": "shift-7", "-": "slash", "_": "shift-slash", ":": "shift-dot", ";": "shift-comma",
          "|": "grave_accent", "=": "shift-0", "+": "minus", "\"": "shift-2", "'": "backslash",
          "(": "shift-8", ")": "shift-9", "&": "shift-6", "*": "shift-backslash", "?": "shift-minus"}
    for c in text:
        if c in dk:
            print(f"sendkey {dk[c]}")
        elif c.islower() or c.isdigit():
            print(f"sendkey {c}")
        elif c.isupper():
            print(f"sendkey shift-{c.lower()}")
        elif c in " \n\t.,":
            print("sendkey " + {" ": "spc", "\n": "ret", "\t": "tab", ".": "dot", ",": "comma"}[c])
        else:
            sys.exit(f"cannot type {c!r} on dk")
    sys.exit(0)
plain = {" ": "spc", "\n": "ret", "\t": "tab", "-": "minus", "=": "equal", ".": "dot", ",": "comma",
         "/": "slash", ";": "semicolon", "'": "apostrophe", "[": "bracket_left", "]": "bracket_right",
         "\\": "backslash", "`": "grave_accent"}
shifted = {"_": "minus", "+": "equal", ">": "dot", "<": "comma", "?": "slash", ":": "semicolon",
           '"': "apostrophe", "{": "bracket_left", "}": "bracket_right", "|": "backslash", "~": "grave_accent",
           "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8", "(": "9", ")": "0"}
for c in text:
    if c.islower() or c.isdigit():
        print(f"sendkey {c}")
    elif c.isupper():
        print(f"sendkey shift-{c.lower()}")
    elif c in plain:
        print(f"sendkey {plain[c]}")
    elif c in shifted:
        print(f"sendkey shift-{shifted[c]}")
    else:
        sys.exit(f"cannot type {c!r}")
PY
}

screenshot() {
  local out="${1:-$WORK/screen.png}"
  echo "screendump $WORK/screen.ppm" | monitor
  [[ -f "$WORK/screen.ppm" ]] || die "screendump produced no image (see monitor output above)"
  magick "$WORK/screen.ppm" "$out"
  rm -f "$WORK/screen.ppm"
  say "screenshot: $out"
}

down() {
  running || { say "not running"; return; }
  ssh "${SSH_OPTS[@]}" -o BatchMode=yes -o ConnectTimeout=3 arctix@127.0.0.1 'sudo systemctl poweroff' 2>/dev/null \
    || echo system_powerdown | monitor >/dev/null || true
  for _ in $(seq 1 30); do running || { say "stopped"; return; }; sleep 1; done
  say "no clean shutdown, killing QEMU"
  kill "$(cat "$PIDFILE")" 2>/dev/null || true
}

case "${1:-}" in
  up)        ensure_disk; ensure_seed; qemu_start iso; wait_ssh ;;
  install-unplugged) install_unplugged ;;
  boot-disk) ensure_disk; qemu_start disk; say "serial log: $SERIAL" ;;
  push)      push ;;
  run)       shift; vm_run "$@" ;;
  ssh)       ssh "${SSH_OPTS[@]}" arctix@127.0.0.1 ;;
  install)   push; vm_run "echo '${TEST_PASSWORD:-arctix-test}' | sudo ./arctix-install install arctix.toml --yes --password-stdin" ;;
  down)      down ;;
  reset)     down; rm -f "$DISK"; ensure_disk ;;
  screenshot) shift; screenshot "$@" ;;
  type)      shift; keys_for "$1" | monitor ;;
  key)       shift; for k in "$@"; do echo "sendkey $k"; done | monitor ;;
  status)    if running; then say "running (pid $(cat "$PIDFILE"))"; else say "not running"; fi ;;
  *)         sed -n '2,32p' "$0" | sed 's/^# \{0,1\}//'; exit 1 ;;
esac
