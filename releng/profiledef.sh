#!/usr/bin/env bash
# shellcheck disable=SC2034

iso_name="arctix"
iso_label="ARCTIX_$(date +%Y%m)"
iso_publisher="Arctix"
iso_application="Arctix Live/Rescue"
iso_version="0.1.2"
install_dir="arch"
bootmodes=('bios.syslinux'
           'uefi.grub')
pacman_conf="pacman.conf"
airootfs_image_type="squashfs"
if [[ "${ARCTIX_RELEASE:-0}" == "1" ]]; then
  airootfs_image_tool_options=('-processors' '16' '-comp' 'xz' '-Xbcj' 'x86' '-b' '1M' '-Xdict-size' '1M')
else
  airootfs_image_tool_options=('-processors' '16' '-comp' 'zstd' '-Xcompression-level' '3' '-b' '1M')
fi
bootstrap_tarball_compression=('zstd' '-c' '-T0' '--auto-threads=logical' '--long' '-19')
kernel_params_aarch64="clk_ignore_unused pd_ignore_unused arm64.nopauth"
file_permissions=(
  ["/etc/shadow"]="0:0:400"
  ["/root"]="0:0:750"
  ["/root/.automated_script.sh"]="0:0:755"
  ["/root/.gnupg"]="0:0:700"
  ["/usr/local/bin/choose-mirror"]="0:0:755"
  ["/usr/local/bin/Installation_guide"]="0:0:755"
  ["/usr/local/bin/livecd-sound"]="0:0:755"
  ["/usr/local/bin/arctix-install"]="0:0:755"
  ["/etc/sudoers.d"]="0:0:750"
  ["/etc/sudoers.d/g_wheel"]="0:0:440"
  ["/home/arctix"]="1000:1000:750" 
)
