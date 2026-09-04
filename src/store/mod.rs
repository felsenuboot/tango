//! SQLite storage: the imported dictionaries and the searches over them (`db`), and the user's own
//! data (`user`), two files with different rules. The dictionary file is derived and disposable;
//! the user file is migrated and never dropped.

pub mod csv;
pub mod db;
pub mod import;
pub mod user;

/// The current time as ISO 8601 in UTC, the timestamp format both databases use.
pub fn now_iso8601() -> String {
    gtk::glib::DateTime::now_utc()
        .and_then(|t| t.format_iso8601())
        .map(|s| s.to_string())
        .unwrap_or_default()
}
