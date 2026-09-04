//! Colour scheme: follow the system, or force light or dark.
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
}

impl Scheme {
    pub const ALL: [Scheme; 3] = [Scheme::System, Scheme::Light, Scheme::Dark];

    /// The name stored in the config file.
    pub fn name(self) -> &'static str {
        match self {
            Scheme::System => "system",
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
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
        }
    }

    fn adw(self) -> adw::ColorScheme {
        match self {
            Scheme::System => adw::ColorScheme::Default,
            Scheme::Light => adw::ColorScheme::ForceLight,
            Scheme::Dark => adw::ColorScheme::ForceDark,
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
        if scheme == Scheme::System || manager.is_high_contrast() {
            return;
        }
        match palette_css(scheme) {
            Some(css) => {
                let provider = gtk::CssProvider::new();
                provider.load_from_string(&css);
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
                );
                *slot.borrow_mut() = Some(provider);
            }
            None => log::warn!(
                "libadwaita stylesheet not found in resources; the desktop's GTK colours may override the {} scheme",
                scheme.name()
            ),
        }
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
