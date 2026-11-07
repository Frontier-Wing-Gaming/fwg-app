//! App controller: owns the data, runs network work on tokio, and pushes view models into the UI.
//!
//! Everything here runs on the UI thread. Network calls are spawned on the tokio runtime and
//! awaited through `slint::spawn_local`, so results come back on the UI thread without locks.

use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    sync::{Mutex, PoisonError},
};

use fwg_core::{
    ApiClient, ApiError, Cache, Resource, Session,
    models::{
        About, GalleryItem, Game, Items, NewsItem, Platform as PushPlatform, ServerStatusResponse,
        Streamer,
    },
    routes::Route,
    server_status::POLL_INTERVAL,
};
use serde::{Serialize, de::DeserializeOwned};
use slint::{ComponentHandle, ModelRc, VecModel, Weak};
use tokio::runtime::Handle;

mod staff;

use crate::{
    AppState, AppWindow, GalleryTile, LoadKind, LoadState, Page, ServerView, StaffCard,
    images::ImageLoader, platform, view,
};

const DEFAULT_INTRO: &str = "A gaming and homelab community built by the people who play on it.";
const GALLERY_PX: u32 = 512;
const AVATAR_PX: u32 = 160;
const BANNER_PX: u32 = 960;

type Slot<T> = RefCell<Option<Resource<T>>>;

thread_local! {
    /// The running app, for OS events that arrive outside the UI (see `post_os_event`).
    static CURRENT: RefCell<std::rc::Weak<App>> = const { RefCell::new(std::rc::Weak::new()) };
}

/// Something the OS reports from another thread (Android JNI callbacks from `MainActivity`).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub enum OsEvent {
    /// Back button/gesture on Android 13+.
    Back,
    /// A tapped notification's `data.route`; only allow-listed screens are opened.
    Route(String),
    /// Result of fetching the FCM token. `silent` = the automatic re-registration at launch.
    PushToken {
        token: Option<String>,
        error: Option<String>,
        silent: bool,
    },
    /// The photo picker's result for a gallery upload (desktop file dialog or Android picker).
    ImagePicked {
        bytes: Option<Vec<u8>>,
        name: Option<String>,
        mime: Option<String>,
        error: Option<String>,
    },
}

static OS_EVENTS: Mutex<Vec<OsEvent>> = Mutex::new(Vec::new());

/// Queues an OS event from any thread. It is handled on the UI thread, or by `App::start` when
/// it arrives before the app is running (e.g. a notification tap that launched the app).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn post_os_event(event: OsEvent) {
    OS_EVENTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(event);
    let _ = slint::invoke_from_event_loop(drain_os_events);
}

fn drain_os_events() {
    let Some(app) = CURRENT.with(|current| current.borrow().upgrade()) else {
        return;
    };
    let events = std::mem::take(&mut *OS_EVENTS.lock().unwrap_or_else(PoisonError::into_inner));
    for event in events {
        app.handle_os_event(event);
    }
}

pub struct App {
    ui: Weak<AppWindow>,
    rt: Handle,
    api: ApiClient,
    session: Session,
    cache: Cache,
    images: Rc<ImageLoader>,
    staff: staff::StaffData,

    news: Slot<Items<NewsItem>>,
    streams: Slot<Items<Streamer>>,
    gallery: Slot<Items<GalleryItem>>,
    games: Slot<Items<Game>>,
    about: Slot<About>,

    servers: RefCell<Option<ServerStatusResponse>>,
    servers_loading: Cell<bool>,
    poll: slint::Timer,

    stack: RefCell<Vec<Page>>,
    server_slug: RefCell<String>,
    server_notice: RefCell<String>,
}

impl App {
    pub fn new(
        ui: &AppWindow,
        rt: Handle,
        api: ApiClient,
        session: Session,
        cache: Cache,
        images: Rc<ImageLoader>,
    ) -> Rc<Self> {
        let app = Rc::new(Self {
            ui: ui.as_weak(),
            rt,
            api,
            session,
            cache,
            images,
            staff: staff::StaffData::default(),
            news: Slot::default(),
            streams: Slot::default(),
            gallery: Slot::default(),
            games: Slot::default(),
            about: Slot::default(),
            servers: RefCell::default(),
            servers_loading: Cell::new(false),
            poll: slint::Timer::default(),
            stack: RefCell::new(vec![Page::Home]),
            server_slug: RefCell::default(),
            server_notice: RefCell::default(),
        });
        app.bind(ui);
        app.bind_staff(ui);
        app
    }

    fn bind(self: &Rc<Self>, ui: &AppWindow) {
        let state = ui.global::<AppState>();
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

        state.on_navigate({
            let weak = weak.clone();
            move |page| {
                if let Some(app) = weak.upgrade() {
                    app.navigate(page);
                }
            }
        });
        state.on_back(with(|app| app.back()));
        state.on_open_server({
            let weak = weak.clone();
            move |slug| {
                if let Some(app) = weak.upgrade() {
                    app.open_server(&slug);
                }
            }
        });
        state.on_open_url(|url| open_url(&url));
        state.on_retry({
            let weak = weak.clone();
            move |key| {
                if let Some(app) = weak.upgrade() {
                    app.load(&key, true);
                }
            }
        });
        state.on_launch_server(with(|app| app.launch_server()));
        state.on_share_address(with(|app| app.share_address()));
        state.on_enable_notifications(with(|app| {
            // On Android the outcome arrives later as OsEvent::PushToken.
            let notice = platform::enable_notifications().unwrap_or_default();
            app.ui(|s| s.set_more_notice(notice.into()))
        }));
    }

    /// First paint plus the server-status poll.
    pub fn start(self: &Rc<Self>) {
        CURRENT.with(|current| *current.borrow_mut() = Rc::downgrade(self));
        self.render_streams();
        self.render_about();
        self.show(Page::Home);
        drain_os_events();

        let weak = Rc::downgrade(self);
        self.poll
            .start(slint::TimerMode::Repeated, POLL_INTERVAL, move || {
                if let Some(app) = weak.upgrade() {
                    // Like the Expo app, poll only while a screen that shows live status is visible.
                    if matches!(app.page(), Page::Home | Page::Games | Page::Server) {
                        app.refresh_servers();
                    }
                }
            });
    }

    fn handle_os_event(self: &Rc<Self>, event: OsEvent) {
        match event {
            OsEvent::Back => self.back(),
            OsEvent::Route(path) => match Route::from_path(&path) {
                Some(route) => self.navigate(route_page(route)),
                None => platform::log(&format!("ignored notification route {path:?}")),
            },
            OsEvent::PushToken {
                token: Some(token),
                silent,
                ..
            } => self.register_push(token, silent),
            OsEvent::PushToken {
                token: None,
                error,
                silent,
            } => {
                let message = error.unwrap_or_else(|| "Notifications could not be enabled.".into());
                platform::log(&format!("push: {message}"));
                if !silent {
                    self.ui(|s| s.set_more_notice(message.into()));
                }
            }
            OsEvent::ImagePicked {
                bytes,
                name,
                mime,
                error,
            } => self.image_picked(bytes, name, mime, error),
        }
    }

    /// Sends this device's FCM token to the website (it detects the provider from the token).
    fn register_push(self: &Rc<Self>, token: String, silent: bool) {
        let api = self.api.clone();
        let task = self
            .rt
            .spawn(async move { api.register_push(&token, PushPlatform::Android).await });
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            let notice = match task.await {
                Ok(Ok(_)) => "Notifications are enabled on this device.".to_owned(),
                Ok(Err(e)) => {
                    platform::log(&format!("push register: {}", error_chain(&e)));
                    e.to_string()
                }
                Err(_) => "Notifications could not be enabled.".to_owned(),
            };
            if !silent {
                app.ui(|s| s.set_more_notice(notice.into()));
            }
        });
    }

    fn ui(&self, f: impl FnOnce(AppState<'_>)) {
        if let Some(ui) = self.ui.upgrade() {
            f(ui.global::<AppState>());
        }
    }

    fn page(&self) -> Page {
        self.stack.borrow().last().copied().unwrap_or(Page::Home)
    }

    // ------------------------------------------------------------ navigation

    fn navigate(self: &Rc<Self>, page: Page) {
        let is_tab = matches!(
            page,
            Page::Home | Page::News | Page::Streams | Page::Gallery | Page::More
        );
        {
            let mut stack = self.stack.borrow_mut();
            if is_tab {
                stack.clear();
            }
            if stack.last() != Some(&page) {
                stack.push(page);
            }
        }
        self.show(page);
    }

    /// Back pops the stack; on another tab's root it goes Home; on Home it leaves the app
    /// (Android) like other phone apps.
    fn back(self: &Rc<Self>) {
        if self.cancel_confirm() {
            return;
        }
        let page = {
            let mut stack = self.stack.borrow_mut();
            if stack.len() > 1 {
                stack.pop();
            } else if stack.last().is_some_and(|p| *p != Page::Home) {
                *stack = vec![Page::Home];
            } else {
                drop(stack);
                platform::leave_app();
                return;
            }
            *stack.last().unwrap_or(&Page::Home)
        };
        self.show(page);
    }

    fn open_server(self: &Rc<Self>, slug: &str) {
        *self.server_slug.borrow_mut() = slug.to_owned();
        self.server_notice.borrow_mut().clear();
        {
            // A sibling mode replaces the current server page, like router.replace.
            let mut stack = self.stack.borrow_mut();
            if stack.last() == Some(&Page::Server) {
                stack.pop();
            }
            stack.push(Page::Server);
        }
        self.show(Page::Server);
    }

    fn show(self: &Rc<Self>, page: Page) {
        self.ui(|s| s.set_page(page));
        match page {
            Page::Home => self.refresh_servers(),
            Page::News => self.load("news", false),
            Page::Streams => self.load("streams", false),
            Page::Gallery => self.load("gallery", false),
            Page::About => self.load("about", false),
            Page::Games | Page::Server => {
                self.render_server();
                self.load("games", false);
                self.refresh_servers();
            }
            Page::More => {}
            Page::Staff
            | Page::ManageNews
            | Page::ManageGallery
            | Page::ManageStaff
            | Page::PushHistory
            | Page::Account
            | Page::ManageServers
            | Page::EditServer => self.show_staff_page(page),
        }
    }

    // ------------------------------------------------------------ loading

    /// Starts loading a resource: the first call shows the saved copy and refreshes; later calls do
    /// nothing unless `force` (retry), and never while a refresh is already running.
    fn load(self: &Rc<Self>, key: &str, force: bool) {
        match key {
            "news" => self.fetch(
                |a| &a.news,
                "news",
                force,
                |api| async move { api.news().await },
                App::render_news,
            ),
            "streams" => self.fetch(
                |a| &a.streams,
                "streams",
                force,
                |api| async move { api.streams().await },
                App::render_streams,
            ),
            "gallery" => self.fetch(
                |a| &a.gallery,
                "gallery",
                force,
                |api| async move { api.gallery().await },
                App::render_gallery,
            ),
            "games" => self.fetch(
                |a| &a.games,
                "games",
                force,
                |api| async move { api.games().await },
                |app| {
                    app.render_games();
                    app.render_server();
                },
            ),
            "about" => self.fetch(
                |a| &a.about,
                "about",
                force,
                |api| async move { api.about().await },
                App::render_about,
            ),
            _ => {}
        }
    }

    fn fetch<T, Fut>(
        self: &Rc<Self>,
        slot: fn(&App) -> &Slot<T>,
        key: &'static str,
        force: bool,
        request: impl FnOnce(ApiClient) -> Fut,
        render: fn(&Rc<App>),
    ) where
        T: Serialize + DeserializeOwned + Send + 'static,
        Fut: Future<Output = Result<T, ApiError>> + Send + 'static,
    {
        {
            let mut current = slot(self).borrow_mut();
            match current.as_mut() {
                None => *current = Some(Resource::load_cached(&self.cache, key)),
                Some(res) if force && !res.refreshing => res.begin_refresh(),
                Some(_) => return,
            }
        }
        render(self);

        let task = self.rt.spawn(request(self.api.clone()));
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            let Ok(result) = task.await else { return };
            if let Err(e) = &result {
                platform::log(&format!("{key}: {}", error_chain(e)));
            }
            if let Some(res) = slot(&app).borrow_mut().as_mut() {
                res.finish_refresh(&app.cache, result);
            }
            render(&app);
        });
    }

    fn refresh_servers(self: &Rc<Self>) {
        if self.servers_loading.replace(true) {
            return;
        }
        let api = self.api.clone();
        let task = self.rt.spawn(async move { api.servers().await });
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            app.servers_loading.set(false);
            // Keep the last good response when a refresh fails.
            if let Ok(Ok(status)) = task.await {
                *app.servers.borrow_mut() = Some(status);
                app.render_status();
            }
        });
    }

    // ------------------------------------------------------------ rendering

    fn render_news(self: &Rc<Self>) {
        let news = self.news.borrow();
        let Some(res) = news.as_ref() else { return };
        let cards = view::news_cards(res.data.as_ref().map_or(&[], |d| &d.items));
        let state = view::load_state(res.view());
        self.ui(|s| {
            s.set_news_state(state);
            s.set_news(cards);
        });
    }

    fn render_streams(self: &Rc<Self>) {
        let streams = self.streams.borrow();
        let res = streams.as_ref();
        let (rows, eyebrow) = view::stream_rows(
            res.and_then(|r| r.data.as_ref())
                .map(|d| d.items.as_slice()),
        );
        let state = res.map_or_else(ready, |r| view::load_state(r.view()));
        self.ui(|s| {
            s.set_streams_state(state);
            s.set_streams(rows);
            s.set_streams_eyebrow(eyebrow);
        });
    }

    fn render_gallery(self: &Rc<Self>) {
        let gallery = self.gallery.borrow();
        let Some(res) = gallery.as_ref() else { return };
        let items = res.data.as_ref().map_or(&[][..], |d| &d.items);
        let tiles = items
            .iter()
            .map(|item| {
                let picture = self.images.get(
                    &item.image_url,
                    GALLERY_PX,
                    self.rerender(App::render_gallery),
                );
                GalleryTile {
                    title: item.title.as_str().into(),
                    has_picture: picture.is_some(),
                    picture: picture.unwrap_or_default(),
                }
            })
            .collect();
        let state = view::load_state(res.view());
        let empty = items.is_empty();
        self.ui(|s| {
            s.set_gallery_state(state);
            s.set_gallery(view::gallery_rows(tiles));
            s.set_gallery_empty(empty);
        });
    }

    fn render_games(self: &Rc<Self>) {
        let games = self.games.borrow();
        let Some(res) = games.as_ref() else { return };
        let status = self.servers.borrow();
        let sections =
            view::game_sections(res.data.as_ref().map_or(&[], |d| &d.items), status.as_ref());
        let state = view::load_state(res.view());
        self.ui(|s| {
            s.set_games_state(state);
            s.set_hosted(sections.hosted);
            s.set_operated(sections.operated);
            s.set_community(sections.community);
        });
    }

    fn render_server(self: &Rc<Self>) {
        let slug = self.server_slug.borrow().clone();
        let games = self.games.borrow();
        let res = games.as_ref();
        let items = res
            .and_then(|r| r.data.as_ref())
            .map_or(&[][..], |d| &d.items);
        let server = match items
            .iter()
            .find(|g| g.slug.as_deref() == Some(slug.as_str()))
        {
            Some(game) => {
                let banner = game
                    .image
                    .as_deref()
                    .filter(|i| !i.is_empty())
                    .and_then(|image| {
                        self.images.get(
                            &self.api.site_asset(image),
                            BANNER_PX,
                            self.rerender(App::render_server),
                        )
                    });
                view::server_view(
                    game,
                    items,
                    self.servers.borrow().as_ref(),
                    self.api.site_root(),
                    banner,
                    &self.server_notice.borrow(),
                    platform::SHARE_LABEL,
                )
            }
            None => ServerView {
                found: false,
                state: res.map_or_else(loading, |r| view::load_state(r.view())),
                ..Default::default()
            },
        };
        self.ui(|s| s.set_server(server));
    }

    fn render_about(self: &Rc<Self>) {
        let about = self.about.borrow();
        let res = about.as_ref();
        let data = res.and_then(|r| r.data.as_ref());
        let intro = data.map_or(DEFAULT_INTRO, |d| d.about.intro.as_str());
        let staff: Vec<StaffCard> = data
            .map(|d| d.staff.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|m| {
                let avatar = m.image_url.as_deref().and_then(|url| {
                    self.images
                        .get_circle(url, AVATAR_PX, self.rerender(App::render_about))
                });
                StaffCard {
                    role: m.role.to_uppercase().into(),
                    name: m.name.as_str().into(),
                    bio: m.bio.as_str().into(),
                    initials: m.initials.as_str().into(),
                    has_avatar: avatar.is_some(),
                    avatar: avatar.unwrap_or_default(),
                }
            })
            .collect();
        let state = res.map_or_else(ready, |r| view::load_state(r.view()));
        self.ui(|s| {
            s.set_about_state(state);
            s.set_about_intro(intro.into());
            s.set_staff(ModelRc::new(VecModel::from(staff)));
        });
    }

    /// Live status changed: refresh everything that shows it.
    fn render_status(self: &Rc<Self>) {
        let status = self.servers.borrow();
        let (online, players) = view::home_stats(status.as_ref());
        let stats = view::stats(status.as_ref());
        drop(status);
        self.ui(|s| {
            s.set_home_online(online);
            s.set_home_players(players);
            s.set_stats(stats);
        });
        self.render_games();
        self.render_server();
    }

    fn rerender(self: &Rc<Self>, render: fn(&Rc<App>)) -> impl FnOnce() + 'static {
        let weak = Rc::downgrade(self);
        move || {
            if let Some(app) = weak.upgrade() {
                render(&app);
            }
        }
    }

    // ------------------------------------------------------------ server actions

    fn current_game(&self) -> Option<(Game, String, String)> {
        let slug = self.server_slug.borrow();
        let games = self.games.borrow();
        let game = games
            .as_ref()?
            .data
            .as_ref()?
            .items
            .iter()
            .find(|g| g.slug.as_deref() == Some(slug.as_str()))?
            .clone();
        let servers = self.servers.borrow();
        let live = servers.as_ref()?.items.get(slug.as_str())?;
        Some((game, live.connect.clone(), live.connect_url.clone()))
    }

    fn launch_server(self: &Rc<Self>) {
        let Some((game, _, connect_url)) = self.current_game() else {
            return;
        };
        if connect_url.is_empty() {
            return;
        }
        self.server_notice.borrow_mut().clear();
        if platform::open_url(&connect_url).is_err() {
            *self.server_notice.borrow_mut() = view::launch_failed_notice(&game).to_owned();
        }
        self.render_server();
    }

    fn share_address(self: &Rc<Self>) {
        let Some((game, connect, _)) = self.current_game() else {
            return;
        };
        if connect.is_empty() {
            return;
        }
        let notice = match platform::share_text(&view::share_text(&game, &connect)) {
            Ok(message) => message.to_owned(),
            Err(_) => {
                "Could not copy the address. Select it above and copy it manually.".to_owned()
            }
        };
        *self.server_notice.borrow_mut() = notice;
        self.render_server();
    }
}

fn route_page(route: Route) -> Page {
    match route {
        Route::News => Page::News,
        Route::Streams => Page::Streams,
        Route::Gallery => Page::Gallery,
        Route::Games => Page::Games,
        Route::About => Page::About,
        Route::More => Page::More,
    }
}

fn open_url(url: &str) {
    if let Err(e) = platform::open_url(url) {
        platform::log(&format!("could not open {url}: {e}"));
    }
}

/// An error with all of its causes, e.g. "Unable to reach… : error sending request: invalid
/// peer certificate: UnknownIssuer", for the log.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn ready() -> LoadState {
    LoadState {
        kind: LoadKind::Ready,
        message: "".into(),
    }
}

fn loading() -> LoadState {
    LoadState {
        kind: LoadKind::Loading,
        message: "".into(),
    }
}
