//! Website API client (port of `src/lib/api.ts`).

use std::sync::{Arc, RwLock};

use reqwest::{
    Method,
    header::{ACCEPT, CONTENT_TYPE},
    multipart,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::{error::ApiError, models::*};

pub const DEFAULT_API_ROOT: &str = "https://www.frontierwinggaming.network/api/v1";
pub const DISCORD_INVITE: &str = "https://discord.gg/B75qnDTaME";
const STAFF_TOKEN_HEADER: &str = "X-FWG-Staff-Token";

/// Called with the rejected token whenever a staff request gets a 401.
pub type UnauthorizedHandler = Arc<dyn Fn(&str) + Send + Sync>;

/// An image picked for a gallery upload.
#[derive(Debug, Clone)]
pub struct ImageUpload {
    pub bytes: Vec<u8>,
    /// Defaults to `upload.jpg`.
    pub file_name: Option<String>,
    /// Defaults to `image/jpeg`.
    pub mime_type: Option<String>,
}

enum Body {
    Empty,
    Json(Value),
    Multipart(multipart::Form),
}

/// Cheap to clone; clones share the HTTP connection pool and the unauthorized handler.
#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    root: Arc<str>,
    on_unauthorized: Arc<RwLock<Option<UnauthorizedHandler>>>,
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new(DEFAULT_API_ROOT)
    }
}

impl ApiClient {
    pub fn new(api_root: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("FrontierGamingNetwork/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("TLS backend is compiled in");
        Self {
            http,
            root: api_root.trim_end_matches('/').into(),
            on_unauthorized: Arc::default(),
        }
    }

    pub fn api_root(&self) -> &str {
        &self.root
    }

    /// The shared HTTP client, for non-API downloads such as images (same TLS setup and pool).
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Website origin, used to resolve asset paths such as game banner images.
    pub fn site_root(&self) -> &str {
        site_root(&self.root)
    }

    /// Resolve a site asset path (e.g. `images/rust-banner.png`) to an absolute URL.
    pub fn site_asset(&self, path: &str) -> String {
        site_asset(self.site_root(), path)
    }

    pub fn set_unauthorized_handler(&self, handler: Option<UnauthorizedHandler>) {
        *self
            .on_unauthorized
            .write()
            .unwrap_or_else(|e| e.into_inner()) = handler;
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Body,
        token: Option<&str>,
    ) -> Result<T, ApiError> {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.root))
            .header(ACCEPT, "application/json");
        req = match body {
            Body::Empty => req.header(CONTENT_TYPE, "application/json"),
            Body::Json(value) => req.json(&value),
            // reqwest sets the multipart boundary content type itself.
            Body::Multipart(form) => req.multipart(form),
        };
        if let Some(token) = token {
            req = req.bearer_auth(token).header(STAFF_TOKEN_HEADER, token);
        }

        let response = req.send().await.map_err(ApiError::Network)?;
        let status = response.status();
        let is_json = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("application/json"));
        let bytes = response.bytes().await.map_err(ApiError::Network)?;
        let data: Option<Value> = if is_json {
            serde_json::from_slice(&bytes).ok()
        } else {
            None
        };

        if status == reqwest::StatusCode::UNAUTHORIZED
            && let Some(token) = token
        {
            let handler = self
                .on_unauthorized
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(handler) = handler {
                handler(token);
            }
        }
        if !status.is_success() {
            let message = data
                .as_ref()
                .and_then(|d| d.get("error"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Request failed ({})", status.as_u16()));
            return Err(ApiError::Server { status, message });
        }
        match data {
            Some(value @ (Value::Object(_) | Value::Array(_))) => {
                serde_json::from_value(value).map_err(ApiError::Decode)
            }
            _ => Err(ApiError::InvalidResponse),
        }
    }

    async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        token: Option<&str>,
    ) -> Result<T, ApiError> {
        self.request(Method::GET, path, Body::Empty, token).await
    }

    async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Value,
        token: Option<&str>,
    ) -> Result<T, ApiError> {
        self.request(method, path, Body::Json(body), token).await
    }

    // ------------------------------------------------------------ public

    pub async fn news(&self) -> Result<Items<NewsItem>, ApiError> {
        self.get("/news.php", None).await
    }

    pub async fn streams(&self) -> Result<Items<Streamer>, ApiError> {
        self.get("/streams.php", None).await
    }

    pub async fn gallery(&self) -> Result<Items<GalleryItem>, ApiError> {
        self.get("/gallery.php", None).await
    }

    pub async fn games(&self) -> Result<Items<Game>, ApiError> {
        self.get("/games.php", None).await
    }

    pub async fn servers(&self) -> Result<ServerStatusResponse, ApiError> {
        self.get("/servers.php?v=2", None).await
    }

    pub async fn about(&self) -> Result<About, ApiError> {
        self.get("/about.php", None).await
    }

    pub async fn register_push(
        &self,
        push_token: &str,
        platform: Platform,
    ) -> Result<Success, ApiError> {
        self.send(
            Method::POST,
            "/push/register.php",
            json!({"token": push_token, "platform": platform}),
            None,
        )
        .await
    }

    // ------------------------------------------------------------ auth

    pub async fn login(&self, email: &str, password: &str) -> Result<LoginResponse, ApiError> {
        self.send(
            Method::POST,
            "/auth/login.php",
            json!({"email": email, "password": password}),
            None,
        )
        .await
    }

    pub async fn logout(&self, token: &str) -> Result<Success, ApiError> {
        self.request(Method::POST, "/auth/logout.php", Body::Empty, Some(token))
            .await
    }

    pub async fn change_password(
        &self,
        token: &str,
        current_password: &str,
        new_password: &str,
    ) -> Result<Success, ApiError> {
        let body = json!({"current_password": current_password, "new_password": new_password});
        self.send(Method::POST, "/auth/password.php", body, Some(token))
            .await
    }

    // ------------------------------------------------------------ news

    pub async fn admin_news(&self, token: &str) -> Result<Items<AdminNews>, ApiError> {
        self.get("/admin/news.php", Some(token)).await
    }

    pub async fn create_news(
        &self,
        token: &str,
        title: &str,
        body: &str,
    ) -> Result<Created, ApiError> {
        let body = json!({"title": title, "body": body, "published": true});
        self.send(Method::POST, "/admin/news.php", body, Some(token))
            .await
    }

    pub async fn update_news(
        &self,
        token: &str,
        id: i64,
        title: Option<&str>,
        body: Option<&str>,
        published: Option<bool>,
    ) -> Result<Success, ApiError> {
        let mut patch = json!({"id": id});
        insert_some(&mut patch, "title", title);
        insert_some(&mut patch, "body", body);
        insert_some(&mut patch, "published", published);
        self.send(Method::PATCH, "/admin/news.php", patch, Some(token))
            .await
    }

    pub async fn delete_news(&self, token: &str, id: i64) -> Result<Success, ApiError> {
        self.send(
            Method::DELETE,
            "/admin/news.php",
            json!({"id": id}),
            Some(token),
        )
        .await
    }

    // ------------------------------------------------------------ gallery

    pub async fn admin_gallery(&self, token: &str) -> Result<Items<AdminGallery>, ApiError> {
        self.get("/admin/gallery.php", Some(token)).await
    }

    /// Publishes an image. An empty title becomes "Community upload", as in the Expo app.
    pub async fn upload_gallery(
        &self,
        token: &str,
        title: &str,
        image: ImageUpload,
    ) -> Result<GalleryUploaded, ApiError> {
        let title = if title.trim().is_empty() {
            "Community upload"
        } else {
            title
        };
        let part = multipart::Part::bytes(image.bytes)
            .file_name(image.file_name.unwrap_or_else(|| "upload.jpg".into()))
            .mime_str(image.mime_type.as_deref().unwrap_or("image/jpeg"))
            .map_err(ApiError::Network)?;
        let form = multipart::Form::new()
            .text("title", title.to_owned())
            .part("image", part);
        self.request(
            Method::POST,
            "/admin/gallery-upload.php",
            Body::Multipart(form),
            Some(token),
        )
        .await
    }

    pub async fn update_gallery(
        &self,
        token: &str,
        id: i64,
        title: Option<&str>,
        published: Option<bool>,
    ) -> Result<Success, ApiError> {
        let mut patch = json!({"id": id});
        insert_some(&mut patch, "title", title);
        insert_some(&mut patch, "published", published);
        self.send(Method::PATCH, "/admin/gallery.php", patch, Some(token))
            .await
    }

    pub async fn delete_gallery(&self, token: &str, id: i64) -> Result<Success, ApiError> {
        self.send(
            Method::DELETE,
            "/admin/gallery.php",
            json!({"id": id}),
            Some(token),
        )
        .await
    }

    // ------------------------------------------------------------ staff accounts

    pub async fn staff(&self, token: &str) -> Result<Items<AdminStaff>, ApiError> {
        self.get("/admin/staff.php", Some(token)).await
    }

    pub async fn create_staff(
        &self,
        token: &str,
        name: &str,
        email: &str,
        password: &str,
        role: StaffRole,
    ) -> Result<Created, ApiError> {
        let body = json!({"name": name, "email": email, "password": password, "role": role});
        self.send(Method::POST, "/admin/staff.php", body, Some(token))
            .await
    }

    pub async fn update_staff(
        &self,
        token: &str,
        id: i64,
        role: Option<StaffRole>,
        active: Option<bool>,
        password: Option<&str>,
    ) -> Result<Success, ApiError> {
        let mut patch = json!({"id": id});
        insert_some(&mut patch, "role", role);
        insert_some(&mut patch, "active", active);
        insert_some(&mut patch, "password", password);
        self.send(Method::PATCH, "/admin/staff.php", patch, Some(token))
            .await
    }

    // ------------------------------------------------------------ push

    /// `route` is where the notification opens; see [`crate::routes::Route`].
    pub async fn send_push(
        &self,
        token: &str,
        title: &str,
        body: &str,
        route: &str,
    ) -> Result<PushSent, ApiError> {
        let body = json!({"title": title, "body": body, "route": route});
        self.send(Method::POST, "/admin/push-send.php", body, Some(token))
            .await
    }

    pub async fn push_history(&self, token: &str) -> Result<Items<PushHistory>, ApiError> {
        self.get("/admin/push-receipts.php", Some(token)).await
    }

    pub async fn check_push_receipts(&self, token: &str) -> Result<PushReceiptsChecked, ApiError> {
        self.request(
            Method::POST,
            "/admin/push-receipts.php",
            Body::Empty,
            Some(token),
        )
        .await
    }

    // ------------------------------------------------------------ servers

    pub async fn admin_servers(&self, token: &str) -> Result<AdminServers, ApiError> {
        self.get("/admin/servers.php", Some(token)).await
    }

    pub async fn create_server(
        &self,
        token: &str,
        slug: &str,
        values: &AdminServerInput,
    ) -> Result<ServerCreated, ApiError> {
        let body = json!({"slug": slug, "values": values});
        self.send(Method::POST, "/admin/servers.php", body, Some(token))
            .await
    }

    pub async fn update_server(
        &self,
        token: &str,
        slug: &str,
        values: &AdminServerInput,
    ) -> Result<Success, ApiError> {
        let body = json!({"slug": slug, "values": values});
        self.send(Method::PATCH, "/admin/servers.php", body, Some(token))
            .await
    }

    /// Permanently removes a staff-added server.
    pub async fn delete_server(&self, token: &str, slug: &str) -> Result<ServerDeleted, ApiError> {
        let body = json!({"slug": slug, "delete": true});
        self.send(Method::DELETE, "/admin/servers.php", body, Some(token))
            .await
    }

    /// Discards staff edits to a file-defined server.
    pub async fn reset_server(&self, token: &str, slug: &str) -> Result<Success, ApiError> {
        self.send(
            Method::DELETE,
            "/admin/servers.php",
            json!({"slug": slug}),
            Some(token),
        )
        .await
    }
}

fn insert_some<V: serde::Serialize>(target: &mut Value, key: &str, value: Option<V>) {
    if let (Some(value), Value::Object(map)) = (value, target) {
        map.insert(key.to_owned(), json!(value));
    }
}

/// `https://host/api/v1` → `https://host`.
pub fn site_root(api_root: &str) -> &str {
    let trimmed = api_root.strip_suffix('/').unwrap_or(api_root);
    trimmed.strip_suffix("/api/v1").unwrap_or(api_root)
}

/// Absolute URLs pass through; anything else is a path under `/public/assets/`.
pub fn site_asset(site_root: &str, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_owned()
    } else {
        format!("{site_root}/public/assets/{}", path.trim_start_matches('/'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_root_strips_api_suffix() {
        assert_eq!(
            site_root(DEFAULT_API_ROOT),
            "https://www.frontierwinggaming.network"
        );
        assert_eq!(site_root("https://x.test/api/v1/"), "https://x.test");
        assert_eq!(site_root("https://x.test/other"), "https://x.test/other");
    }

    #[test]
    fn site_asset_resolves_relative_paths() {
        let root = "https://x.test";
        assert_eq!(
            site_asset(root, "/images/a.png"),
            "https://x.test/public/assets/images/a.png"
        );
        assert_eq!(
            site_asset(root, "https://cdn.test/a.png"),
            "https://cdn.test/a.png"
        );
    }
}
