# Themes

Follow the system, or force light or dark; the forced schemes hold even on
desktops that push their own GTK palette. Beyond those: WaniKani (the
site's blue as accent on your base), WaniKani Dark, Light and Pink, and four
Sanzo Wada colour combinations from *A Dictionary of Color Combinations*.

![The same entry in the light theme](../../data/screenshots/light.png)

![WaniKani Pink](../../data/screenshots/theme-pink.png)

![Wada 295 · Dull Violet Black](../../data/screenshots/theme-wada-295.png)

## Custom CSS

A `~/.config/tango/style.css` loads on top of any theme at the next start.
The app's own classes are named `tango-…` (`.tango-headword`,
`.tango-reading`, `.tango-learned`, …); libadwaita's named colours
(`@accent_bg_color`, `@window_bg_color`, …) can be overridden with
`@define-color`.

---
[Guide index](README.md) · [Tour](../TOUR.md)
