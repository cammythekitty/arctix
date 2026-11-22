# fix for screen readers
if grep -Fqa 'accessibility=' /proc/cmdline &> /dev/null; then
    setopt SINGLE_LINE_ZLE
fi

~/.automated_script.sh

[ -z "$WAYLAND_DISPLAY" ] && [ "$XDG_VTNR" = 1 ] && exec Hyprland
