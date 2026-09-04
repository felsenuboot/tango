//! API tokens live in the desktop keyring (the Secret Service, through libsecret), never in the
//! config file. One item per provider, found by the `provider` attribute.
//!
//! Headless runs and CI have no keyring; `TANGO_<PROVIDER>_TOKEN` in the environment stands in
//! for a stored token there (read only, never written).

use std::collections::HashMap;

use anyhow::Context;
use gtk::gio;
use libsecret::{Schema, SchemaAttributeType, SchemaFlags};

const SCHEMA_NAME: &str = "io.github.felsenuboot.Tango";

fn schema() -> Schema {
    Schema::new(
        SCHEMA_NAME,
        SchemaFlags::NONE,
        HashMap::from([("provider", SchemaAttributeType::String)]),
    )
}

fn env_name(provider: &str) -> String {
    format!("TANGO_{}_TOKEN", provider.to_uppercase())
}

/// Stores `token` for `provider`, replacing an older one.
pub fn store(provider: &str, token: &str) -> anyhow::Result<()> {
    libsecret::password_store_sync(
        Some(&schema()),
        HashMap::from([("provider", provider)]),
        Some(libsecret::COLLECTION_DEFAULT),
        &format!("Tango: {provider} API token"),
        token,
        gio::Cancellable::NONE,
    )
    .with_context(|| format!("storing the {provider} token in the keyring"))?;
    Ok(())
}

/// The stored token, or the environment's stand-in, or nothing.
pub fn lookup(provider: &str) -> anyhow::Result<Option<String>> {
    if let Ok(token) = std::env::var(env_name(provider))
        && !token.trim().is_empty()
    {
        return Ok(Some(token.trim().to_string()));
    }
    let found = libsecret::password_lookup_sync(
        Some(&schema()),
        HashMap::from([("provider", provider)]),
        gio::Cancellable::NONE,
    )
    .with_context(|| format!("reading the {provider} token from the keyring"))?;
    Ok(found.map(|s| s.to_string()))
}

/// Forgets the token; nothing to do when there was none.
pub fn clear(provider: &str) -> anyhow::Result<()> {
    libsecret::password_clear_sync(
        Some(&schema()),
        HashMap::from([("provider", provider)]),
        gio::Cancellable::NONE,
    )
    .with_context(|| format!("removing the {provider} token from the keyring"))?;
    Ok(())
}
