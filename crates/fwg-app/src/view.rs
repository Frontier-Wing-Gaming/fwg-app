//! Pure conversions from `fwg-core` data to the structs the Slint UI displays.

use fwg_core::{
    cache::ResourceView,
    format, games,
    markdown::{self, BlockKind, SpanStyle},
    models::{
        Game, GameServerType, NewsItem, ServerState, ServerStatus, ServerStatusResponse, Streamer,
    },
    server_status, streams,
};
use slint::{Color, Image, ModelRc, SharedString, StyledText, VecModel};

use crate::{
    Badge, GalleryRow, GalleryTile, GameCard, LinkItem, LoadKind, LoadState, MdBlock, MdKind,
    ModeRow, NewsCard, ServerLed, ServerView, Stats, Step, StreamRow,
};

pub const STATUS_PAGE: &str = "https://status.frontierwinggaming.network/";

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn strings(items: impl IntoIterator<Item = impl Into<SharedString>>) -> ModelRc<SharedString> {
    model(items.into_iter().map(Into::into).collect())
}

pub fn load_state(view: ResourceView) -> LoadState {
    match view {
        ResourceView::Loading => LoadState {
            kind: LoadKind::Loading,
            message: "".into(),
        },
        ResourceView::Failed { error } => LoadState {
            kind: LoadKind::Failed,
            message: error.into(),
        },
        ResourceView::Offline { updated_at } => {
            let message = match updated_at {
                Some(ms) => format!("Offline copy · updated {}", format::timestamp_label(ms)),
                None => "Offline copy".to_owned(),
            };
            LoadState {
                kind: LoadKind::Offline,
                message: message.into(),
            }
        }
        ResourceView::Ready => LoadState {
            kind: LoadKind::Ready,
            message: "".into(),
        },
    }
}

pub fn led(state: ServerState) -> ServerLed {
    match state {
        ServerState::Online => ServerLed::Online,
        ServerState::Offline => ServerLed::Offline,
        ServerState::Unknown => ServerLed::Unknown,
        ServerState::Unlisted => ServerLed::Unlisted,
        ServerState::Community => ServerLed::Community,
        ServerState::Hosted => ServerLed::Hosted,
        ServerState::Inactive => ServerLed::Inactive,
    }
}

pub fn badge(status: Option<&ServerStatus>) -> Badge {
    let state = status.map_or(ServerState::Unknown, |s| s.status);
    Badge {
        led: led(state),
        label: state.label().to_uppercase().into(),
        detail: server_status::server_detail(status).into(),
    }
}

/// `#rgb`, `#rrggbb` or `#rrggbbaa`; anything else becomes the theme cyan.
pub fn parse_color(value: &str) -> Color {
    let fallback = Color::from_rgb_u8(0x42, 0xd9, 0xff);
    let Some(hex) = value.trim().strip_prefix('#') else {
        return fallback;
    };
    let digits: Option<Vec<u8>> = match hex.len() {
        3 => hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| (d * 17) as u8))
            .collect(),
        6 | 8 => (0..hex.len())
            .step_by(2)
            .map(|i| {
                hex.get(i..i + 2)
                    .and_then(|p| u8::from_str_radix(p, 16).ok())
            })
            .collect(),
        _ => None,
    };
    match digits.as_deref() {
        Some([r, g, b]) => Color::from_rgb_u8(*r, *g, *b),
        Some([r, g, b, a]) => Color::from_argb_u8(*a, *r, *g, *b),
        _ => fallback,
    }
}

pub fn markdown_blocks(source: &str) -> ModelRc<MdBlock> {
    let blocks = markdown::parse(source)
        .iter()
        .map(|block| {
            let mut md = block.to_commonmark();
            if block.kind == BlockKind::Heading {
                md = format!("**{md}**");
            }
            let text = StyledText::from_markdown(&md)
                .unwrap_or_else(|_| StyledText::from_plain_text(&block.plain_text()));
            let kind = match block.kind {
                BlockKind::Paragraph => MdKind::Paragraph,
                BlockKind::Heading => MdKind::Heading,
                BlockKind::Bullet => MdKind::Bullet,
                BlockKind::Quote => MdKind::Quote,
            };
            let heavy = block.kind == BlockKind::Heading
                || block
                    .spans
                    .iter()
                    .any(|s| matches!(s.style, SpanStyle::Bold | SpanStyle::Code));
            MdBlock {
                kind,
                text,
                plain: block.plain_text().into(),
                measure_weight: if heavy { 800 } else { 400 },
            }
        })
        .collect();
    model(blocks)
}

pub fn news_cards(items: &[NewsItem]) -> ModelRc<NewsCard> {
    model(
        items
            .iter()
            .map(|item| NewsCard {
                date: format::date_label(&item.published_at).to_uppercase().into(),
                title: item.title.as_str().into(),
                blocks: markdown_blocks(&item.body),
            })
            .collect(),
    )
}

/// Rows (live first) and the "N CREATORS LIVE" eyebrow. Uses the built-in list until the API answers.
pub fn stream_rows(items: Option<&[Streamer]>) -> (ModelRc<StreamRow>, SharedString) {
    let mut items = items.map_or_else(streams::fallback_streamers, <[Streamer]>::to_vec);
    streams::sort_live_first(&mut items);
    let eyebrow = format!("{} CREATORS LIVE", streams::live_count(&items));
    let rows = items
        .iter()
        .map(|s| StreamRow {
            name: s.display_name.as_str().into(),
            line: streams::status_line(s).into(),
            live: s.live,
            url: streams::twitch_url(s).into(),
        })
        .collect();
    (model(rows), eyebrow.into())
}

pub fn gallery_rows(tiles: Vec<GalleryTile>) -> ModelRc<GalleryRow> {
    let rows = tiles
        .chunks(2)
        .map(|pair| GalleryRow {
            left: pair[0].clone(),
            right: pair.get(1).cloned().unwrap_or_default(),
            has_right: pair.len() == 2,
        })
        .collect();
    model(rows)
}

/// Home stat tiles: "● 3/5" + "12", or the defaults until hosted servers report in.
pub fn home_stats(status: Option<&ServerStatusResponse>) -> (SharedString, SharedString) {
    match status.map(|s| &s.summary).filter(|s| s.hosted > 0) {
        Some(s) => (
            format!("● {}/{}", s.online, s.hosted).into(),
            s.players.to_string().into(),
        ),
        None => ("● Online".into(), "Open to all".into()),
    }
}

pub fn stats(status: Option<&ServerStatusResponse>) -> Stats {
    match status.map(|s| &s.summary).filter(|s| s.hosted > 0) {
        Some(s) => Stats {
            show: true,
            online: format!("{}/{}", s.online, s.hosted).into(),
            players: s.players.to_string().into(),
        },
        None => Stats::default(),
    }
}

fn live<'a>(game: &Game, status: Option<&'a ServerStatusResponse>) -> Option<&'a ServerStatus> {
    status.and_then(|s| s.items.get(game.slug.as_deref()?))
}

fn has_no_server(game: &Game) -> bool {
    game.server
        .as_ref()
        .is_none_or(|s| s.kind == GameServerType::None)
}

fn game_card(group: &[&Game], status: Option<&ServerStatusResponse>) -> GameCard {
    let first = group[0];
    let accent = parse_color(&first.accent);
    let eyebrow = games::genre_label(first).to_uppercase().into();
    if group.len() == 1 {
        let live = live(first, status);
        return GameCard {
            accent,
            eyebrow,
            title: first.name.as_str().into(),
            description: first.description.as_str().into(),
            slug: first.slug.clone().unwrap_or_default().into(),
            url: first.url.as_str().into(),
            has_badge: first.slug.is_some(),
            badge: badge(live),
            connect: live.map(|l| l.connect.as_str()).unwrap_or_default().into(),
            ..Default::default()
        };
    }
    let modes = group
        .iter()
        .map(|game| {
            let live = live(game, status);
            let name = game
                .variant
                .as_deref()
                .filter(|v| !v.is_empty())
                .unwrap_or(&game.name);
            ModeRow {
                name: name.to_uppercase().into(),
                slug: game.slug.clone().unwrap_or_default().into(),
                url: game.url.as_str().into(),
                badge: badge(live),
                connect: live.map(|l| l.connect.as_str()).unwrap_or_default().into(),
            }
        })
        .collect();
    GameCard {
        accent,
        eyebrow,
        title: games::base_name(first).into(),
        description: first.description.as_str().into(),
        grouped: true,
        modes_label: format!("{} MODES", group.len()).into(),
        modes: model(modes),
        ..Default::default()
    }
}

pub struct GameSections {
    pub hosted: ModelRc<GameCard>,
    pub operated: ModelRc<GameCard>,
    pub community: ModelRc<GameCard>,
}

pub fn game_sections(items: &[Game], status: Option<&ServerStatusResponse>) -> GameSections {
    // Older API responses have no `server` field; show them all in one list as before.
    let has_server_data = items.iter().any(|g| g.server.is_some());
    let pick =
        |f: fn(&Game) -> bool| -> Vec<Game> { items.iter().filter(|g| f(g)).cloned().collect() };
    let (hosted, operated, community) = if has_server_data {
        (
            pick(games::is_fwg_hosted),
            pick(games::is_operated),
            pick(has_no_server),
        )
    } else {
        (items.to_vec(), Vec::new(), Vec::new())
    };
    let cards = |list: &[Game]| {
        model(
            games::group_games(list)
                .iter()
                .map(|g| game_card(g, status))
                .collect(),
        )
    };
    GameSections {
        hosted: cards(&hosted),
        operated: cards(&operated),
        community: cards(&community),
    }
}

/// Everything the server detail page shows. `banner` is the loaded banner image, if any.
pub fn server_view(
    game: &Game,
    all: &[Game],
    status: Option<&ServerStatusResponse>,
    site_root: &str,
    banner: Option<Image>,
    notice: &str,
    share_label: &str,
) -> ServerView {
    let live = live(game, status);
    let hosted = !has_no_server(game);
    let operator = game.operator.clone().unwrap_or_default();
    let online = live.is_some_and(|l| l.status == ServerState::Online);
    let base = games::base_name(game);

    let eyebrow: Vec<&str> = [Some(game.genre.as_str()), game.variant.as_deref()]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .collect();

    let pack = game.download.as_ref();
    let pack_show = pack.is_some_and(|p| !p.url.is_empty())
        || game.modpack.as_ref().is_some_and(|m| !m.slug.is_empty());
    let chips: Vec<String> = pack
        .map(|p| {
            [
                p.version.as_ref().map(|v| format!("v{v}")),
                p.minecraft.as_ref().map(|m| format!("Minecraft {m}")),
                p.loader.clone(),
                p.mods.filter(|&m| m > 0).map(|m| format!("{m} mods")),
            ]
            .into_iter()
            .flatten()
            .filter(|c| !c.is_empty())
            .collect()
        })
        .unwrap_or_default();

    let siblings: Vec<ModeRow> = match &game.game {
        Some(key) => all
            .iter()
            .filter(|g| g.game.as_ref() == Some(key) && g.slug != game.slug && g.slug.is_some())
            .map(|g| ModeRow {
                name: g
                    .variant
                    .as_deref()
                    .filter(|v| !v.is_empty())
                    .unwrap_or(&g.name)
                    .into(),
                slug: g.slug.clone().unwrap_or_default().into(),
                ..Default::default()
            })
            .collect(),
        None => Vec::new(),
    };

    let mut links = Vec::new();
    if let Some(url) = game.mods_url.as_deref().filter(|u| !u.is_empty()) {
        links.push(("Mod collection", url.to_owned()));
    }
    if let Some(url) = game.discord_channel.as_deref().filter(|u| !u.is_empty()) {
        links.push(("Discord channel", url.to_owned()));
    }
    links.push(("Official game site", game.url.clone()));
    links.push(("Network status", STATUS_PAGE.to_owned()));

    let notes = game.notes.clone().unwrap_or_default();
    let callout = game.callout.clone().unwrap_or_default();

    ServerView {
        found: true,
        state: LoadState {
            kind: LoadKind::Ready,
            message: "".into(),
        },
        title: base.into(),
        eyebrow: eyebrow.join(" · ").to_uppercase().into(),
        has_banner: banner.is_some(),
        banner: banner.unwrap_or_default(),
        description: game.description.as_str().into(),
        operator: operator.as_str().into(),
        accent: parse_color(&game.accent),
        hosted,
        badge: badge(live),
        players: match live {
            Some(l) if online => format!(
                "{} / {}",
                l.players.unwrap_or(0),
                l.max_players
                    .map_or_else(|| "?".to_owned(), |m| m.to_string())
            ),
            _ => "—".to_owned(),
        }
        .into(),
        map: live
            .filter(|_| online)
            .and_then(|l| l.map.as_deref())
            .unwrap_or_default()
            .into(),
        connect: live.map(|l| l.connect.as_str()).unwrap_or_default().into(),
        can_launch: live.is_some_and(|l| !l.connect_url.is_empty()),
        share_label: share_label.into(),
        no_address: format!(
            "The address is posted in the {} channel on Discord.",
            game.name
        )
        .into(),
        community_text: if notes.is_empty() {
            "This game runs on official servers. The wing meets up through Discord.".into()
        } else {
            notes.as_str().into()
        },
        notice: notice.into(),
        pack_show,
        pack_name: pack
            .and_then(|p| p.name.clone())
            .unwrap_or_else(|| "Modpack required".into())
            .into(),
        pack_chips: chips.join("  ·  ").into(),
        pack_url: format!(
            "{site_root}/servers/{}#modpack",
            game.slug.as_deref().unwrap_or_default()
        )
        .into(),
        join: model(
            game.join
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, text)| Step {
                    no: format!("{:02}", i + 1).into(),
                    text: text.as_str().into(),
                })
                .collect(),
        ),
        rules: strings(game.rules.iter().flatten().map(String::as_str)),
        operator_notes: if hosted {
            notes.as_str().into()
        } else {
            "".into()
        },
        callout_title: callout
            .title
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "Heads up".into())
            .to_uppercase()
            .into(),
        callout_text: callout.text.unwrap_or_default().into(),
        siblings_label: format!("OTHER {} MODES", base.to_uppercase()).into(),
        siblings: model(siblings),
        links: model(
            links
                .into_iter()
                .map(|(label, url)| LinkItem {
                    label: label.into(),
                    url: url.into(),
                })
                .collect(),
        ),
    }
}

/// The text shared or copied for a server address.
pub fn share_text(game: &Game, connect: &str) -> String {
    match game.operator.as_deref().filter(|o| !o.is_empty()) {
        Some(op) => format!("{} (run by {op}): {connect}", game.name),
        None => format!("{} on Frontier Wing Gaming: {connect}", game.name),
    }
}

/// Shown when the game's connect link cannot be opened on this device.
pub fn launch_failed_notice(game: &Game) -> &'static str {
    if game
        .server
        .as_ref()
        .is_some_and(|s| s.kind == GameServerType::Fivem)
    {
        "FiveM is not installed on this device. Copy the address and connect from your PC."
    } else {
        "Steam could not open this link here. Copy the address and connect from your PC."
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#ff5a1f"), Color::from_rgb_u8(0xff, 0x5a, 0x1f));
        assert_eq!(parse_color("#fff"), Color::from_rgb_u8(255, 255, 255));
        assert_eq!(parse_color("red"), Color::from_rgb_u8(0x42, 0xd9, 0xff));
    }

    #[test]
    fn streams_fall_back_to_built_in_list() {
        let (rows, eyebrow) = stream_rows(None);
        assert_eq!(rows.row_count(), 9);
        assert_eq!(eyebrow.as_str(), "0 CREATORS LIVE");
    }

    #[test]
    fn gallery_pairs_tiles() {
        let tile = |t: &str| GalleryTile {
            title: t.into(),
            ..Default::default()
        };
        let rows = gallery_rows(vec![tile("a"), tile("b"), tile("c")]);
        assert_eq!(rows.row_count(), 2);
        assert!(!rows.row_data(1).unwrap().has_right);
    }
}
