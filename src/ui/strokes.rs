//! Stroke order diagrams: KanjiVG's SVG paths drawn with cairo, stroke by stroke.
//!
//! KanjiVG uses only a handful of SVG path commands (move, line, cubic curves, absolute and
//! relative), so a small parser turns the `d` strings into segments once and the drawing area
//! paints them scaled to its size, greying out the strokes not yet "written" and numbering each
//! stroke at its start.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

/// The 109×109 box KanjiVG draws in.
const BOX: f64 = 109.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    CurveTo(f64, f64, f64, f64, f64, f64),
    Close,
}

/// Parses SVG path data into absolute segments. Unknown commands end the parse; whatever was
/// read stays, so a surprise in the data draws part of a stroke rather than nothing.
pub fn parse_path(d: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut numbers: Vec<f64> = Vec::new();
    let mut command = None;
    let (mut x, mut y) = (0.0, 0.0);
    let (mut start_x, mut start_y) = (0.0, 0.0);
    let mut last_control: Option<(f64, f64)> = None;
    let mut chars = d.chars().peekable();
    let mut token = String::new();
    let flush = |token: &mut String, numbers: &mut Vec<f64>| {
        if !token.is_empty() {
            if let Ok(n) = token.parse::<f64>() {
                numbers.push(n);
            }
            token.clear();
        }
    };
    loop {
        let c = chars.next();
        let is_command = c.is_some_and(|c| c.is_ascii_alphabetic());
        let is_end = c.is_none();
        if is_command
            || is_end
            || c == Some(',')
            || c == Some(' ')
            || c == Some('-') && !token.is_empty() && !token.ends_with('e')
        {
            flush(&mut token, &mut numbers);
            if c == Some('-') {
                token.push('-');
            }
        } else if let Some(c) = c {
            token.push(c);
        }
        if is_command || is_end {
            // Apply the previous command with the numbers gathered for it.
            if let Some(cmd) = command {
                let relative = char::is_ascii_lowercase(&cmd);
                let abs = |v: f64, base: f64| if relative { base + v } else { v };
                match cmd.to_ascii_uppercase() {
                    'M' => {
                        for (i, pair) in numbers.chunks(2).enumerate() {
                            if pair.len() < 2 {
                                break;
                            }
                            x = abs(pair[0], x);
                            y = abs(pair[1], y);
                            if i == 0 {
                                out.push(Segment::MoveTo(x, y));
                                (start_x, start_y) = (x, y);
                            } else {
                                out.push(Segment::LineTo(x, y));
                            }
                        }
                        last_control = None;
                    }
                    'L' => {
                        for pair in numbers.chunks(2) {
                            if pair.len() < 2 {
                                break;
                            }
                            x = abs(pair[0], x);
                            y = abs(pair[1], y);
                            out.push(Segment::LineTo(x, y));
                        }
                        last_control = None;
                    }
                    'C' => {
                        for six in numbers.chunks(6) {
                            if six.len() < 6 {
                                break;
                            }
                            let (x1, y1) = (abs(six[0], x), abs(six[1], y));
                            let (x2, y2) = (abs(six[2], x), abs(six[3], y));
                            x = abs(six[4], x);
                            y = abs(six[5], y);
                            out.push(Segment::CurveTo(x1, y1, x2, y2, x, y));
                            last_control = Some((x2, y2));
                        }
                    }
                    'S' => {
                        for four in numbers.chunks(4) {
                            if four.len() < 4 {
                                break;
                            }
                            // The first control point mirrors the previous curve's second one.
                            let (x1, y1) = match last_control {
                                Some((cx, cy)) => (2.0 * x - cx, 2.0 * y - cy),
                                None => (x, y),
                            };
                            let (x2, y2) = (abs(four[0], x), abs(four[1], y));
                            x = abs(four[2], x);
                            y = abs(four[3], y);
                            out.push(Segment::CurveTo(x1, y1, x2, y2, x, y));
                            last_control = Some((x2, y2));
                        }
                    }
                    'Z' => {
                        out.push(Segment::Close);
                        (x, y) = (start_x, start_y);
                        last_control = None;
                    }
                    _ => return out,
                }
                numbers.clear();
            }
            command = c;
        }
        if is_end {
            break;
        }
    }
    out
}

/// A drawing area that shows a kanji's strokes, the first `shown` of them painted.
pub struct Diagram {
    pub area: gtk::DrawingArea,
    strokes: Rc<RefCell<Vec<Vec<Segment>>>>,
    shown: Rc<Cell<usize>>,
    timer: Rc<RefCell<Option<glib::SourceId>>>,
}

impl Diagram {
    pub fn new(size: i32) -> Self {
        let area = gtk::DrawingArea::builder()
            .content_width(size)
            .content_height(size)
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .css_classes(["tango-diagram"])
            .build();
        let strokes = Rc::new(RefCell::new(Vec::new()));
        let shown = Rc::new(Cell::new(0));
        area.set_draw_func({
            let strokes = strokes.clone();
            let shown = shown.clone();
            move |area, cr, width, height| draw(area, cr, width, height, &strokes.borrow(), shown.get())
        });
        Self {
            area,
            strokes,
            shown,
            timer: Rc::new(RefCell::new(None)),
        }
    }

    pub fn set_paths(&self, paths: &[String]) {
        self.stop();
        *self.strokes.borrow_mut() = paths.iter().map(|p| parse_path(p)).collect();
        self.shown.set(paths.len());
        self.area.queue_draw();
    }

    /// Writes the kanji stroke by stroke, one every 350 ms.
    pub fn play(&self) {
        self.stop();
        let total = self.strokes.borrow().len();
        if total == 0 {
            return;
        }
        self.shown.set(0);
        self.area.queue_draw();
        let shown = self.shown.clone();
        let area = self.area.clone();
        let timer = self.timer.clone();
        let id = glib::timeout_add_local(Duration::from_millis(350), move || {
            shown.set(shown.get() + 1);
            area.queue_draw();
            if shown.get() >= total {
                *timer.borrow_mut() = None;
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        *self.timer.borrow_mut() = Some(id);
    }

    fn stop(&self) {
        if let Some(id) = self.timer.borrow_mut().take() {
            id.remove();
        }
    }
}

fn draw(
    area: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    width: i32,
    height: i32,
    strokes: &[Vec<Segment>],
    shown: usize,
) {
    let size = f64::from(width.min(height));
    let scale = size / BOX;
    cr.scale(scale, scale);
    let fg = area.color();
    // Grid: the box and its centre lines, faint.
    cr.set_source_rgba(
        f64::from(fg.red()),
        f64::from(fg.green()),
        f64::from(fg.blue()),
        0.15,
    );
    cr.set_line_width(0.6);
    cr.rectangle(0.5, 0.5, BOX - 1.0, BOX - 1.0);
    cr.move_to(BOX / 2.0, 0.0);
    cr.line_to(BOX / 2.0, BOX);
    cr.move_to(0.0, BOX / 2.0);
    cr.line_to(BOX, BOX / 2.0);
    let _ = cr.stroke();
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    cr.set_line_width(4.0);
    for (i, stroke) in strokes.iter().enumerate() {
        let alpha = if i < shown { 1.0 } else { 0.18 };
        cr.set_source_rgba(
            f64::from(fg.red()),
            f64::from(fg.green()),
            f64::from(fg.blue()),
            alpha,
        );
        for seg in stroke {
            match *seg {
                Segment::MoveTo(x, y) => cr.move_to(x, y),
                Segment::LineTo(x, y) => cr.line_to(x, y),
                Segment::CurveTo(x1, y1, x2, y2, x, y) => cr.curve_to(x1, y1, x2, y2, x, y),
                Segment::Close => cr.close_path(),
            }
        }
        let _ = cr.stroke();
        // The stroke number at its start, only once the stroke is painted.
        if i < shown
            && let Some(Segment::MoveTo(x, y)) = stroke.first()
        {
            cr.set_font_size(7.0);
            cr.set_source_rgba(0.85, 0.2, 0.2, 0.95);
            cr.move_to(x - 3.0, y - 2.0);
            let _ = cr.show_text(&(i + 1).to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kanjivg_paths_parse() {
        let segs = parse_path("M36.05,19.5c0.07,0.61-0.17,1.57-0.63,2.21C30,29.25,23.75,36.25,11.25,45.29");
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0], Segment::MoveTo(36.05, 19.5));
        let Segment::CurveTo(x1, y1, _, _, x, y) = segs[1] else {
            panic!("expected a curve")
        };
        assert!((x1 - 36.12).abs() < 1e-9 && (y1 - 20.11).abs() < 1e-9);
        assert!((x - 35.42).abs() < 1e-9 && (y - 21.71).abs() < 1e-9);
        assert_eq!(segs[2], Segment::CurveTo(30.0, 29.25, 23.75, 36.25, 11.25, 45.29));
        assert_eq!(
            parse_path("M1,2 L3,4 z"),
            [
                Segment::MoveTo(1.0, 2.0),
                Segment::LineTo(3.0, 4.0),
                Segment::Close
            ]
        );
        // Smooth curves mirror the previous control point; unknown commands stop the parse.
        let s = parse_path("M0,0C1,1,2,2,3,3S5,5,6,6Q1,1,2,2");
        assert_eq!(s.len(), 3);
        assert_eq!(s[2], Segment::CurveTo(4.0, 4.0, 5.0, 5.0, 6.0, 6.0));
        assert_eq!(
            parse_path("m1-2l-3-4"),
            [Segment::MoveTo(1.0, -2.0), Segment::LineTo(-2.0, -6.0)]
        );
    }
}
