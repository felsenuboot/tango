//! A modal progress dialog that runs an import job on a worker thread.
//!
//! The worker cannot touch widgets, so it sends `Progress` messages through a channel and a
//! future on the main loop applies them. That is the standard gtk-rs threading pattern.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::store::import::Report;

enum Progress {
    Status(String, Option<f64>),
    Finished(Result<String, String>),
}

pub fn run(
    parent: &adw::ApplicationWindow,
    job: impl FnOnce(Report) -> anyhow::Result<String> + Send + 'static,
    on_done: impl FnOnce(Result<String, String>) + 'static,
) {
    let label = gtk::Label::builder()
        .label("Starting…")
        .wrap(true)
        .xalign(0.0)
        .build();
    let bar = gtk::ProgressBar::new();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    content.append(&label);
    content.append(&bar);
    let view = adw::ToolbarView::builder().content(&content).build();
    view.add_top_bar(
        &adw::HeaderBar::builder()
            .show_end_title_buttons(false)
            .show_start_title_buttons(false)
            .build(),
    );
    let dialog = adw::Dialog::builder()
        .title("Importing")
        .content_width(420)
        .can_close(false)
        .child(&view)
        .build();
    dialog.present(Some(parent));

    // Pulse the bar while the job has not reported a fraction yet.
    let indeterminate = Rc::new(Cell::new(true));
    let pulse = glib::timeout_add_local(Duration::from_millis(120), {
        let bar = bar.clone();
        let indeterminate = indeterminate.clone();
        move || {
            if indeterminate.get() {
                bar.pulse();
            }
            glib::ControlFlow::Continue
        }
    });

    let (tx, rx) = async_channel::unbounded::<Progress>();
    std::thread::Builder::new()
        .name("tango-import".into())
        .spawn(move || {
            let mut report = |message: String, fraction: Option<f64>| {
                let _ = tx.send_blocking(Progress::Status(message, fraction));
            };
            let result = job(&mut report).map_err(|e| format!("{e:#}"));
            let _ = tx.send_blocking(Progress::Finished(result));
        })
        .expect("spawning the import thread");

    glib::spawn_future_local(async move {
        let mut on_done = Some(on_done);
        while let Ok(progress) = rx.recv().await {
            match progress {
                Progress::Status(message, fraction) => {
                    label.set_text(&message);
                    indeterminate.set(fraction.is_none());
                    if let Some(f) = fraction {
                        bar.set_fraction(f);
                    }
                }
                Progress::Finished(result) => {
                    pulse.remove();
                    dialog.set_can_close(true);
                    dialog.close();
                    if let Err(ref e) = result {
                        log::error!("job failed: {e}");
                    }
                    if let Some(done) = on_done.take() {
                        done(result);
                    }
                    break;
                }
            }
        }
    });
}
