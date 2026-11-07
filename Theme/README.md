# Valkyrie Icon Theme — early usable prototype

This is a first, editable SVG prototype based on the approved plum/lavender direction.
It includes folders, common file types, drives, network and trash icons.

## Install for the current user
Copy the `Valkyrie` directory into `~/.local/share/icons/`.

```sh
mkdir -p ~/.local/share/icons
cp -r Valkyrie ~/.local/share/icons/
```

Then select **Valkyrie** in your desktop's icon-theme settings. Availability of theme selection varies by desktop.

## Notes
- SVGs are scalable and can be edited.
- This is an initial icon-theme package, not yet a complete replacement for all Linux icon names.
- Unprovided icon names fall back to Adwaita/hicolor.
- GTK application controls are controlled by the GTK theme separately from the icon theme.
- The light/dark appearance is designed to work on both backgrounds; the icons themselves do not switch variants yet.
