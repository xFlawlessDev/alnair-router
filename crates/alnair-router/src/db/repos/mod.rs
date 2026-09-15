//! Repository modules.

use serde::Deserialize;

pub mod aliases;
pub mod api_keys;
pub mod combos;
pub mod connection_accounts;
pub mod connections;
pub mod key_plans;
pub mod usage;

/// Deserializes a present-but-null field as `Some(None)`, so PATCH bodies can
/// distinguish "leave unchanged" (field absent) from "clear it" (field `null`).
pub(crate) fn double_option<'de, D, T>(
    deserializer: D,
) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
