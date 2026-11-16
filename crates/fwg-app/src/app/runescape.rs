//! RuneScape leaderboard and sign-up (the website's `/runescape` page), plus its board picker,
//! member profiles and competition standings.
//!
//! Every board (game, period and skill/boss), profile and competition is its own cached resource,
//! so going back to one shows the saved copy at once and works offline. The sign-up form posts
//! straight to the website, where staff approve it.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    hash::Hash,
    rc::Rc,
};

use fwg_core::{
    ApiClient, ApiError, Resource,
    models::{
        Items, RsBoard, RsCompetition, RsCompetitionDetail, RsCompetitionStatus, RsEvent, RsGame,
        RsMetric, RsPeriod, RsProfile, RsSignup,
    },
};
use serde::{Serialize, de::DeserializeOwned};
use slint::{ComponentHandle, SharedString};

use super::{App, error_chain};
use crate::{AppWindow, Page, RuneScapeState, platform, view, view::runescape as rsview};

type Slot<K, T> = RefCell<HashMap<K, Resource<T>>>;
/// Game, period and board ("overall", "skill:Attack", …).
type BoardKey = (RsGame, RsPeriod, String);

/// Achievements shown on the leaderboard page (the website shows more).
const FEED_LIMIT: u32 = 8;
/// Finished competitions shown under the running and upcoming ones.
const FINISHED_SHOWN: usize = 3;

pub(super) struct RuneScapeData {
    boards: Slot<BoardKey, RsBoard>,
    feed: Slot<(), Items<RsEvent>>,
    competitions: Slot<(), Items<RsCompetition>>,
    players: Slot<i64, RsProfile>,
    details: Slot<i64, RsCompetitionDetail>,
    selected: RefCell<BoardKey>,
    player: Cell<i64>,
    competition: Cell<i64>,
    busy: Cell<bool>,
}

impl Default for RuneScapeData {
    fn default() -> Self {
        Self {
            boards: Slot::default(),
            feed: Slot::default(),
            competitions: Slot::default(),
            players: Slot::default(),
            details: Slot::default(),
            selected: RefCell::new((RsGame::Rs3, RsPeriod::Week, "overall".to_owned())),
            player: Cell::new(0),
            competition: Cell::new(0),
            busy: Cell::new(false),
        }
    }
}

impl App {
    pub(super) fn bind_runescape(self: &Rc<Self>, ui: &AppWindow) {
        let state = ui.global::<RuneScapeState>();
        let weak = Rc::downgrade(self);
        let with = move |f: fn(&Rc<App>)| {
            let weak = weak.clone();
            move || {
                if let Some(app) = weak.upgrade() {
                    f(&app);
                }
            }
        };
        state.on_select({
            let weak = Rc::downgrade(self);
            move |game, period| {
                if let Some(app) = weak.upgrade() {
                    app.select_board(game.as_str(), period.as_str());
                }
            }
        });
        state.on_select_board({
            let weak = Rc::downgrade(self);
            move |metric| {
                if let Some(app) = weak.upgrade() {
                    app.runescape.selected.borrow_mut().2 = metric.to_string();
                    app.back();
                }
            }
        });
        state.on_open_player({
            let weak = Rc::downgrade(self);
            move |id| {
                if let Some(app) = weak.upgrade()
                    && id > 0
                {
                    app.runescape.player.set(i64::from(id));
                    app.navigate(Page::RunescapePlayer);
                }
            }
        });
        state.on_open_competition({
            let weak = Rc::downgrade(self);
            move |id| {
                if let Some(app) = weak.upgrade()
                    && id > 0
                {
                    app.runescape.competition.set(i64::from(id));
                    app.navigate(Page::RunescapeCompetition);
                }
            }
        });
        state.on_reload(with(|app| app.load_runescape(true)));
        state.on_reload_player(with(|app| app.load_rs_player(true)));
        state.on_reload_competition(with(|app| app.load_rs_competition(true)));
        state.on_sign_up(with(|app| app.runescape_sign_up()));
    }

    fn rs_ui(&self, f: impl FnOnce(RuneScapeState<'_>)) {
        if let Some(ui) = self.ui.upgrade() {
            f(ui.global::<RuneScapeState>());
        }
    }

    /// Switches game or period. A board the other game doesn't have (most bosses) falls back to
    /// Overall; quest points are RuneScape 3 only.
    fn select_board(self: &Rc<Self>, game: &str, period: &str) {
        let game = RsGame::ALL.into_iter().find(|g| g.key() == game);
        let period = RsPeriod::ALL.into_iter().find(|p| p.key() == period);
        let (Some(game), Some(period)) = (game, period) else {
            return;
        };
        let metric = self.runescape.selected.borrow().2.clone();
        let keep = metric == "overall"
            || self
                .known_metrics(game)
                .is_some_and(|known| known.iter().any(|m| m.value == metric));
        *self.runescape.selected.borrow_mut() = (
            game,
            period,
            if keep { metric } else { "overall".to_owned() },
        );
        self.load_runescape(false);
    }

    /// The boards a game has, from any of its boards loaded so far (the selected one first).
    fn known_metrics(&self, game: RsGame) -> Option<Vec<RsMetric>> {
        let selected = self.runescape.selected.borrow().clone();
        let boards = self.runescape.boards.borrow();
        let has = |b: &&RsBoard| !b.metrics.is_empty();
        boards
            .get(&selected)
            .and_then(|r| r.data.as_ref())
            .filter(|b| b.game == game)
            .filter(has)
            .or_else(|| {
                boards
                    .iter()
                    .filter(|(k, _)| k.0 == game)
                    .find_map(|(_, r)| r.data.as_ref().filter(has))
            })
            .map(|b| b.metrics.clone())
    }

    /// Shows a cached resource at once and fetches it the first time (or on `force`, i.e. retry),
    /// like the other screens. `render` runs before and after the fetch.
    #[allow(clippy::too_many_arguments)]
    fn rs_load<K, T, Fut>(
        self: &Rc<Self>,
        slot: fn(&RuneScapeData) -> &Slot<K, T>,
        key: K,
        cache_key: String,
        force: bool,
        fetch: impl FnOnce(ApiClient) -> Fut,
        render: fn(&Rc<App>),
    ) where
        K: Eq + Hash + Clone + 'static,
        T: Serialize + DeserializeOwned + Send + 'static,
        Fut: Future<Output = Result<T, ApiError>> + Send + 'static,
    {
        let start = {
            let mut map = slot(&self.runescape).borrow_mut();
            match map.get_mut(&key) {
                None => {
                    map.insert(key.clone(), Resource::load_cached(&self.cache, &cache_key));
                    true
                }
                Some(res) if force && !res.refreshing => {
                    res.begin_refresh();
                    true
                }
                Some(_) => false,
            }
        };
        render(self);
        if !start {
            return;
        }
        let task = self.rt.spawn(fetch(self.api.clone()));
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            let Ok(result) = task.await else { return };
            if let Err(e) = &result {
                platform::log(&format!("{cache_key}: {}", error_chain(e)));
            }
            if let Some(res) = slot(&app.runescape).borrow_mut().get_mut(&key) {
                res.finish_refresh(&app.cache, result);
            }
            render(&app);
        });
    }

    /// The leaderboard page: the selected board, competitions and the latest achievements.
    pub(super) fn load_runescape(self: &Rc<Self>, force: bool) {
        let key = self.runescape.selected.borrow().clone();
        let (game, period, metric) = key.clone();
        // Overall keeps the key from before there were other boards, so saved copies still load.
        let cache_key = if metric == "overall" {
            format!("runescape.{}.{}", game.key(), period.key())
        } else {
            format!("runescape.{}.{}.{metric}", game.key(), period.key())
        };
        self.rs_load(
            |d| &d.boards,
            key,
            cache_key,
            force,
            move |api| async move { api.runescape(game, period, &metric).await },
            |app| app.render_runescape(),
        );
        self.rs_load(
            |d| &d.competitions,
            (),
            "runescape.competitions".to_owned(),
            force,
            |api| async move { api.runescape_competitions().await },
            |app| app.render_runescape_lists(),
        );
        self.rs_load(
            |d| &d.feed,
            (),
            "runescape.feed".to_owned(),
            force,
            |api| async move { api.runescape_feed(FEED_LIMIT).await },
            |app| app.render_runescape_lists(),
        );
    }

    pub(super) fn load_rs_player(self: &Rc<Self>, force: bool) {
        let id = self.runescape.player.get();
        if id <= 0 {
            return;
        }
        self.rs_load(
            |d| &d.players,
            id,
            format!("runescape.player.{id}"),
            force,
            move |api| async move { api.runescape_player(id).await },
            |app| app.render_rs_player(),
        );
    }

    pub(super) fn load_rs_competition(self: &Rc<Self>, force: bool) {
        let id = self.runescape.competition.get();
        if id <= 0 {
            return;
        }
        self.rs_load(
            |d| &d.details,
            id,
            format!("runescape.competition.{id}"),
            force,
            move |api| async move { api.runescape_competition(id).await },
            |app| app.render_rs_competition(),
        );
    }

    fn render_runescape(self: &Rc<Self>) {
        let key = self.runescape.selected.borrow().clone();
        let (game, period, metric) = key.clone();
        let metrics = self.known_metrics(game).unwrap_or_default();
        let boards = self.runescape.boards.borrow();
        let Some(res) = boards.get(&key) else {
            return;
        };
        let board = res.data.as_ref();
        let rows = board.map_or_else(Default::default, rsview::rows);
        let footnote = board.map(rsview::footnote).unwrap_or_default();
        let label = metrics
            .iter()
            .find(|m| m.value == metric)
            .map(|m| m.label.clone())
            .or_else(|| board.map(|b| b.metric_label.clone()))
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "Overall".to_owned());
        let state = view::load_state(res.view());
        drop(boards);
        self.rs_ui(|s| {
            s.set_game(game.key().into());
            s.set_period(period.key().into());
            s.set_game_label(game.label().into());
            s.set_metric(metric.as_str().into());
            s.set_metric_label(label.into());
            s.set_boards(rsview::board_choices(&metrics, &metric));
            s.set_state(state);
            s.set_rows(rows);
            s.set_footnote(footnote.into());
        });
    }

    /// Competitions and achievements. Both just stay hidden while loading or when unavailable.
    fn render_runescape_lists(self: &Rc<Self>) {
        let competitions: Vec<RsCompetition> = self
            .runescape
            .competitions
            .borrow()
            .get(&())
            .and_then(|r| r.data.as_ref())
            .map(|d| {
                let (open, done): (Vec<_>, Vec<_>) = d
                    .items
                    .iter()
                    .cloned()
                    .partition(|c| c.status != RsCompetitionStatus::Ended);
                open.into_iter()
                    .chain(done.into_iter().take(FINISHED_SHOWN))
                    .collect()
            })
            .unwrap_or_default();
        let feed: Vec<RsEvent> = self
            .runescape
            .feed
            .borrow()
            .get(&())
            .and_then(|r| r.data.as_ref())
            .map(|d| d.items.clone())
            .unwrap_or_default();
        self.rs_ui(|s| {
            s.set_competitions(rsview::competition_rows(&competitions));
            s.set_feed(rsview::feed_rows(&feed));
        });
    }

    fn render_rs_player(self: &Rc<Self>) {
        let id = self.runescape.player.get();
        let players = self.runescape.players.borrow();
        let Some(res) = players.get(&id) else { return };
        let state = view::load_state(res.view());
        let profile = res.data.as_ref();
        let name = profile.map(|p| p.name.clone()).unwrap_or_default();
        let lines = profile.map(rsview::profile_lines).unwrap_or_default();
        let feed = profile.map_or_else(Default::default, |p| rsview::feed_rows(&p.events));
        drop(players);
        self.rs_ui(|s| {
            s.set_player_state(state);
            s.set_player_name(name.into());
            s.set_player_lines(lines);
            s.set_player_feed(feed);
        });
    }

    fn render_rs_competition(self: &Rc<Self>) {
        let id = self.runescape.competition.get();
        let details = self.runescape.details.borrow();
        let Some(res) = details.get(&id) else { return };
        let state = view::load_state(res.view());
        let shown = res.data.as_ref().map(rsview::competition);
        drop(details);
        self.rs_ui(|s| {
            s.set_comp_state(state);
            match shown {
                Some(c) => {
                    s.set_comp_title(c.title.into());
                    s.set_comp_eyebrow(c.eyebrow.into());
                    s.set_comp_status(c.status.into());
                    s.set_comp_status_label(c.status_label.into());
                    s.set_comp_detail(c.detail.into());
                    s.set_comp_rows(c.rows);
                    s.set_comp_footnote(c.footnote.into());
                }
                None => {
                    s.set_comp_title(SharedString::new());
                    s.set_comp_eyebrow(SharedString::new());
                    s.set_comp_status(SharedString::new());
                    s.set_comp_rows(Default::default());
                    s.set_comp_footnote(SharedString::new());
                }
            }
        });
    }

    fn runescape_sign_up(self: &Rc<Self>) {
        if self.runescape.busy.get() {
            return;
        }
        let Some(ui) = self.ui.upgrade() else { return };
        let form = ui.global::<RuneScapeState>();
        let signup = RsSignup {
            discord: form.get_discord().trim().to_owned(),
            rs3_name: form.get_rs3_name().trim().to_owned(),
            osrs_name: form.get_osrs_name().trim().to_owned(),
            message: form.get_message().trim().to_owned(),
            consent: form.get_consent(),
        };
        drop(form);
        // Same checks as the website form, so the common mistakes don't need a round trip.
        let problem = if signup.discord.is_empty() {
            Some("Enter your Discord username so staff can reach you.")
        } else if signup.rs3_name.is_empty() && signup.osrs_name.is_empty() {
            Some("Enter at least one RuneScape name: RuneScape 3, Old School, or both.")
        } else if !signup.consent {
            Some("Please agree to your names and stats being shown on the leaderboard.")
        } else {
            None
        };
        if let Some(problem) = problem {
            self.rs_ui(|s| {
                s.set_notice(problem.into());
                s.set_notice_error(true);
            });
            return;
        }

        self.runescape.busy.set(true);
        self.rs_ui(|s| {
            s.set_busy(true);
            s.set_notice(SharedString::new());
        });
        let api = self.api.clone();
        let task = self
            .rt
            .spawn(async move { api.runescape_signup(&signup).await });
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            app.runescape.busy.set(false);
            let (notice, error, clear) = match task.await {
                Ok(Ok(result)) => {
                    let mut lines = vec![if result.message.is_empty() {
                        "Thanks! Staff will review your sign-up.".to_owned()
                    } else {
                        result.message
                    }];
                    lines.extend(result.warnings);
                    (lines.join("\n\n"), false, true)
                }
                Ok(Err(e)) => {
                    platform::log(&format!("runescape signup: {}", error_chain(&e)));
                    (e.to_string(), true, false)
                }
                Err(_) => ("Sign-up failed. Please try again.".to_owned(), true, false),
            };
            app.rs_ui(|s| {
                s.set_busy(false);
                s.set_notice(notice.into());
                s.set_notice_error(error);
                if clear {
                    s.set_discord(SharedString::new());
                    s.set_rs3_name(SharedString::new());
                    s.set_osrs_name(SharedString::new());
                    s.set_message(SharedString::new());
                    s.set_consent(false);
                }
            });
        });
    }
}
