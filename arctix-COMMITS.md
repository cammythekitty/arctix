# arctix git history

Remote: https://github.com/cammythekitty/arctix (branch main)
Exported 2026-11-20 when the .git folder was left out of this backup.

```
commit e97384ae55ac69fe088db72d60ca8b93e48edfff
Author: Camilla
Date:   Tue Nov 17 13:14:49 2026 +0100

    update hostname, motd, and version; add quickshell package and adjust pacman.conf


 releng/airootfs/etc/hostname | 2 +-
 releng/airootfs/etc/motd     | 2 +-
 releng/packages.x86_64       | 1 +
 releng/pacman.conf           | 2 +-
 releng/profiledef.sh         | 4 ++--
 5 files changed, 6 insertions(+), 5 deletions(-)
commit 0a6ffbb348e47a4ad20453787d3df016954b7392
Author: Camilla
Date:   Tue Nov 17 11:46:18 2026 +0100

    addded python to the packages and removed the old  arch build


 out/archlinux-2026.11.17-x86_64.iso | Bin 1319927808 -> 0 bytes
 releng/packages.x86_64              |   1 +
 2 files changed, 1 insertion(+)
commit 281c50ad773155cc4c51acb9a2567dc431351a30
Author: Camilla
Date:   Tue Nov 17 11:40:17 2026 +0100

    v0.0.2: first bootable hyprland build shell and more need fixing


 out/arctix-0.0.2-x86_64.iso                                                                        |  Bin 0 -> 1694154752 bytes
 releng/airootfs/etc/group                                                                          |    6 +
 releng/airootfs/etc/gshadow                                                                        |    6 +
 releng/airootfs/etc/motd                                                                           |   12 +-
 releng/airootfs/etc/passwd                                                                         |    1 +
 releng/airootfs/etc/shadow                                                                         |    1 +
 releng/airootfs/etc/sudoers.d/g_wheel                                                              |    1 +
 releng/airootfs/etc/systemd/system/getty@tty1.service.d/autologin.conf                             |    2 +-
 releng/airootfs/home/arctix/.config/hypr/custom/env.lua                                            |    1 +
 releng/airootfs/home/arctix/.config/hypr/custom/execs.lua                                          |    1 +
 releng/airootfs/home/arctix/.config/hypr/custom/general.lua                                        |    1 +
 releng/airootfs/home/arctix/.config/hypr/custom/keybinds.lua                                       |    8 +
 releng/airootfs/home/arctix/.config/hypr/custom/rules.lua                                          |    1 +
 releng/airootfs/home/arctix/.config/hypr/custom/scripts/__restore_video_wallpaper.sh               |    2 +
 releng/airootfs/home/arctix/.config/hypr/custom/variables.lua                                      |    1 +
 releng/airootfs/home/arctix/.config/hypr/hypridle.conf                                             |   26 ++
 releng/airootfs/home/arctix/.config/hypr/hyprland.conf.old                                         |  231 +++++++++++
 releng/airootfs/home/arctix/.config/hypr/hyprland.lua                                              |   44 +++
 releng/airootfs/home/arctix/.config/hypr/hyprland/colors.lua                                       |   16 +
 releng/airootfs/home/arctix/.config/hypr/hyprland/env.lua                                          |   16 +
 releng/airootfs/home/arctix/.config/hypr/hyprland/execs.lua                                        |   25 ++
 releng/airootfs/home/arctix/.config/hypr/hyprland/general.lua                                      |  305 +++++++++++++++
 releng/airootfs/home/arctix/.config/hypr/hyprland/keybinds.lua                                     |  359 +++++++++++++++++
 releng/airootfs/home/arctix/.config/hypr/hyprland/lib/init.lua                                     |   27 ++
 releng/airootfs/home/arctix/.config/hypr/hyprland/rules.lua                                        |  169 ++++++++
 releng/airootfs/home/arctix/.config/hypr/hyprland/scripts/ai/license_show-loaded-ollama-models.txt |  201 ++++++++++
 releng/airootfs/home/arctix/.config/hypr/hyprland/scripts/ai/primary-buffer-query.sh               |   43 +++
 releng/airootfs/home/arctix/.config/hypr/hyprland/scripts/ai/show-loaded-ollama-models.sh          |   99 +++++
 releng/airootfs/home/arctix/.config/hypr/hyprland/scripts/fuzzel-emoji.sh                          | 1953 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 releng/airootfs/home/arctix/.config/hypr/hyprland/scripts/launch_first_available.sh                |    7 +
 releng/airootfs/home/arctix/.config/hypr/hyprland/scripts/snip_to_search.sh                        |    5 +
 releng/airootfs/home/arctix/.config/hypr/hyprland/scripts/start_geoclue_agent.sh                   |   30 ++
 releng/airootfs/home/arctix/.config/hypr/hyprland/services/create_custom_config.lua                |   29 ++
 releng/airootfs/home/arctix/.config/hypr/hyprland/services/init.lua                                |    1 +
 releng/airootfs/home/arctix/.config/hypr/hyprland/shellOverrides/main.lua                          |    2 +
 releng/airootfs/home/arctix/.config/hypr/hyprland/variables.lua                                    |   19 +
 releng/airootfs/home/arctix/.config/hypr/hyprlock.conf                                             |   93 +++++
 releng/airootfs/home/arctix/.config/hypr/hyprlock/colors.conf                                      |   12 +
 releng/airootfs/home/arctix/.config/hypr/kitty.conf                                                |   87 +++++
 releng/airootfs/home/arctix/.config/hypr/kitty/kitty.conf                                          |   39 ++
 releng/airootfs/home/arctix/.config/hypr/kitty/scroll_mark.py                                      |   18 +
 releng/airootfs/home/arctix/.config/hypr/kitty/search.py                                           |  341 ++++++++++++++++
 releng/airootfs/home/arctix/.config/kitty/kitty.conf                                               |   39 ++
 releng/airootfs/home/arctix/.config/kitty/scroll_mark.py                                           |   18 +
 releng/airootfs/home/arctix/.config/kitty/search.py                                                |  341 ++++++++++++++++
 releng/airootfs/home/arctix/.zlogin                                                                |    3 +
 releng/airootfs/root/.config/hypr/custom/env.lua                                                   |    1 +
 releng/airootfs/root/.config/hypr/custom/execs.lua                                                 |    1 +
 releng/airootfs/root/.config/hypr/custom/general.lua                                               |    1 +
 releng/airootfs/root/.config/hypr/custom/keybinds.lua                                              |    8 +
 releng/airootfs/root/.config/hypr/custom/rules.lua                                                 |    1 +
 releng/airootfs/root/.config/hypr/custom/scripts/__restore_video_wallpaper.sh                      |    2 +
 releng/airootfs/root/.config/hypr/custom/variables.lua                                             |    1 +
 releng/airootfs/root/.config/hypr/hypridle.conf                                                    |   26 ++
 releng/airootfs/root/.config/hypr/hyprland.conf.old                                                |  231 +++++++++++
 releng/airootfs/root/.config/hypr/hyprland.lua                                                     |   44 +++
 releng/airootfs/root/.config/hypr/hyprland/colors.lua                                              |   16 +
 releng/airootfs/root/.config/hypr/hyprland/env.lua                                                 |   16 +
 releng/airootfs/root/.config/hypr/hyprland/execs.lua                                               |   25 ++
 releng/airootfs/root/.config/hypr/hyprland/general.lua                                             |  305 +++++++++++++++
 releng/airootfs/root/.config/hypr/hyprland/keybinds.lua                                            |  359 +++++++++++++++++
 releng/airootfs/root/.config/hypr/hyprland/lib/init.lua                                            |   27 ++
 releng/airootfs/root/.config/hypr/hyprland/rules.lua                                               |  169 ++++++++
 releng/airootfs/root/.config/hypr/hyprland/scripts/ai/license_show-loaded-ollama-models.txt        |  201 ++++++++++
 releng/airootfs/root/.config/hypr/hyprland/scripts/ai/primary-buffer-query.sh                      |   43 +++
 releng/airootfs/root/.config/hypr/hyprland/scripts/ai/show-loaded-ollama-models.sh                 |   99 +++++
 releng/airootfs/root/.config/hypr/hyprland/scripts/fuzzel-emoji.sh                                 | 1953 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 releng/airootfs/root/.config/hypr/hyprland/scripts/launch_first_available.sh                       |    7 +
 releng/airootfs/root/.config/hypr/hyprland/scripts/snip_to_search.sh                               |    5 +
 releng/airootfs/root/.config/hypr/hyprland/scripts/start_geoclue_agent.sh                          |   30 ++
 releng/airootfs/root/.config/hypr/hyprland/services/create_custom_config.lua                       |   29 ++
 releng/airootfs/root/.config/hypr/hyprland/services/init.lua                                       |    1 +
 releng/airootfs/root/.config/hypr/hyprland/shellOverrides/main.lua                                 |    2 +
 releng/airootfs/root/.config/hypr/hyprland/variables.lua                                           |   19 +
 releng/airootfs/root/.config/hypr/hyprlock.conf                                                    |   93 +++++
 releng/airootfs/root/.config/hypr/hyprlock/colors.conf                                             |   12 +
 releng/airootfs/root/.config/hypr/kitty.conf                                                       |   87 +++++
 releng/airootfs/root/.config/hypr/kitty/kitty.conf                                                 |   39 ++
 releng/airootfs/root/.config/hypr/kitty/scroll_mark.py                                             |   18 +
 releng/airootfs/root/.config/hypr/kitty/search.py                                                  |  341 ++++++++++++++++
 releng/airootfs/root/.config/kitty/kitty.conf                                                      |   39 ++
 releng/airootfs/root/.config/kitty/scroll_mark.py                                                  |   18 +
 releng/airootfs/root/.config/kitty/search.py                                                       |  341 ++++++++++++++++
 releng/airootfs/root/.zlogin                                                                       |    2 +
 releng/efiboot/loader/entries/01-archiso-linux.conf                                                |    2 +-
 releng/efiboot/loader/entries/02-archiso-speech-linux.conf                                         |    2 +-
 releng/grub/grub.cfg                                                                               |    4 +-
 releng/grub/loopback.cfg                                                                           |    4 +-
 releng/packages.x86_64                                                                             |    9 +
 releng/profiledef.sh                                                                               |   15 +-
 releng/syslinux/archiso_head.cfg                                                                   |    2 +-
 releng/syslinux/archiso_pxe-linux.cfg                                                              |   18 +-
 releng/syslinux/archiso_sys-linux.cfg                                                              |   12 +-
 93 files changed, 9282 insertions(+), 40 deletions(-)
commit 7d7bf5c6d08a8d8216af9dc18e0acedd21ef4554
Author: Camilla
Date:   Tue Nov 17 10:35:55 2026 +0100

    v0.0.1: first bootable build


 out/archlinux-2026.11.17-x86_64.iso                                                                        | Bin 0 -> 1319927808 bytes
 releng/airootfs/etc/hostname                                                                               |   1 +
 releng/airootfs/etc/locale.conf                                                                            |   1 +
 releng/airootfs/etc/localtime                                                                              |   1 +
 releng/airootfs/etc/mkinitcpio.conf.d/archiso.conf                                                         |   3 ++
 releng/airootfs/etc/motd                                                                                   |  11 ++++
 releng/airootfs/etc/pacman.d/hooks/uncomment-mirrors.hook                                                  |  13 +++++
 releng/airootfs/etc/pacman.d/hooks/zzzz99-remove-custom-hooks-from-airootfs.hook                           |  18 +++++++
 releng/airootfs/etc/passwd                                                                                 |   1 +
 releng/airootfs/etc/resolv.conf                                                                            |   1 +
 releng/airootfs/etc/shadow                                                                                 |   1 +
 releng/airootfs/etc/ssh/sshd_config.d/10-archiso.conf                                                      |   3 ++
 releng/airootfs/etc/systemd/journald.conf.d/volatile-storage.conf                                          |   2 +
 releng/airootfs/etc/systemd/logind.conf.d/do-not-suspend.conf                                              |   4 ++
 releng/airootfs/etc/systemd/network/20-ethernet.network                                                    |  22 ++++++++
 releng/airootfs/etc/systemd/network/20-wlan.network                                                        |  20 ++++++++
 releng/airootfs/etc/systemd/network/20-wwan.network                                                        |  19 +++++++
 releng/airootfs/etc/systemd/networkd.conf.d/ipv6-privacy-extensions.conf                                   |   2 +
 releng/airootfs/etc/systemd/resolved.conf.d/archiso.conf                                                   |   4 ++
 releng/airootfs/etc/systemd/system-generators/systemd-gpt-auto-generator                                   |   1 +
 releng/airootfs/etc/systemd/system/choose-mirror.service                                                   |  10 ++++
 releng/airootfs/etc/systemd/system/cloud-init.target.wants/cloud-config.service                            |   1 +
 releng/airootfs/etc/systemd/system/cloud-init.target.wants/cloud-final.service                             |   1 +
 releng/airootfs/etc/systemd/system/cloud-init.target.wants/cloud-init-local.service                        |   1 +
 releng/airootfs/etc/systemd/system/cloud-init.target.wants/cloud-init-main.service                         |   1 +
 releng/airootfs/etc/systemd/system/cloud-init.target.wants/cloud-init-network.service                      |   1 +
 releng/airootfs/etc/systemd/system/dbus-org.freedesktop.ModemManager1.service                              |   1 +
 releng/airootfs/etc/systemd/system/dbus-org.freedesktop.network1.service                                   |   1 +
 releng/airootfs/etc/systemd/system/dbus-org.freedesktop.resolve1.service                                   |   1 +
 releng/airootfs/etc/systemd/system/dbus-org.freedesktop.timesync1.service                                  |   1 +
 releng/airootfs/etc/systemd/system/etc-pacman.d-gnupg.mount                                                |   8 +++
 releng/airootfs/etc/systemd/system/getty@tty1.service.d/autologin.conf                                     |   3 ++
 releng/airootfs/etc/systemd/system/livecd-alsa-unmuter.service                                             |  13 +++++
 releng/airootfs/etc/systemd/system/livecd-talk.service                                                     |  20 ++++++++
 releng/airootfs/etc/systemd/system/multi-user.target.wants/ModemManager.service                            |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/choose-mirror.service                           |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/hv_fcopy_daemon.service                         |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/hv_kvp_daemon.service                           |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/hv_vss_daemon.service                           |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/iwd.service                                     |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/livecd-talk.service                             |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/pacman-init.service                             |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/sshd.service                                    |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/systemd-networkd.service                        |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/systemd-resolved.service                        |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/vboxservice.service                             |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/vmtoolsd.service                                |   1 +
 releng/airootfs/etc/systemd/system/multi-user.target.wants/vmware-vmblock-fuse.service                     |   1 +
 releng/airootfs/etc/systemd/system/network-online.target.wants/systemd-networkd-wait-online.service        |   1 +
 releng/airootfs/etc/systemd/system/pacman-init.service                                                     |  15 ++++++
 releng/airootfs/etc/systemd/system/sockets.target.wants/pcscd.socket                                       |   1 +
 releng/airootfs/etc/systemd/system/sockets.target.wants/systemd-networkd.socket                            |   1 +
 releng/airootfs/etc/systemd/system/sound.target.wants/livecd-alsa-unmuter.service                          |   1 +
 releng/airootfs/etc/systemd/system/sysinit.target.wants/systemd-time-wait-sync.service                     |   1 +
 releng/airootfs/etc/systemd/system/sysinit.target.wants/systemd-timesyncd.service                          |   1 +
 releng/airootfs/etc/systemd/system/systemd-networkd-wait-online.service.d/wait-for-only-one-interface.conf |   6 +++
 releng/airootfs/root/.automated_script.sh                                                                  |  43 ++++++++++++++++
 releng/airootfs/root/.gnupg/scdaemon.conf                                                                  |   4 ++
 releng/airootfs/root/.zlogin                                                                               |   6 +++
 releng/airootfs/usr/local/bin/Installation_guide                                                           |   5 ++
 releng/airootfs/usr/local/bin/choose-mirror                                                                |  29 +++++++++++
 releng/airootfs/usr/local/bin/livecd-sound                                                                 | 239 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 releng/airootfs/usr/local/share/livecd-sound/asound.conf.in                                                |   3 ++
 releng/bootstrap_packages                                                                                  |   2 +
 releng/efiboot/loader/entries/01-archiso-linux.conf                                                        |   5 ++
 releng/efiboot/loader/entries/02-archiso-speech-linux.conf                                                 |   5 ++
 releng/efiboot/loader/entries/03-archiso-memtest86+AA64.conf                                               |   4 ++
 releng/efiboot/loader/entries/03-archiso-memtest86+IA32.conf                                               |   4 ++
 releng/efiboot/loader/entries/03-archiso-memtest86+LOONGARCH64.conf                                        |   4 ++
 releng/efiboot/loader/entries/03-archiso-memtest86+x64.conf                                                |   4 ++
 releng/efiboot/loader/loader.conf                                                                          |   3 ++
 releng/grub/grub.cfg                                                                                       | 110 +++++++++++++++++++++++++++++++++++++++
 releng/grub/loopback.cfg                                                                                   |  83 ++++++++++++++++++++++++++++++
 releng/packages.aarch64                                                                                    | 118 ++++++++++++++++++++++++++++++++++++++++++
 releng/packages.x86_64                                                                                     | 135 ++++++++++++++++++++++++++++++++++++++++++++++++
 releng/pacman.conf                                                                                         | 100 ++++++++++++++++++++++++++++++++++++
 releng/profiledef.sh                                                                                       |  25 +++++++++
 releng/syslinux/archiso_head.cfg                                                                           |  28 ++++++++++
 releng/syslinux/archiso_pxe-linux.cfg                                                                      |  32 ++++++++++++
 releng/syslinux/archiso_pxe.cfg                                                                            |   5 ++
 releng/syslinux/archiso_sys-linux.cfg                                                                      |  20 ++++++++
 releng/syslinux/archiso_sys.cfg                                                                            |   8 +++
 releng/syslinux/archiso_tail.cfg                                                                           |  35 +++++++++++++
 releng/syslinux/splash.png                                                                                 | Bin 0 -> 45400 bytes
 releng/syslinux/syslinux.cfg                                                                               |  11 ++++
 85 files changed, 1302 insertions(+)
```

## Uncommitted changes at time of backup

```
 D out/arctix-0.0.2-x86_64.iso
 D releng/airootfs/etc/hostname
 D releng/airootfs/etc/locale.conf
 M releng/airootfs/etc/passwd
 M releng/airootfs/etc/shadow
 M releng/airootfs/etc/ssh/sshd_config.d/10-archiso.conf
 M releng/packages.x86_64
 M releng/syslinux/archiso_head.cfg
 M releng/syslinux/splash.png
?? out/arctix-0.0.3-x86_64.iso
?? releng/airootfs/home/arctix/.config/fish/
?? releng/airootfs/home/arctix/.config/illogical-impulse/
?? releng/airootfs/home/arctix/.config/quickshell/
?? releng/airootfs/root/.config/illogical-impulse/
?? releng/airootfs/root/.config/quickshell/
?? releng/airootfs/usr/share/
```
