//! Colour scheme: follow the system, or force light or dark; and the themes on top of those
//! (issue #58): WaniKani's blue or pink, and a few Sanzo Wada combinations, each a base scheme
//! plus a block of named-colour overrides from `themes.css`. A custom `style.css` in the config
//! directory is loaded by `ui::startup` for anything beyond that.
//!
//! Forcing a scheme takes two steps. `adw::StyleManager` switches libadwaita's stylesheet, but
//! that alone is not enough (issue #1): GTK loads the user's own `~/.config/gtk-4.0/gtk.css` at
//! `STYLE_PROVIDER_PRIORITY_USER`, above every application provider, and desktops such as
//! Hyprland with Matugen use that file to push their palette into all GTK apps through lines like
//! `@define-color window_bg_color #0f1416;`. A forced light scheme would still come out dark.
//!
//! So for a forced scheme Tango re-declares libadwaita's own named colours one priority above the
//! user's file. The colours are read from the stylesheet libadwaita ships as a GResource, so they
//! always match the installed version rather than a copy kept here. "Follow system" removes that
//! provider again, and the desktop palette applies as it does in every other GTK app. Colours the
//! stylesheet itself leaves to the runtime, such as the system accent colour, are not touched.

use std::cell::RefCell;

use gtk::{gdk, gio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    System,
    Light,
    Dark,
    /// WaniKani's blue as the accent, on whatever base the desktop gives.
    WaniKani,
    WaniKaniDark,
    WaniKaniLight,
    /// Kanji pink surfaces and accents on a light base.
    WaniKaniPink,
    /// Sanzo Wada, "A Dictionary of Color Combinations", by combination number.
    Wada295,
    Wada325,
    Wada344,
    Wada276,
}

impl Scheme {
    pub const ALL: [Scheme; 11] = [
        Scheme::System,
        Scheme::Light,
        Scheme::Dark,
        Scheme::WaniKani,
        Scheme::WaniKaniDark,
        Scheme::WaniKaniLight,
        Scheme::WaniKaniPink,
        Scheme::Wada295,
        Scheme::Wada325,
        Scheme::Wada344,
        Scheme::Wada276,
    ];

    /// The name stored in the config file.
    pub fn name(self) -> &'static str {
        match self {
            Scheme::System => "system",
            Scheme::Light => "light",
            Scheme::Dark => "dark",
            Scheme::WaniKani => "wanikani",
            Scheme::WaniKaniDark => "wanikani-dark",
            Scheme::WaniKaniLight => "wanikani-light",
            Scheme::WaniKaniPink => "wanikani-pink",
            Scheme::Wada295 => "wada-295",
            Scheme::Wada325 => "wada-325",
            Scheme::Wada344 => "wada-344",
            Scheme::Wada276 => "wada-276",
        }
    }

    /// The light, dark or system scheme a theme builds on.
    pub fn base(self) -> Scheme {
        match self {
            Scheme::WaniKani => Scheme::System,
            Scheme::WaniKaniDark | Scheme::Wada295 | Scheme::Wada325 | Scheme::Wada344 => Scheme::Dark,
            Scheme::WaniKaniLight | Scheme::WaniKaniPink | Scheme::Wada276 => Scheme::Light,
            base => base,
        }
    }

    /// The theme's own named colours from `themes.css`, empty for the plain schemes.
    fn extra_css(self) -> String {
        const THEMES: &str = include_str!("themes.css");
        let marker = format!("/* == {} */", self.name());
        let Some(start) = THEMES.find(&marker) else {
            return String::new();
        };
        let rest = &THEMES[start + marker.len()..];
        let end = rest.find("/* == ").unwrap_or(rest.len());
        rest[..end].to_string()
    }

    /// Unknown names fall back to following the system.
    pub fn from_name(name: &str) -> Scheme {
        Scheme::ALL
            .into_iter()
            .find(|s| s.name() == name)
            .unwrap_or(Scheme::System)
    }

    /// What the preferences dialog shows.
    pub fn label(self) -> &'static str {
        match self {
            Scheme::System => "Follow system",
            Scheme::Light => "Light",
            Scheme::Dark => "Dark",
            Scheme::WaniKani => "WaniKani",
            Scheme::WaniKaniDark => "WaniKani Dark",
            Scheme::WaniKaniLight => "WaniKani Light",
            Scheme::WaniKaniPink => "WaniKani Pink",
            Scheme::Wada295 => "Wada 295 · Dull Violet Black",
            Scheme::Wada325 => "Wada 325 · Deep Slate Green",
            Scheme::Wada344 => "Wada 344 · Lyons Blue",
            Scheme::Wada276 => "Wada 276 · Seashell Pink",
        }
    }

    fn adw(self) -> adw::ColorScheme {
        match self.base() {
            Scheme::Light => adw::ColorScheme::ForceLight,
            Scheme::Dark => adw::ColorScheme::ForceDark,
            _ => adw::ColorScheme::Default,
        }
    }
}

thread_local! {
    /// The palette provider currently installed for a forced scheme, if any.
    static OVERRIDE: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

/// Switches the whole application to `scheme`. Safe to call repeatedly.
pub fn apply(scheme: Scheme) {
    let manager = adw::StyleManager::default();
    manager.set_color_scheme(scheme.adw());
    let Some(display) = gdk::Display::default() else {
        return;
    };
    OVERRIDE.with(|slot| {
        if let Some(old) = slot.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(&display, &old);
        }
        // High contrast has its own named colours in the stylesheet; leave them alone.
        if manager.is_high_contrast() {
            return;
        }
        // A forced base re-declares libadwaita's palette; a theme adds its colours after it,
        // so its lines win. Following the system with a theme keeps the desktop's palette and
        // changes only what the theme names.
        let base = scheme.base();
        let palette = if base == Scheme::System {
            Some(String::new())
        } else {
            palette_css(base)
        };
        let css = match palette {
            Some(palette) => format!("{palette}\n{}", scheme.extra_css()),
            None => {
                log::warn!(
                    "libadwaita stylesheet not found in resources; the desktop's GTK colours may override the {} scheme",
                    scheme.name()
                );
                scheme.extra_css()
            }
        };
        if css.trim().is_empty() {
            return;
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&css);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
        );
        *slot.borrow_mut() = Some(provider);
    });
}

/// libadwaita's `@define-color` lines for `scheme`, or `None` if the stylesheet is not where the
/// known libadwaita versions keep it.
fn palette_css(scheme: Scheme) -> Option<String> {
    let read = |file: &str| {
        gio::resources_lookup_data(
            &format!("/org/gnome/Adwaita/styles/{file}"),
            gio::ResourceLookupFlags::NONE,
        )
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };
    // libadwaita 1.8 and later: one stylesheet, the dark values behind a media query.
    if let Some(css) = read("gtk.css") {
        let palette = parse_palette(&css);
        let mut lines = palette.unconditional;
        if scheme == Scheme::Dark {
            lines.extend(palette.dark_only);
        }
        return Some(lines.join("\n"));
    }
    // Older libadwaita: one palette file per scheme, next to base.css.
    let file = match scheme {
        Scheme::Dark => "defaults-dark.css",
        _ => "defaults-light.css",
    };
    read(file).map(|css| parse_palette(&css).unconditional.join("\n"))
}

/// The `@define-color` statements of a stylesheet: the top-level ones, and those inside
/// `@media (prefers-color-scheme: dark) { … }` blocks. Anything else (rules, other media queries
/// such as high contrast) is skipped.
#[derive(Debug, Default, PartialEq, Eq)]
struct Palette {
    unconditional: Vec<String>,
    dark_only: Vec<String>,
}

fn parse_palette(css: &str) -> Palette {
    let css = strip_comments(css);
    let mut palette = Palette::default();
    let mut depth = 0usize;
    let mut in_dark_block = false;
    let mut rest = css.as_str();
    while let Some(c) = rest.chars().next() {
        let statement_here = rest.starts_with("@define-color");
        if statement_here && (depth == 0 || (depth == 1 && in_dark_block)) {
            let end = rest.find(';').unwrap_or(rest.len());
            // Collapse whitespace so multi-line statements become one line each.
            let statement = rest[..end].split_whitespace().collect::<Vec<_>>().join(" ") + ";";
            if depth == 0 {
                palette.unconditional.push(statement);
            } else {
                palette.dark_only.push(statement);
            }
            rest = &rest[end..];
            continue;
        }
        if depth == 0 && rest.starts_with("@media") {
            let head_end = rest.find('{').unwrap_or(rest.len());
            let head: String = rest[..head_end].split_whitespace().collect();
            in_dark_block = head.contains("prefers-color-scheme:dark");
            rest = &rest[head_end..];
            continue;
        }
        match c {
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    in_dark_block = false;
                }
            }
            // Skip quoted strings so a brace inside one does not confuse the depth count.
            '"' | '\'' => {
                let close = rest[1..].find(c).map_or(rest.len(), |i| i + 2);
                rest = &rest[close..];
                continue;
            }
            _ => {}
        }
        rest = &rest[c.len_utf8()..];
    }
    palette
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = match rest[start + 2..].find("*/") {
            Some(end) => &rest[start + 2 + end + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_names_roundtrip() {
        for s in Scheme::ALL {
            assert_eq!(Scheme::from_name(s.name()), s);
        }
        assert_eq!(Scheme::from_name("purple"), Scheme::System);
    }

    #[test]
    fn every_theme_has_its_css_and_a_base() {
        for s in Scheme::ALL {
            let plain = matches!(s, Scheme::System | Scheme::Light | Scheme::Dark);
            assert_eq!(s.extra_css().is_empty(), plain, "{}", s.name());
            assert!(matches!(s.base(), Scheme::System | Scheme::Light | Scheme::Dark));
            assert!(
                !s.extra_css().contains("/* =="),
                "{} bleeds into the next",
                s.name()
            );
        }
        assert!(
            Scheme::WaniKaniPink
                .extra_css()
                .contains("accent_bg_color #ff00aa")
        );
    }

    #[test]
    fn single_stylesheet_layout() {
        let css = "/* palette { */\n@define-color blue_3 #3584e4;\n\
                   @define-color window_bg_color #fafafb;\n\
                   :root { --window-bg-color: @window_bg_color; }\n\
                   @media (prefers-color-scheme: dark) { @define-color window_bg_color #222226; \
                   :root { --x: 1; } }\n\
                   @media (prefers-contrast: more) { @define-color window_fg_color black; }\n\
                   label { content: \"}\"; }\n\
                   @define-color theme_bg_color\n  @window_bg_color;\n";
        let palette = parse_palette(css);
        assert_eq!(
            palette.unconditional,
            [
                "@define-color blue_3 #3584e4;",
                "@define-color window_bg_color #fafafb;",
                "@define-color theme_bg_color @window_bg_color;",
            ]
        );
        assert_eq!(palette.dark_only, ["@define-color window_bg_color #222226;"]);
    }

    #[test]
    fn per_scheme_file_layout() {
        let css = "@define-color window_bg_color #242424;\n@define-color window_fg_color #ffffff;\n";
        let palette = parse_palette(css);
        assert_eq!(palette.unconditional.len(), 2);
        assert!(palette.dark_only.is_empty());
    }
}
