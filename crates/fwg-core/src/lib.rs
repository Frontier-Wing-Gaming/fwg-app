//! Platform-independent core of the Frontier Gaming Network app.
//!
//! Ported from the Expo app's `src/lib` (api, cache, auth, games) plus the pure logic that lived in
//! screens (server status labels, fallback streamer list, notification routes). Nothing here knows
//! about the UI toolkit or the OS: storage is behind [`storage::KeyValueStore`] so Android/iOS can
//! supply Keystore/Keychain-backed implementations.

pub mod api;
pub mod auth;
pub mod cache;
mod de;
pub mod error;
pub mod format;
pub mod games;
pub mod markdown;
pub mod models;
pub mod routes;
pub mod server_status;
pub mod storage;
pub mod streams;

pub use api::{ApiClient, ImageUpload};
pub use auth::Session;
pub use cache::{Cache, Resource, ResourceView};
pub use error::{ApiError, Error};
