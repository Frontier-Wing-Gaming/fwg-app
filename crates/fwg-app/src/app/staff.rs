//! Staff area: sign-in, quick actions and the manage screens (port of the Expo `admin.tsx`,
//! `manage-*.tsx` and `account-security.tsx`).
//!
//! Every request goes through [`App::staff_request`]: it uses the session's current token, shows
//! the busy state, reports failures as a toast and notices when a 401 ended the session.

use std::{cell::RefCell, future::Future, io::Cursor, rc::Rc, time::Duration};

use fwg_core::{
    ApiClient, ApiError, ImageUpload,
    format::date_label,
    models::{AdminGallery, AdminNews, AdminStaff, PushHistory, StaffRole},
};
use slint::{ComponentHandle, ModelRc, SharedPixelBuffer, SharedString, VecModel};

use super::{App, GALLERY_PX, error_chain};

mod servers;
use crate::{
    AdminGalleryRow, AdminNewsRow, AppState, AppWindow, LoadKind, LoadState, Page, PushRow,
    StaffAccountRow, StaffState, platform,
};

/// The website's gallery limit is 10 MB; photos above this edge length are scaled down.
const UPLOAD_MAX_BYTES: usize = 10 * 1024 * 1024;
const UPLOAD_MAX_EDGE: u32 = 2560;
const PREVIEW_PX: u32 = 640;
const TOAST_TIME: Duration = Duration::from_secs(4);

type ConfirmAction = Box<dyn FnOnce(&Rc<App>)>;

#[derive(Default)]
pub(super) struct StaffData {
    news: RefCell<List<AdminNews>>,
    gallery: RefCell<List<AdminGallery>>,
    staff: RefCell<List<AdminStaff>>,
    push: RefCell<List<PushHistory>>,
    picked: RefCell<Option<ImageUpload>>,
    confirm: RefCell<Option<ConfirmAction>>,
    notice: RefCell<String>,
    toast: slint::Timer,
    servers: servers::ServersData,
}

struct List<T> {
    items: Vec<T>,
    error: Option<String>,
    loading: bool,
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            error: None,
            loading: false,
        }
    }
}

impl<T> List<T> {
    fn load_state(&self) -> LoadState {
        let (kind, message) = match (&self.error, self.items.is_empty()) {
            (Some(e), true) => (LoadKind::Failed, e.clone()),
            _ if self.loading && self.items.is_empty() => (LoadKind::Loading, String::new()),
            _ => (LoadKind::Ready, String::new()),
        };
        LoadState {
            kind,
            message: message.into(),
        }
    }
}

/// Which manage list a request refreshes.
#[derive(Clone, Copy)]
enum ListKey {
    News,
    Gallery,
    Staff,
    Push,
}

/// The picked image after checking and shrinking it, ready for upload.
struct Prepared {
    upload: ImageUpload,
    preview: SharedPixelBuffer<slint::Rgba8Pixel>,
    info: String,
}

impl App {
    pub(super) fn bind_staff(self: &Rc<Self>, ui: &AppWindow) {
        let staff = ui.global::<StaffState>();
        let weak = Rc::downgrade(self);
        let with = move |f: fn(&Rc<App>)| {
            let weak = weak.clone();
            move || {
                if let Some(app) = weak.upgrade() {
                    f(&app);
                }
            }
        };
        let weak = Rc::downgrade(self);
        let with_id = move |f: fn(&Rc<App>, i64)| {
            let weak = weak.clone();
            move |id: i32| {
                if let Some(app) = weak.upgrade() {
                    f(&app, i64::from(id));
                }
            }
        };

        staff.on_sign_in(with(|app| app.sign_in()));
        staff.on_sign_out(with(|app| app.sign_out()));
        staff.on_dismiss_notice(with(|app| {
            app.staff.notice.borrow_mut().clear();
            app.render_staff();
        }));
        staff.on_publish_news(with(|app| app.publish_news()));
        staff.on_send_push(with(|app| app.send_push()));
        staff.on_pick_image(with(|_| platform::pick_image()));
        staff.on_upload_image(with(|app| app.upload_image()));
        staff.on_add_staff(with(|app| app.add_staff()));
        staff.on_change_password(with(|app| app.change_password()));
        staff.on_check_receipts(with(|app| app.check_receipts()));
        staff.on_save_news(with(|app| app.save_news()));
        staff.on_save_staff(with(|app| app.save_staff()));
        staff.on_edit_news(with_id(|app, id| app.edit_news(id)));
        staff.on_toggle_news(with_id(|app, id| app.toggle_news(id)));
        staff.on_delete_news(with_id(|app, id| app.delete_news(id)));
        staff.on_toggle_gallery(with_id(|app, id| app.toggle_gallery(id)));
        staff.on_delete_gallery(with_id(|app, id| app.delete_gallery(id)));
        staff.on_edit_staff(with_id(|app, id| app.edit_staff(id)));
        staff.on_toggle_staff(with_id(|app, id| app.toggle_staff(id)));
        let weak = Rc::downgrade(self);
        staff.on_reload(move |key| {
            if let (Some(app), Some(key)) = (weak.upgrade(), list_key(&key)) {
                app.load_list(key);
            }
        });

        let state = ui.global::<AppState>();
        state.on_confirm_accept(with(|app| {
            app.ui(|s| s.set_confirm_show(false));
            if let Some(action) = app.staff.confirm.borrow_mut().take() {
                action(app);
            }
        }));
        self.bind_servers(ui);
        state.on_confirm_cancel(with(|app| {
            app.cancel_confirm();
        }));
    }

    // ------------------------------------------------------------ plumbing

    fn staff_ui(&self, f: impl FnOnce(StaffState<'_>)) {
        if let Some(ui) = self.ui.upgrade() {
            f(ui.global::<StaffState>());
        }
    }

    /// Shows a short message above the tab bar.
    pub(super) fn toast(self: &Rc<Self>, message: &str) {
        self.ui(|s| s.set_toast(message.into()));
        let weak = Rc::downgrade(self);
        self.staff
            .toast
            .start(slint::TimerMode::SingleShot, TOAST_TIME, move || {
                if let Some(app) = weak.upgrade() {
                    app.ui(|s| s.set_toast(SharedString::new()));
                }
            });
    }

    fn confirm(
        self: &Rc<Self>,
        title: &str,
        text: &str,
        action_label: &str,
        destructive: bool,
        action: impl FnOnce(&Rc<App>) + 'static,
    ) {
        *self.staff.confirm.borrow_mut() = Some(Box::new(action));
        self.ui(|s| {
            s.set_confirm_title(title.into());
            s.set_confirm_text(text.into());
            s.set_confirm_action(action_label.into());
            s.set_confirm_destructive(destructive);
            s.set_confirm_show(true);
        });
    }

    /// Closes an open confirmation; returns whether one was open (back closes it first).
    pub(super) fn cancel_confirm(self: &Rc<Self>) -> bool {
        let open = self.staff.confirm.borrow_mut().take().is_some();
        self.ui(|s| s.set_confirm_show(false));
        open
    }

    /// Runs a staff request with the current token. `done` runs on the UI thread on success.
    fn staff_request<T, Fut>(
        self: &Rc<Self>,
        request: impl FnOnce(ApiClient, String) -> Fut,
        done: impl FnOnce(&Rc<App>, T) + 'static,
    ) where
        T: Send + 'static,
        Fut: Future<Output = Result<T, ApiError>> + Send + 'static,
    {
        let Some(token) = self.session.token() else {
            self.after_staff_request();
            return;
        };
        self.staff_ui(|s| s.set_busy(true));
        let task = self.rt.spawn(request(self.api.clone(), token));
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            let result = task.await;
            app.staff_ui(|s| s.set_busy(false));
            match result {
                Ok(Ok(value)) => done(&app, value),
                Ok(Err(e)) => {
                    platform::log(&format!("staff: {}", error_chain(&e)));
                    app.toast(&e.to_string());
                }
                Err(_) => app.toast("The request failed."),
            }
            app.after_staff_request();
        });
    }

    /// After any staff request: if a 401 ended the session, drop staff data and go to sign-in.
    fn after_staff_request(self: &Rc<Self>) {
        if self.session.is_signed_in() {
            return;
        }
        if let Some(notice) = self.session.take_notice() {
            *self.staff.notice.borrow_mut() = notice;
        }
        self.clear_staff_data();
        if is_staff_page(self.page()) {
            {
                let mut stack = self.stack.borrow_mut();
                stack.retain(|p| !is_staff_page(*p));
                stack.push(Page::Staff);
            }
            self.show(Page::Staff);
        }
        self.render_staff();
    }

    fn clear_staff_data(&self) {
        *self.staff.news.borrow_mut() = List::default();
        *self.staff.gallery.borrow_mut() = List::default();
        *self.staff.staff.borrow_mut() = List::default();
        *self.staff.push.borrow_mut() = List::default();
        *self.staff.picked.borrow_mut() = None;
        self.clear_servers();
        self.staff_ui(|s| {
            s.set_news(ModelRc::default());
            s.set_gallery(ModelRc::default());
            s.set_staff(ModelRc::default());
            s.set_push(ModelRc::default());
            s.set_has_image(false);
            s.set_image_preview(slint::Image::default());
            s.set_edit_news_id(-1);
            s.set_edit_staff_id(-1);
            for set in [
                StaffState::set_news_title,
                StaffState::set_news_body,
                StaffState::set_push_title,
                StaffState::set_push_body,
                StaffState::set_gallery_caption,
                StaffState::set_staff_name,
                StaffState::set_staff_email,
                StaffState::set_staff_password,
                StaffState::set_current_password,
                StaffState::set_new_password,
                StaffState::set_confirm_password,
                StaffState::set_edit_staff_password,
            ] {
                set(&s, SharedString::new());
            }
        });
    }

    /// Called by `show` for staff pages.
    pub(super) fn show_staff_page(self: &Rc<Self>, page: Page) {
        self.render_staff();
        if !self.session.is_signed_in() {
            if page != Page::Staff {
                // Deep pages need a session; fall back to the sign-in screen.
                self.after_staff_request();
            }
            return;
        }
        match page {
            Page::ManageNews => self.load_list(ListKey::News),
            Page::ManageGallery => self.load_list(ListKey::Gallery),
            Page::ManageStaff => self.load_list(ListKey::Staff),
            Page::PushHistory => self.load_list(ListKey::Push),
            Page::ManageServers => self.load_servers(),
            _ => {}
        }
    }

    pub(super) fn render_staff(&self) {
        let user = self.session.user();
        let role = user.as_ref().map(|u| u.role.as_str()).unwrap_or("");
        let notice = self.staff.notice.borrow().clone();
        self.staff_ui(|s| {
            s.set_signed_in(user.is_some());
            s.set_name(user.as_ref().map(|u| u.name.as_str()).unwrap_or("").into());
            s.set_is_owner(role == "owner");
            s.set_is_admin(role == "owner" || role == "admin");
            s.set_session_notice(notice.into());
        });
    }

    // ------------------------------------------------------------ session

    fn sign_in(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let form = ui.global::<StaffState>();
        let email = form.get_email().trim().to_owned();
        let password = form.get_password().to_string();
        if email.is_empty() || password.is_empty() {
            return;
        }
        form.set_busy(true);
        let session = self.session.clone();
        let task = self
            .rt
            .spawn(async move { session.sign_in(&email, &password).await });
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            let result = task.await;
            app.staff_ui(|s| s.set_busy(false));
            match result {
                Ok(Ok(_)) => {
                    app.staff.notice.borrow_mut().clear();
                    app.staff_ui(|s| s.set_password(SharedString::new()));
                    app.render_staff();
                }
                Ok(Err(e)) => app.toast(&e.to_string()),
                Err(_) => app.toast("Sign in failed."),
            }
        });
    }

    fn sign_out(self: &Rc<Self>) {
        let session = self.session.clone();
        let task = self.rt.spawn(async move { session.sign_out().await });
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            let _ = task.await;
            app.clear_staff_data();
            app.render_staff();
            app.toast("Signed out.");
        });
    }

    // ------------------------------------------------------------ quick actions

    fn publish_news(self: &Rc<Self>) {
        let Some((title, body)) = self.form(|s| (s.get_news_title(), s.get_news_body())) else {
            return;
        };
        let (title, body) = (title.trim().to_owned(), body.trim().to_owned());
        if title.is_empty() || body.is_empty() {
            return;
        }
        self.staff_request(
            move |api, token| async move { api.create_news(&token, &title, &body).await },
            |app, _| {
                app.staff_ui(|s| {
                    s.set_news_title(SharedString::new());
                    s.set_news_body(SharedString::new());
                });
                app.toast("Announcement published.");
            },
        );
    }

    fn send_push(self: &Rc<Self>) {
        let Some((title, body, route)) =
            self.form(|s| (s.get_push_title(), s.get_push_body(), s.get_push_route()))
        else {
            return;
        };
        let (title, body) = (title.trim().to_owned(), body.trim().to_owned());
        if title.is_empty() || body.is_empty() {
            return;
        }
        // Not in the Expo app: a send reaches every registered device, so ask first.
        self.confirm(
            "Send to everyone?",
            &format!("“{title}” goes to every device with notifications enabled."),
            "Send",
            false,
            move |app| {
                let route = route.to_string();
                app.staff_request(
                    move |api, token| async move { api.send_push(&token, &title, &body, &route).await },
                    |app, sent| {
                        app.staff_ui(|s| {
                            s.set_push_title(SharedString::new());
                            s.set_push_body(SharedString::new());
                        });
                        let plural = if sent.sent == 1 { "" } else { "s" };
                        app.toast(&format!("Notification sent to {} device{plural}.", sent.sent));
                    },
                );
            },
        );
    }

    /// The photo picker's answer (`OsEvent::ImagePicked`).
    pub(super) fn image_picked(
        self: &Rc<Self>,
        bytes: Option<Vec<u8>>,
        name: Option<String>,
        mime: Option<String>,
        error: Option<String>,
    ) {
        let Some(bytes) = bytes else {
            if let Some(error) = error {
                self.toast(&error);
            }
            return;
        };
        self.staff_ui(|s| s.set_busy(true));
        let task = self
            .rt
            .spawn_blocking(move || prepare_upload(bytes, name, mime));
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            let result = task.await;
            app.staff_ui(|s| s.set_busy(false));
            match result {
                Ok(Ok(prepared)) => {
                    let preview = slint::Image::from_rgba8(prepared.preview);
                    *app.staff.picked.borrow_mut() = Some(prepared.upload);
                    app.staff_ui(|s| {
                        s.set_has_image(true);
                        s.set_image_preview(preview);
                        s.set_image_info(prepared.info.into());
                    });
                }
                Ok(Err(message)) => app.toast(&message),
                Err(_) => app.toast("The image could not be read."),
            }
        });
    }

    fn upload_image(self: &Rc<Self>) {
        let Some(upload) = self.staff.picked.borrow().clone() else {
            return;
        };
        let caption = self.form(|s| s.get_gallery_caption()).unwrap_or_default();
        let title = match caption.trim() {
            "" => "Community upload".to_owned(),
            t => t.to_owned(),
        };
        self.staff_request(
            move |api, token| async move { api.upload_gallery(&token, &title, upload).await },
            |app, _| {
                *app.staff.picked.borrow_mut() = None;
                app.staff_ui(|s| {
                    s.set_has_image(false);
                    s.set_image_preview(slint::Image::default());
                    s.set_gallery_caption(SharedString::new());
                });
                app.toast("Gallery image published.");
            },
        );
    }

    fn add_staff(self: &Rc<Self>) {
        let Some((name, email, password, role)) = self.form(|s| {
            (
                s.get_staff_name(),
                s.get_staff_email(),
                s.get_staff_password(),
                s.get_staff_role(),
            )
        }) else {
            return;
        };
        let role = if role == "admin" {
            StaffRole::Admin
        } else {
            StaffRole::Editor
        };
        let (name, email, password) = (
            name.trim().to_owned(),
            email.trim().to_owned(),
            password.to_string(),
        );
        self.staff_request(
            move |api, token| async move {
                api.create_staff(&token, &name, &email, &password, role)
                    .await
            },
            |app, _| {
                app.staff_ui(|s| {
                    s.set_staff_name(SharedString::new());
                    s.set_staff_email(SharedString::new());
                    s.set_staff_password(SharedString::new());
                });
                app.toast("Staff account created.");
            },
        );
    }

    fn change_password(self: &Rc<Self>) {
        let Some((current, new, confirm)) = self.form(|s| {
            (
                s.get_current_password(),
                s.get_new_password(),
                s.get_confirm_password(),
            )
        }) else {
            return;
        };
        if new != confirm {
            self.toast("The new passwords do not match.");
            return;
        }
        let (current, new) = (current.to_string(), new.to_string());
        self.staff_request(
            move |api, token| async move { api.change_password(&token, &current, &new).await },
            |app, _| {
                app.staff_ui(|s| {
                    s.set_current_password(SharedString::new());
                    s.set_new_password(SharedString::new());
                    s.set_confirm_password(SharedString::new());
                });
                app.toast("Password changed. Other sessions were signed out.");
            },
        );
    }

    fn form<R>(&self, f: impl FnOnce(&StaffState<'_>) -> R) -> Option<R> {
        let ui = self.ui.upgrade()?;
        let state = ui.global::<StaffState>();
        Some(f(&state))
    }

    // ------------------------------------------------------------ manage lists

    fn load_list(self: &Rc<Self>, key: ListKey) {
        macro_rules! fetch {
            ($field:ident, $call:ident) => {{
                {
                    let mut list = self.staff.$field.borrow_mut();
                    if list.loading {
                        return;
                    }
                    list.loading = true;
                }
                self.render_list(key);
                let Some(token) = self.session.token() else {
                    return self.after_staff_request();
                };
                let api = self.api.clone();
                let task = self.rt.spawn(async move { api.$call(&token).await });
                let app = self.clone();
                let _ = slint::spawn_local(async move {
                    let result = task.await;
                    {
                        let mut list = app.staff.$field.borrow_mut();
                        list.loading = false;
                        match result {
                            Ok(Ok(page)) => {
                                list.items = page.items;
                                list.error = None;
                            }
                            Ok(Err(e)) => {
                                platform::log(&format!("staff list: {}", error_chain(&e)));
                                list.error = Some(e.to_string());
                            }
                            Err(_) => list.error = Some("The request failed.".into()),
                        }
                    }
                    app.render_list(key);
                    app.after_staff_request();
                });
            }};
        }
        match key {
            ListKey::News => fetch!(news, admin_news),
            ListKey::Gallery => fetch!(gallery, admin_gallery),
            ListKey::Staff => fetch!(staff, staff),
            ListKey::Push => fetch!(push, push_history),
        }
    }

    fn render_list(self: &Rc<Self>, key: ListKey) {
        match key {
            ListKey::News => {
                let list = self.staff.news.borrow();
                let rows: Vec<AdminNewsRow> = list
                    .items
                    .iter()
                    .map(|n| AdminNewsRow {
                        id: n.id as i32,
                        status: if n.published { "PUBLISHED" } else { "HIDDEN" }.into(),
                        title: n.title.as_str().into(),
                        preview: preview(&n.body, 220).into(),
                        published: n.published,
                    })
                    .collect();
                let state = list.load_state();
                self.staff_ui(|s| {
                    s.set_news_state(state);
                    s.set_news(ModelRc::new(VecModel::from(rows)));
                });
            }
            ListKey::Gallery => self.render_admin_gallery(),
            ListKey::Staff => {
                let list = self.staff.staff.borrow();
                let rows: Vec<StaffAccountRow> = list
                    .items
                    .iter()
                    .map(|m| StaffAccountRow {
                        id: m.id as i32,
                        label: format!(
                            "{} · {}",
                            m.role.as_str().to_uppercase(),
                            if m.active { "ACTIVE" } else { "DISABLED" }
                        )
                        .into(),
                        name: m.name.as_str().into(),
                        email: m.email.as_str().into(),
                        active: m.active,
                    })
                    .collect();
                let state = list.load_state();
                self.staff_ui(|s| {
                    s.set_staff_state(state);
                    s.set_staff(ModelRc::new(VecModel::from(rows)));
                });
            }
            ListKey::Push => {
                let list = self.staff.push.borrow();
                let rows: Vec<PushRow> = list
                    .items
                    .iter()
                    .map(|p| PushRow {
                        label: format!(
                            "{} · {} RECIPIENT{}",
                            p.status.to_uppercase(),
                            p.recipient_count,
                            if p.recipient_count == 1 { "" } else { "S" }
                        )
                        .into(),
                        title: p.title.as_str().into(),
                        body: p.body.as_str().into(),
                        meta: format!(
                            "{} · {}",
                            utc_label(&p.created_at),
                            p.sender.as_deref().unwrap_or("Former staff")
                        )
                        .into(),
                    })
                    .collect();
                let state = list.load_state();
                self.staff_ui(|s| {
                    s.set_push_state(state);
                    s.set_push(ModelRc::new(VecModel::from(rows)));
                });
            }
        }
    }

    fn render_admin_gallery(self: &Rc<Self>) {
        let list = self.staff.gallery.borrow();
        let rows: Vec<AdminGalleryRow> = list
            .items
            .iter()
            .map(|g| {
                let picture = self.images.get(
                    &g.image_url,
                    GALLERY_PX,
                    self.rerender(App::render_admin_gallery),
                );
                AdminGalleryRow {
                    id: g.id as i32,
                    status: if g.published { "PUBLISHED" } else { "HIDDEN" }.into(),
                    title: g.title.as_str().into(),
                    published: g.published,
                    has_picture: picture.is_some(),
                    picture: picture.unwrap_or_default(),
                }
            })
            .collect();
        let state = list.load_state();
        drop(list);
        self.staff_ui(|s| {
            s.set_gallery_state(state);
            s.set_gallery(ModelRc::new(VecModel::from(rows)));
        });
    }

    fn edit_news(self: &Rc<Self>, id: i64) {
        let list = self.staff.news.borrow();
        let Some(item) = list.items.iter().find(|n| n.id == id) else {
            return;
        };
        let (title, body) = (item.title.clone(), item.body.clone());
        drop(list);
        self.staff_ui(|s| {
            s.set_edit_news_title(title.into());
            s.set_edit_news_body(body.into());
            s.set_edit_news_id(id as i32);
        });
    }

    fn save_news(self: &Rc<Self>) {
        let Some((id, title, body)) = self.form(|s| {
            (
                s.get_edit_news_id(),
                s.get_edit_news_title(),
                s.get_edit_news_body(),
            )
        }) else {
            return;
        };
        let (title, body) = (title.trim().to_owned(), body.trim().to_owned());
        self.staff_request(
            move |api, token| async move {
                api.update_news(&token, i64::from(id), Some(&title), Some(&body), None)
                    .await
            },
            |app, _| {
                app.staff_ui(|s| s.set_edit_news_id(-1));
                app.toast("Announcement updated.");
                app.load_list(ListKey::News);
            },
        );
    }

    fn toggle_news(self: &Rc<Self>, id: i64) {
        let Some((title, published)) = self
            .staff
            .news
            .borrow()
            .items
            .iter()
            .find(|n| n.id == id)
            .map(|n| (n.title.clone(), n.published))
        else {
            return;
        };
        let (question, action) = if published {
            ("Unpublish announcement?", "Unpublish")
        } else {
            ("Publish announcement?", "Publish")
        };
        self.confirm(question, &title, action, false, move |app| {
            app.staff_request(
                move |api, token| async move {
                    api.update_news(&token, id, None, None, Some(!published))
                        .await
                },
                |app, _| app.load_list(ListKey::News),
            );
        });
    }

    fn delete_news(self: &Rc<Self>, id: i64) {
        let Some(title) = self
            .staff
            .news
            .borrow()
            .items
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.title.clone())
        else {
            return;
        };
        self.confirm(
            "Delete announcement?",
            &format!("“{title}” will be permanently removed."),
            "Delete",
            true,
            move |app| {
                app.staff_request(
                    move |api, token| async move { api.delete_news(&token, id).await },
                    |app, _| {
                        app.toast("Announcement deleted.");
                        app.load_list(ListKey::News);
                    },
                );
            },
        );
    }

    fn toggle_gallery(self: &Rc<Self>, id: i64) {
        let Some(published) = self
            .staff
            .gallery
            .borrow()
            .items
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.published)
        else {
            return;
        };
        self.staff_request(
            move |api, token| async move {
                api.update_gallery(&token, id, None, Some(!published)).await
            },
            |app, _| app.load_list(ListKey::Gallery),
        );
    }

    fn delete_gallery(self: &Rc<Self>, id: i64) {
        let Some(title) = self
            .staff
            .gallery
            .borrow()
            .items
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.title.clone())
        else {
            return;
        };
        self.confirm(
            "Delete gallery image?",
            &format!("“{title}” and its uploaded file will be permanently removed."),
            "Delete",
            true,
            move |app| {
                app.staff_request(
                    move |api, token| async move { api.delete_gallery(&token, id).await },
                    |app, _| {
                        app.toast("Gallery image deleted.");
                        app.load_list(ListKey::Gallery);
                    },
                );
            },
        );
    }

    fn edit_staff(self: &Rc<Self>, id: i64) {
        let Some((name, role)) = self
            .staff
            .staff
            .borrow()
            .items
            .iter()
            .find(|m| m.id == id)
            .map(|m| (m.name.clone(), m.role.as_str()))
        else {
            return;
        };
        self.staff_ui(|s| {
            s.set_edit_staff_name(name.into());
            s.set_edit_staff_role(role.into());
            s.set_edit_staff_password(SharedString::new());
            s.set_edit_staff_id(id as i32);
        });
    }

    fn save_staff(self: &Rc<Self>) {
        let Some((id, role, password)) = self.form(|s| {
            (
                s.get_edit_staff_id(),
                s.get_edit_staff_role(),
                s.get_edit_staff_password(),
            )
        }) else {
            return;
        };
        let role = match role.as_str() {
            "owner" => StaffRole::Owner,
            "admin" => StaffRole::Admin,
            _ => StaffRole::Editor,
        };
        let password = (!password.is_empty()).then(|| password.to_string());
        self.staff_request(
            move |api, token| async move {
                api.update_staff(&token, i64::from(id), Some(role), None, password.as_deref())
                    .await
            },
            |app, _| {
                app.staff_ui(|s| {
                    s.set_edit_staff_id(-1);
                    s.set_edit_staff_password(SharedString::new());
                });
                app.toast("Staff account updated.");
                app.load_list(ListKey::Staff);
            },
        );
    }

    fn toggle_staff(self: &Rc<Self>, id: i64) {
        let Some((name, active)) = self
            .staff
            .staff
            .borrow()
            .items
            .iter()
            .find(|m| m.id == id)
            .map(|m| (m.name.clone(), m.active))
        else {
            return;
        };
        let (title, text, action) = if active {
            (
                "Deactivate staff account?",
                format!("{name}'s active sessions will no longer authorize staff actions."),
                "Deactivate",
            )
        } else {
            (
                "Reactivate staff account?",
                format!("This restores {name}'s access using their current password."),
                "Reactivate",
            )
        };
        self.confirm(title, &text, action, active, move |app| {
            app.staff_request(
                move |api, token| async move {
                    api.update_staff(&token, id, None, Some(!active), None)
                        .await
                },
                |app, _| app.load_list(ListKey::Staff),
            );
        });
    }

    fn check_receipts(self: &Rc<Self>) {
        self.staff_request(
            |api, token| async move { api.check_push_receipts(&token).await },
            |app, result| {
                app.toast(&format!(
                    "Checked {} receipts; disabled {} invalid devices.",
                    result.checked, result.disabled
                ));
                app.load_list(ListKey::Push);
            },
        );
    }
}

pub(super) fn is_staff_page(page: Page) -> bool {
    matches!(
        page,
        Page::Staff
            | Page::ManageNews
            | Page::ManageGallery
            | Page::ManageStaff
            | Page::PushHistory
            | Page::Account
            | Page::ManageServers
            | Page::EditServer
    )
}

fn list_key(key: &str) -> Option<ListKey> {
    Some(match key {
        "news" => ListKey::News,
        "gallery" => ListKey::Gallery,
        "staff" => ListKey::Staff,
        "push" => ListKey::Push,
        _ => return None,
    })
}

/// First `max` characters of a news body on one paragraph, for the manage list.
fn preview(body: &str, max: usize) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", flat[..cut].trim_end()),
        None => flat,
    }
}

/// "2026-11-06 21:40:00" (UTC, from SQLite) → "Nov 6, 2026 · 21:40 UTC".
fn utc_label(value: &str) -> String {
    match value.get(11..16) {
        Some(time) => format!("{} · {time} UTC", date_label(value)),
        None => date_label(value),
    }
}

/// Checks the picked file is an image the website accepts and keeps it under the size limit.
/// JPEG/WebP photos are always re-encoded as JPEG, which applies the camera rotation and drops
/// EXIF data such as GPS location. PNGs are kept unless too large; GIFs are kept as-is.
fn prepare_upload(
    bytes: Vec<u8>,
    name: Option<String>,
    mime: Option<String>,
) -> Result<Prepared, String> {
    use image::{ImageDecoder, ImageFormat, ImageReader, imageops::FilterType};

    let format = image::guess_format(&bytes).map_err(|_| {
        "That file is not an image the website accepts (JPG, PNG, WebP or GIF).".to_owned()
    })?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP | ImageFormat::Gif
    ) {
        return Err("Use a JPG, PNG, WebP, or GIF image.".into());
    }
    let mut decoder = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?
        .into_decoder()
        .map_err(|_| "The image could not be read.".to_owned())?;
    let orientation = decoder.orientation().ok();
    let mut img = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| "The image could not be read.".to_owned())?;
    if let Some(orientation) = orientation {
        img.apply_orientation(orientation);
    }

    let stem = name
        .as_deref()
        .and_then(|n| n.rsplit_once('.').map(|(s, _)| s).or(Some(n)))
        .filter(|s| !s.is_empty())
        .unwrap_or("upload")
        .to_owned();
    let too_big = img.width().max(img.height()) > UPLOAD_MAX_EDGE;
    let original_kb = bytes.len() / 1024;

    let upload = match format {
        ImageFormat::Gif => {
            if bytes.len() > UPLOAD_MAX_BYTES {
                return Err("GIFs must be 10 MB or smaller.".into());
            }
            ImageUpload {
                bytes,
                file_name: Some(format!("{stem}.gif")),
                mime_type: Some(mime.unwrap_or_else(|| "image/gif".into())),
            }
        }
        ImageFormat::Png if !too_big && bytes.len() <= UPLOAD_MAX_BYTES => ImageUpload {
            bytes,
            file_name: Some(format!("{stem}.png")),
            mime_type: Some("image/png".into()),
        },
        _ => {
            let img = if too_big {
                img.resize(UPLOAD_MAX_EDGE, UPLOAD_MAX_EDGE, FilterType::Lanczos3)
            } else {
                img.clone()
            };
            let mut out = Vec::new();
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 88);
            img.to_rgb8()
                .write_with_encoder(encoder)
                .map_err(|e| e.to_string())?;
            if out.len() > UPLOAD_MAX_BYTES {
                return Err("The image is still over 10 MB after shrinking it.".into());
            }
            ImageUpload {
                bytes: out,
                file_name: Some(format!("{stem}.jpg")),
                mime_type: Some("image/jpeg".into()),
            }
        }
    };

    let thumb = img.thumbnail(PREVIEW_PX, PREVIEW_PX).to_rgba8();
    let preview =
        SharedPixelBuffer::clone_from_slice(thumb.as_raw(), thumb.width(), thumb.height());
    let info = format!(
        "{} · {} × {} · {} KB → {} KB",
        upload.file_name.as_deref().unwrap_or("upload"),
        img.width()
            .min(if too_big { UPLOAD_MAX_EDGE } else { u32::MAX }),
        img.height()
            .min(if too_big { UPLOAD_MAX_EDGE } else { u32::MAX }),
        original_kb,
        upload.bytes.len() / 1024
    );
    Ok(Prepared {
        upload,
        preview,
        info,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_cuts_on_characters() {
        assert_eq!(preview("short\n\nbody", 50), "short body");
        assert_eq!(preview("ééééé", 3), "ééé…");
    }

    #[test]
    fn utc_label_formats_sqlite_time() {
        assert!(utc_label("2026-11-06 21:40:00").ends_with("· 21:40 UTC"));
    }

    #[test]
    fn rejects_non_images() {
        assert!(prepare_upload(b"not an image".to_vec(), None, None).is_err());
    }

    #[test]
    fn large_png_is_shrunk_to_jpeg() {
        let img = image::RgbImage::from_pixel(3000, 1000, image::Rgb([200, 50, 20]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let prepared = prepare_upload(png, Some("shot.png".into()), None).unwrap();
        assert_eq!(prepared.upload.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(prepared.upload.file_name.as_deref(), Some("shot.jpg"));
        let out = image::load_from_memory(&prepared.upload.bytes).unwrap();
        assert_eq!((out.width(), out.height()), (2560, 853));
    }
}
