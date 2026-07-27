//! Private compatibility boundary for the upstream runtime.
//!
//! Product code uses Sunsetz-owned names. The values in this module preserve
//! compatibility with the installed upstream CLI and its existing data.

use std::path::PathBuf;

pub const PRODUCT_HOME_ENV: &str = "SUNSETZ_HOME";
pub const LEGACY_PRODUCT_HOME_ENV: &str = "GROK_APP_HOME";
pub const PRODUCT_ACP_ENV: &str = "SUNSETZ_ACP";
pub const LEGACY_PRODUCT_ACP_ENV: &str = "GROK_APP_ACP";

pub fn product_home_override() -> Option<PathBuf> {
    std::env::var(PRODUCT_HOME_ENV)
        .or_else(|_| std::env::var(LEGACY_PRODUCT_HOME_ENV))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

pub fn use_mock_runtime() -> bool {
    std::env::var(PRODUCT_ACP_ENV)
        .or_else(|_| std::env::var(LEGACY_PRODUCT_ACP_ENV))
        .map(|value| value.eq_ignore_ascii_case("mock"))
        .unwrap_or(false)
}

pub fn public_model_label(label: &str) -> String {
    label.replace("Grok", "Sunsetz").replace("grok", "Sunsetz")
}
