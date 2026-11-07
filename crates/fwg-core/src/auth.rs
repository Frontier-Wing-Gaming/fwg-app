//! Staff session (port of `AuthProvider` in `src/lib/auth.tsx`).
//!
//! The token and user live in a secure [`KeyValueStore`] under the same keys as the Expo app.
//! Any staff request that gets a 401 for the *current* token signs the user out and leaves a
//! notice; a late 401 for a token that was already replaced is ignored. Storage writes happen
//! under the session lock, so a 401 cleanup can never erase a newer sign-in.

use std::sync::{Arc, Mutex, MutexGuard, Weak};

use crate::{api::ApiClient, error::Error, models::User, storage::KeyValueStore};

const TOKEN_KEY: &str = "fwg.staff.token";
const USER_KEY: &str = "fwg.staff.user";
pub const SESSION_EXPIRED: &str = "Your staff session expired. Please sign in again.";

#[derive(Default)]
struct State {
    token: Option<String>,
    user: Option<User>,
    notice: Option<String>,
}

struct Inner {
    state: Mutex<State>,
    store: Arc<dyn KeyValueStore>,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Clears memory first so the app is signed out even if the store delete fails.
    fn clear(&self, state: &mut State) -> std::io::Result<()> {
        state.token = None;
        state.user = None;
        let a = self.store.delete(TOKEN_KEY);
        let b = self.store.delete(USER_KEY);
        a.and(b)
    }
}

#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
    api: ApiClient,
}

impl Session {
    /// Restores a saved session and installs the 401 handler on `api` (shared by its clones).
    pub fn restore(api: ApiClient, secure_store: Arc<dyn KeyValueStore>) -> Self {
        let token = secure_store.get(TOKEN_KEY).ok().flatten();
        let user = secure_store
            .get(USER_KEY)
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<User>(&raw).ok());
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                token,
                user,
                notice: None,
            }),
            store: secure_store,
        });

        let weak: Weak<Inner> = Arc::downgrade(&inner);
        api.set_unauthorized_handler(Some(Arc::new(move |rejected: &str| {
            let Some(inner) = weak.upgrade() else { return };
            let mut state = inner.lock();
            if state.token.as_deref() != Some(rejected) {
                return;
            }
            state.notice = Some(SESSION_EXPIRED.to_owned());
            let _ = inner.clear(&mut state);
        })));

        Self { inner, api }
    }

    pub fn api(&self) -> &ApiClient {
        &self.api
    }

    pub fn token(&self) -> Option<String> {
        self.inner.lock().token.clone()
    }

    pub fn user(&self) -> Option<User> {
        self.inner.lock().user.clone()
    }

    pub fn is_signed_in(&self) -> bool {
        self.inner.lock().token.is_some()
    }

    /// The "session expired" message, if one is waiting. Reading it clears it.
    pub fn take_notice(&self) -> Option<String> {
        self.inner.lock().notice.take()
    }

    pub fn has_notice(&self) -> bool {
        self.inner.lock().notice.is_some()
    }

    pub async fn sign_in(&self, email: &str, password: &str) -> Result<User, Error> {
        let result = self.api.login(email.trim(), password).await?;
        let user_json = serde_json::to_string(&result.user).map_err(std::io::Error::other)?;

        let mut state = self.inner.lock();
        self.inner.store.set(TOKEN_KEY, &result.token)?;
        if let Err(e) = self.inner.store.set(USER_KEY, &user_json) {
            let _ = self.inner.store.delete(TOKEN_KEY);
            return Err(e.into());
        }
        state.token = Some(result.token);
        state.user = Some(result.user.clone());
        state.notice = None;
        Ok(result.user)
    }

    /// Tells the website to end the session (errors ignored), then forgets it locally.
    pub async fn sign_out(&self) -> Result<(), Error> {
        if let Some(token) = self.token() {
            let _ = self.api.logout(&token).await;
        }
        let mut state = self.inner.lock();
        self.inner.clear(&mut state)?;
        Ok(())
    }
}
