#!/usr/bin/env bash
set -euo pipefail
trap 'status=$?; printf "build-image.sh failed at line %s: %s (status %s)\\n" "$LINENO" "$BASH_COMMAND" "$status" >&2' ERR

# App-owned candidate builder. Inputs are this recipe and Debian snapshot packages;
# task repositories and their build scripts are never inputs.
root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
output_dir="${1:?usage: build-image.sh OUTPUT_DIR}"
for tool in debootstrap qemu-img mkfs.ext4 debugfs mount umount mountpoint realpath grub-install chroot sha256sum parted sfdisk losetup sudo curl grep find sort touch; do
  command -v "$tool" >/dev/null || { echo "missing required builder: $tool" >&2; exit 2; }
done
work_parent="${RUNNER_TEMP:-/tmp}"
mkdir -p "$work_parent"
work_parent="$(cd "$work_parent" && pwd -P)"
work_dir="$(mktemp -d "$work_parent/computer-use-image.XXXXXX")"
snapshot="20261101T000000Z"
source_date_epoch="1793491200"
export SOURCE_DATE_EPOCH="$source_date_epoch" TZ=UTC LC_ALL=C
base_name="koolade-computer-use-guest-x86_64"
image_size="4G"

work_dir_is_safe() {
  [[ -d "$work_dir" && ! -L "$work_dir" ]] || return 1
  [[ "$(dirname -- "$work_dir")" == "$work_parent" ]] || return 1
  [[ "${work_dir##*/}" =~ ^computer-use-image\.[A-Za-z0-9]+$ ]] || return 1
  [[ "$(realpath -e -- "$work_dir")" == "$work_dir" ]]
}

loopdev=""
cleanup() {
  local original_status=$?
  local cleanup_failed=0
  trap - EXIT
  if mountpoint -q "$work_dir/mnt"; then
    sudo umount "$work_dir/mnt" || cleanup_failed=1
  fi
  if [[ -n "$loopdev" ]]; then
    sudo losetup -d "$loopdev" || cleanup_failed=1
  fi
  if mountpoint -q "$work_dir/mnt"; then
    echo "refusing to remove image work directory while mount remains" >&2
    cleanup_failed=1
  elif work_dir_is_safe; then
    sudo rm -rf --one-file-system -- "$work_dir" || cleanup_failed=1
  else
    echo "refusing to remove unvalidated image work directory" >&2
    cleanup_failed=1
  fi
  if (( original_status != 0 )); then exit "$original_status"; fi
  if (( cleanup_failed != 0 )); then exit 1; fi
}
trap cleanup EXIT

mkdir -p "$output_dir" "$work_dir/rootfs" "$work_dir/debootstrap-cache" "$work_dir/mnt"
mkdir -p "$work_dir/download-tools"
ln -s "$root_dir/containers/computer-use/debootstrap-wget" "$work_dir/download-tools/wget"

# debootstrap does not retry individual failed package downloads. Keep its .deb
# cache across bounded attempts so transient snapshot failures only refetch the
# packages that are still missing; the immutable snapshot keeps inputs pinned.
bootstrap_attempt=1
while true; do
  if sudo env PATH="$work_dir/download-tools:$PATH" debootstrap --verbose \
    --force-check-gpg --keyring=/usr/share/keyrings/debian-archive-keyring.gpg \
    --cache-dir="$work_dir/debootstrap-cache" \
    --arch=amd64 --variant=minbase bookworm "$work_dir/rootfs" \
    "https://snapshot.debian.org/archive/debian/${snapshot}/"; then
    break
  else
    bootstrap_status=$?
  fi
  if (( bootstrap_attempt >= 3 )); then
    echo "Debian bootstrap failed after ${bootstrap_attempt} attempts" >&2
    exit "$bootstrap_status"
  fi
  echo "Debian bootstrap attempt ${bootstrap_attempt} failed; retrying with cached packages" >&2
  work_dir_is_safe || { echo "refusing to reset an unsafe image work directory" >&2; exit 1; }
  sudo rm -rf --one-file-system -- "$work_dir/rootfs"
  mkdir -p "$work_dir/rootfs"
  ((bootstrap_attempt += 1))
done
sudo tee "$work_dir/rootfs/etc/apt/sources.list" >/dev/null <<APT
deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/${snapshot}/ bookworm main
deb [check-valid-until=no] https://snapshot.debian.org/archive/debian/${snapshot}/ bookworm-updates main
deb [check-valid-until=no] https://snapshot.debian.org/archive/debian-security/${snapshot}/ bookworm-security main
APT
chroot_guest() {
  sudo chroot "$work_dir/rootfs" env \
    SOURCE_DATE_EPOCH="$source_date_epoch" TZ=UTC LC_ALL=C "$@"
}
apt_get() {
  chroot_guest env DEBIAN_FRONTEND=noninteractive \
    apt-get -o Acquire::Retries=5 "$@"
}
apt_get update
apt_get full-upgrade -y
apt_get install -y --no-install-recommends \
  systemd-sysv linux-image-amd64 grub-pc-bin grub2-common \
  xorg openbox lightdm chromium dbus-x11 fonts-liberation \
  ca-certificates sudo locales
chroot_guest apt-get clean
sudo rm -f "$work_dir/rootfs/var/cache/ldconfig/aux-cache"
chroot_guest useradd --create-home --shell /bin/bash --groups audio,video koolade
chroot_guest passwd --lock koolade
chroot_guest usermod --lock root
sudo install -d -m 0755 "$work_dir/rootfs/etc/lightdm/lightdm.conf.d"
sudo tee "$work_dir/rootfs/etc/lightdm/lightdm.conf.d/50-koolade-guest.conf" >/dev/null <<'LIGHTDM'
[Seat:*]
autologin-user=koolade
autologin-user-timeout=0
user-session=openbox
LIGHTDM
sudo install -d -m 0755 "$work_dir/rootfs/home/koolade/.config/openbox"
sudo tee "$work_dir/rootfs/home/koolade/.config/openbox/autostart" >/dev/null <<'AUTO'
xset -dpms
xset s off
AUTO
chroot_guest chown -R koolade:koolade /home/koolade
sudo truncate -s 0 "$work_dir/rootfs/etc/machine-id"
sudo ln -sfn /etc/machine-id "$work_dir/rootfs/var/lib/dbus/machine-id"
sudo rm -f "$work_dir/rootfs/var/lib/systemd/random-seed"
chroot_guest ln -sf /usr/share/zoneinfo/UTC /etc/localtime
chroot_guest sed -i 's/^# en_US.UTF-8 UTF-8/en_US.UTF-8 UTF-8/' /etc/locale.gen
chroot_guest locale-gen en_US.UTF-8
chroot_guest update-locale LANG=en_US.UTF-8
chroot_guest systemctl enable lightdm
chroot_guest systemctl disable NetworkManager systemd-networkd 2>/dev/null || true

# CU-05 owns the authenticated agent. Until that artifact is reviewed, this
# service fails closed and cannot report guest readiness.
sudo tee "$work_dir/rootfs/etc/systemd/system/koolade-computer-use-agent.service" >/dev/null <<'UNIT'
[Unit]
Description=Kool.ad/e Computer Use guest companion (CU-05 required)
After=multi-user.target

[Service]
Type=notify
ExecStart=/usr/lib/koolade/computer-use-agent
Restart=no
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
UNIT
chroot_guest systemctl enable koolade-computer-use-agent.service
sudo tee "$work_dir/rootfs/etc/fstab" >/dev/null <<'FSTAB'
LABEL=rootfs / ext4 errors=remount-ro 0 1
FSTAB
sudo tee "$work_dir/rootfs/etc/hostname" >/dev/null <<'HOST'
koolade-computer-use-guest
HOST
chroot_guest update-initramfs -u -k all
chroot_guest dpkg-query -W -f='${Package}\t${Version}\n' | sort \
  > "$output_dir/${base_name}.packages.tsv"
python3 "$root_dir/containers/computer-use/make-sbom.py" \
  "$output_dir/${base_name}.packages.tsv" "$output_dir/${base_name}.sbom.cdx.json"

image="$work_dir/${base_name}.raw"
sudo truncate -s "$image_size" "$image"
sudo parted -s "$image" mklabel msdos mkpart primary ext4 1MiB 100% set 1 boot on
sudo sfdisk --disk-id "$image" 0x4b4f4f4c
loopdev="$(sudo losetup --find --show --partscan "$image")"
partition="${loopdev}p1"
for _ in $(seq 1 20); do [[ -b "$partition" ]] && break; sleep 0.25; done
[[ -b "$partition" ]] || { echo "partition device unavailable" >&2; exit 1; }
sudo env E2FSPROGS_FAKE_TIME="$source_date_epoch" mkfs.ext4 -F \
  -E lazy_itable_init=0,lazy_journal_init=0,hash_seed=00000000-0000-4000-8000-000000000379 \
  -U 00000000-0000-4000-8000-000000000379 -L rootfs "$partition"
sudo mount "$partition" "$work_dir/mnt"
sudo cp -a "$work_dir/rootfs/." "$work_dir/mnt/"
sudo grub-install --target=i386-pc --boot-directory="$work_dir/mnt/boot" "$loopdev"
sudo mkdir -p "$work_dir/mnt/boot/grub"
sudo tee "$work_dir/mnt/boot/grub/grub.cfg" >/dev/null <<'GRUB'
set timeout=0
set default=0
menuentry 'Kool.ad/e Computer Use guest' {
  search --label rootfs --set=root
  linux /vmlinuz root=LABEL=rootfs ro quiet
  initrd /initrd.img
}
GRUB
sudo cp -a "$work_dir/mnt/." "$work_dir/rootfs/"
if sudo test -d "$work_dir/rootfs/lost+found"; then
  sudo rmdir "$work_dir/rootfs/lost+found"
fi
if sudo grep -R -aF -e "$work_dir" -e "$loopdev" "$work_dir/rootfs/boot/grub"; then
  echo "refusing to package builder paths in the guest GRUB files" >&2
  exit 1
fi
sudo find "$work_dir/rootfs/var/log" -xdev -type f -delete
sudo find "$work_dir/rootfs" -xdev -exec touch -h -d "@$source_date_epoch" {} +
sudo umount "$work_dir/mnt"

# Rebuild the final filesystem offline so the image contains no transient mount
# journal records or builder mount path. Normalize inode and superblock times
# after the read-only scan; mtime normalization alone cannot reset inode ctime.
sudo env E2FSPROGS_FAKE_TIME="$source_date_epoch" mkfs.ext4 -F \
  -E lazy_itable_init=0,lazy_journal_init=0,hash_seed=00000000-0000-4000-8000-000000000379 \
  -U 00000000-0000-4000-8000-000000000379 -L rootfs \
  -d "$work_dir/rootfs" "$partition"
sudo mount -o ro,noload "$partition" "$work_dir/mnt"
normalization_commands="$work_dir/normalize-filesystem.debugfs"
{
  printf 'set_current_time @%s\n' "$source_date_epoch"
  sudo find "$work_dir/mnt" -xdev -printf '%i\0' | sort -z -u \
    | while IFS= read -r -d '' inode; do
        [[ "$inode" =~ ^[0-9]+$ ]] || exit 1
        printf 'sif <%s> atime @%s\nsif <%s> ctime @%s\nsif <%s> mtime @%s\n' \
          "$inode" "$source_date_epoch" \
          "$inode" "$source_date_epoch" \
          "$inode" "$source_date_epoch"
      done
  printf 'ssv mtime @%s\nssv wtime @%s\nssv lastcheck @%s\n' \
    "$source_date_epoch" "$source_date_epoch" "$source_date_epoch"
  printf 'ssv mkfs_time @%s\n' "$source_date_epoch"
  printf 'ssv mnt_count 0\nssv last_mounted "koolade-computer-use-base-v1"\n'
} > "$normalization_commands"
sudo umount "$work_dir/mnt"
normalization_log="$work_dir/normalize-filesystem.debugfs.log"
sudo env E2FSPROGS_FAKE_TIME="$source_date_epoch" \
  debugfs -w -f "$normalization_commands" "$partition" >"$normalization_log" 2>&1
if grep -Eiq 'error|failed|unable|not found|invalid|cannot|warning' "$normalization_log"; then
  echo "filesystem metadata normalization reported an error" >&2
  exit 1
fi
sudo losetup -d "$loopdev"
loopdev=""
qemu-img convert -f raw -O qcow2 -c "$image" "$output_dir/${base_name}.qcow2"
qemu-img info --output=json "$output_dir/${base_name}.qcow2" \
  | python3 -c '
import json, sys
def without_filenames(value):
    if isinstance(value, dict):
        return {
            key: without_filenames(child)
            for key, child in value.items()
            if key != "filename"
        }
    if isinstance(value, list):
        return [without_filenames(child) for child in value]
    return value

print(json.dumps(without_filenames(json.load(sys.stdin)), sort_keys=True, separators=(",", ":")))
' \
  > "$output_dir/${base_name}.qemu-info.json"
sha256sum "$output_dir/${base_name}.qcow2" > "$output_dir/${base_name}.sha256"
(cd "$root_dir" && sha256sum \
  containers/computer-use/build-image.sh \
  containers/computer-use/make-sbom.py \
  containers/computer-use/debootstrap-wget \
  .github/workflows/computer-use-images.yml) \
  > "$output_dir/${base_name}.recipe-components.sha256"
recipe_digest="$(sha256sum "$output_dir/${base_name}.recipe-components.sha256" | cut -d ' ' -f 1)"
printf 'sha256:%s\n' "$recipe_digest" > "$output_dir/${base_name}.recipe.sha256"
printf 'debootstrap: %s\nqemu-img: %s\ngrub-install: %s\ndebugfs: %s\n' \
  "$(debootstrap --version 2>&1 | head -1)" \
  "$(qemu-img --version | head -1)" \
  "$(grub-install --version | head -1)" \
  "$(debugfs -V 2>&1 | head -1)" \
  > "$output_dir/${base_name}.builder-versions.txt"
sha256sum "$output_dir/${base_name}.sbom.cdx.json" \
  > "$output_dir/${base_name}.sbom.sha256"
source_revision="${GITHUB_SHA:-$(git -C "$root_dir" rev-parse HEAD)}"
printf '%s\n' "$source_revision" > "$output_dir/${base_name}.source-revision"
printf 'candidate image only; no CU-05 readiness or signed promotion\n' \
  > "$output_dir/${base_name}.promotion-status.txt"
