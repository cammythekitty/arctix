hl.bind("CTRL+SUPER+ALT+Slash", hl.dsp.exec_cmd("xdg-open ~/.config/hypr/custom/keybinds.lua"), {description = "Edit user keybinds"} )

-- PrtSc: start the interactive region snip (pick two corners), same as Super+Shift+S.
-- Drop the stock full-screen-to-clipboard Print bind first. Ctrl+Print still takes a full-screen shot.
hl.unbind("Print")
hl.bind("Print", hl.dsp.global("quickshell:regionScreenshot"), { description = "Utilities: Screen snip" })
hl.bind("Print",
    hl.dsp.exec_cmd("qs -c $qsConfig ipc call TEST_ALIVE || pidof slurp || hyprshot --freeze --clipboard-only --mode region --silent"))
