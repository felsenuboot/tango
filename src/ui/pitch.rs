//! The pitch accent graph beside a reading, the way Takoboto and the NHK accent dictionary draw
//! it: one step per mora, high or low, the drop marked, and a last dashed step for the particle
//! that would follow (high after a flat word, low after a drop). Wadoku gives the accent number
//! (issue #4); this turns it into a picture (issue #69).

use adw::prelude::*;

/// Width of one mora step and the height of the graph, in pixels.
const STEP: i32 = 22;
const HEIGHT: i32 = 18;

/// Splits a kana reading into moras: a small ゃゅょ (or ャュョ, ぁぃぅぇぉ) joins the kana before it.
pub fn moras(reading: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in reading.chars() {
        let small = matches!(
            c,
            'ゃ' | 'ゅ'
                | 'ょ'
                | 'ャ'
                | 'ュ'
                | 'ョ'
                | 'ぁ'
                | 'ぃ'
                | 'ぅ'
                | 'ぇ'
                | 'ぉ'
                | 'ァ'
                | 'ィ'
                | 'ゥ'
                | 'ェ'
                | 'ォ'
                | 'ゎ'
                | 'ヮ'
        );
        match out.last_mut() {
            Some(last) if small => last.push(c),
            _ => out.push(c.to_string()),
        }
    }
    out
}

/// High (true) or low per mora, plus one value for the particle after the word. Accent 0
/// (heiban): low, then high to the end and beyond. Accent 1: high, then low. Accent n: low, high
/// up to mora n, low after.
pub fn pattern(moras: usize, accent: u8) -> Vec<bool> {
    let accent = usize::from(accent);
    (1..=moras + 1)
        .map(|i| match accent {
            0 => i > 1,
            1 => i == 1,
            n => i > 1 && i <= n,
        })
        .collect()
}

/// The graph for `reading` with `accent`: the line above, the moras as labels beneath.
pub fn graph(reading: &str, accent: u8) -> gtk::Box {
    let moras = moras(reading);
    let pattern = pattern(moras.len(), accent);
    let count = moras.len();
    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .valign(gtk::Align::Center)
        .tooltip_text(if accent == 0 {
            "Pitch accent: flat (heiban), no drop".to_string()
        } else {
            format!("Pitch accent: the pitch drops after mora {accent}")
        })
        .css_classes(["tango-pitch-graph"])
        .build();
    let area = gtk::DrawingArea::builder()
        .content_width(STEP * (count as i32 + 1))
        .content_height(HEIGHT)
        .build();
    area.set_draw_func(move |area, cr, _width, height| {
        let fg = area.color();
        cr.set_source_rgba(
            f64::from(fg.red()),
            f64::from(fg.green()),
            f64::from(fg.blue()),
            0.85,
        );
        cr.set_line_width(2.0);
        let step = f64::from(STEP);
        let y = |high: bool| if high { 4.0 } else { f64::from(height) - 4.0 };
        // Solid line over the word, a dashed step for the particle after it.
        for (i, high) in pattern.iter().enumerate() {
            let x0 = step * i as f64 + 3.0;
            let x1 = x0 + step - 6.0;
            if i == count {
                cr.set_dash(&[3.0, 3.0], 0.0);
            }
            cr.move_to(x0, y(*high));
            cr.line_to(x1, y(*high));
            let _ = cr.stroke();
            if i + 1 < pattern.len() && pattern[i + 1] != *high {
                cr.move_to(x1, y(*high));
                cr.line_to(x0 + step, y(pattern[i + 1]));
                let _ = cr.stroke();
            }
        }
        cr.set_dash(&[], 0.0);
        for (i, high) in pattern.iter().enumerate().take(count) {
            cr.arc(
                step * i as f64 + step / 2.0,
                y(*high),
                2.5,
                0.0,
                std::f64::consts::TAU,
            );
            let _ = cr.fill();
        }
    });
    column.append(&area);
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    for mora in &moras {
        let label = gtk::Label::builder()
            .label(mora)
            .width_request(STEP)
            .xalign(0.5)
            .css_classes(["caption"])
            .build();
        row.append(&label);
    }
    column.append(&row);
    column
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_kana_join_their_mora() {
        assert_eq!(moras("きょうと"), ["きょ", "う", "と"]);
        assert_eq!(moras("ねこ"), ["ね", "こ"]);
        assert_eq!(moras("シャツ"), ["シャ", "ツ"]);
        assert!(moras("").is_empty());
    }

    #[test]
    fn patterns_follow_the_accent_number() {
        assert_eq!(pattern(2, 0), [false, true, true]); // ねこ heiban: low high, particle high
        assert_eq!(pattern(2, 1), [true, false, false]); // atamadaka
        assert_eq!(pattern(3, 2), [false, true, false, false]); // drop after the second mora
        assert_eq!(pattern(3, 3), [false, true, true, false]); // odaka: high to the end, particle low
    }
}
