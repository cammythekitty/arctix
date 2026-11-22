if status is-login; and test -z "$WAYLAND_DISPLAY"; and test (tty) = /dev/tty1
    start-hyprland
end
