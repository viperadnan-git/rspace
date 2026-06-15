//! Shared rspace domain: on-disk layout, query cache, settings, sorting.

pub mod cache;
pub mod paths;
pub mod settings;
pub mod sort;

pub use cache::{Lookup, QueryCache};
pub use paths::Paths;
pub use settings::{Settings, SettingsStore};
pub use sort::{SortField, SortOrder};
