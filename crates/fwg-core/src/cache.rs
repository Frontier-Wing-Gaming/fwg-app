//! Offline content cache (port of `useCachedResource` in `src/lib/cache.ts`).
//!
//! Flow per screen: [`Resource::load_cached`] shows the saved copy at once, then the screen
//! fetches fresh data and passes the result to [`Resource::finish_refresh`]. A failed refresh keeps
//! whatever data is on screen. [`Resource::view`] says which status banner to show.
//! Storage keys and the envelope format (`{data, savedAt}`) match the Expo app.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{error::ApiError, storage::KeyValueStore};

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    data: T,
    /// Milliseconds since the Unix epoch.
    #[serde(rename = "savedAt")]
    saved_at: i64,
}

#[derive(Clone)]
pub struct Cache {
    store: Arc<dyn KeyValueStore>,
}

impl Cache {
    pub fn new(store: Arc<dyn KeyValueStore>) -> Self {
        Self { store }
    }

    fn key(key: &str) -> String {
        format!("fwg.cache.{key}")
    }

    /// The saved value and when it was saved (ms). Unreadable or outdated entries count as missing.
    pub fn read<T: DeserializeOwned>(&self, key: &str) -> Option<(T, i64)> {
        let raw = self.store.get(&Self::key(key)).ok()??;
        let env: Envelope<T> = serde_json::from_str(&raw).ok()?;
        Some((env.data, env.saved_at))
    }

    /// Saves `data`, returning the timestamp used.
    pub fn write<T: Serialize>(&self, key: &str, data: &T) -> std::io::Result<i64> {
        let saved_at = now_ms();
        let raw = serde_json::to_string(&Envelope { data, saved_at })?;
        self.store.set(&Self::key(key), &raw)?;
        Ok(saved_at)
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// What the status area above a list should show (the Expo `ResourceState` component).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceView {
    /// Nothing loaded yet: spinner.
    Loading,
    /// "Unable to load" card with a retry button.
    Failed { error: String },
    /// "Offline copy · updated …" banner with retry.
    Offline { updated_at: Option<i64> },
    /// Fresh data, no banner.
    Ready,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resource<T> {
    pub key: String,
    pub data: Option<T>,
    pub loading: bool,
    pub refreshing: bool,
    pub error: Option<String>,
    pub is_cached: bool,
    /// Milliseconds since the Unix epoch.
    pub updated_at: Option<i64>,
}

impl<T: Serialize + DeserializeOwned> Resource<T> {
    /// Starts in the loading state with the saved copy, if any, already filled in.
    /// The caller should start a refresh right after.
    pub fn load_cached(cache: &Cache, key: &str) -> Self {
        let mut res = Self {
            key: key.to_owned(),
            data: None,
            loading: true,
            refreshing: true,
            error: None,
            is_cached: false,
            updated_at: None,
        };
        if let Some((data, saved_at)) = cache.read(key) {
            res.data = Some(data);
            res.updated_at = Some(saved_at);
            res.is_cached = true;
            res.loading = false;
        }
        res
    }

    /// Call when a manual refresh (retry / pull-to-refresh) starts.
    pub fn begin_refresh(&mut self) {
        self.refreshing = true;
    }

    /// Applies a fetch result. Fresh data is saved; on error the current data is kept.
    pub fn finish_refresh(&mut self, cache: &Cache, result: Result<T, ApiError>) {
        match result {
            Ok(fresh) => {
                // A failed save only costs the offline copy; the fresh data still shows.
                let saved_at = cache.write(&self.key, &fresh).unwrap_or_else(|_| now_ms());
                self.data = Some(fresh);
                self.updated_at = Some(saved_at);
                self.is_cached = false;
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.loading = false;
        self.refreshing = false;
    }
}

impl<T> Resource<T> {
    pub fn view(&self) -> ResourceView {
        if self.loading {
            ResourceView::Loading
        } else if let (Some(error), false) = (&self.error, self.is_cached) {
            ResourceView::Failed {
                error: error.clone(),
            }
        } else if self.is_cached {
            ResourceView::Offline {
                updated_at: self.updated_at,
            }
        } else {
            ResourceView::Ready
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStore;

    fn cache() -> Cache {
        Cache::new(Arc::new(MemoryStore::default()))
    }

    #[test]
    fn empty_cache_starts_loading() {
        let r = Resource::<Vec<u32>>::load_cached(&cache(), "news");
        assert_eq!(r.view(), ResourceView::Loading);
        assert!(r.data.is_none());
    }

    #[test]
    fn cached_copy_shows_offline_until_refreshed() {
        let c = cache();
        c.write("news", &vec![1u32]).unwrap();
        let mut r = Resource::<Vec<u32>>::load_cached(&c, "news");
        assert_eq!(r.data, Some(vec![1]));
        assert!(matches!(
            r.view(),
            ResourceView::Offline {
                updated_at: Some(_)
            }
        ));

        r.finish_refresh(&c, Err(ApiError::InvalidResponse));
        assert_eq!(
            r.data,
            Some(vec![1]),
            "failed refresh keeps the cached copy"
        );
        assert!(matches!(r.view(), ResourceView::Offline { .. }));

        r.finish_refresh(&c, Ok(vec![2]));
        assert_eq!(r.view(), ResourceView::Ready);
        assert_eq!(c.read::<Vec<u32>>("news").unwrap().0, vec![2]);
    }

    #[test]
    fn failure_without_cache_shows_error() {
        let c = cache();
        let mut r = Resource::<Vec<u32>>::load_cached(&c, "news");
        r.finish_refresh(&c, Err(ApiError::InvalidResponse));
        assert!(matches!(r.view(), ResourceView::Failed { .. }));
    }

    #[test]
    fn reads_envelopes_written_by_the_expo_app() {
        let store = Arc::new(MemoryStore::default());
        store
            .set(
                "fwg.cache.streams",
                r#"{"data":{"items":[]},"savedAt":1762400000000}"#,
            )
            .unwrap();
        let c = Cache::new(store);
        let (data, at) = c.read::<serde_json::Value>("streams").unwrap();
        assert_eq!(
            (data["items"].as_array().unwrap().len(), at),
            (0, 1762400000000)
        );
        assert!(
            c.read::<Vec<String>>("streams").is_none(),
            "shape mismatch counts as missing"
        );
    }
}
