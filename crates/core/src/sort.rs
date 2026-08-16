//! How directory listings are ordered. Lives in core so it can be persisted in
//! [`crate::Settings`]; serializes to its variant name (e.g. `"Modified"`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortField {
    Name,
    Size,
    Modified,
}

impl SortField {
    pub fn label(self) -> &'static str {
        match self {
            SortField::Name => "Name",
            SortField::Size => "Size",
            SortField::Modified => "Modified",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn toggle(self) -> Self {
        match self {
            SortOrder::Asc => SortOrder::Desc,
            SortOrder::Desc => SortOrder::Asc,
        }
    }
}
