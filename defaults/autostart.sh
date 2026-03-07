#!/bin/sh
## monotile autostart

## export environment for xdg-desktop-portal, screensharing, etc. 
systemctl --user import-environment WAYLAND_DISPLAY XDG_CURRENT_DESKTOP
dbus-update-activation-environment --systemd WAYLAND_DISPLAY XDG_CURRENT_DESKTOP

## notification daemon
# mako &

## clipboard history daemon
# wl-paste --watch cliphist store &

## status bar
# waybar &

## wallpaper
# swaybg -i ~/wallpaper.png &

## idle management
# swayidle \
#     timeout 600 "wlopm --off '*'" resume "wlopm --on '*'" \
#     timeout 1200 "systemctl suspend" \
#     before-sleep "playerctl pause" \
#     before-sleep "waylock -input-color 0x494806 -fail-color 0xcc241d" \
#     after-resume "wlopm --on '*'" \
#     lock "waylock -input-color 0x494806 -fail-color 0xcc241d" \
#     &

## battery warnings
# batsignal -e -w 10 -c 5 -d 3 -D 'systemctl suspend' &

## night light (adjust coordinates to your location)
# wlsunset -l 48.7 -L 11.4 &
