//! Livestream list helpers (from `src/app/streams.tsx`).

use crate::models::Streamer;

/// Shown (all offline) until `/streams.php` answers or when it has never answered.
pub const FALLBACK_LOGINS: [&str; 9] = [
    "therealcobratrinity",
    "blooberryfoxttv",
    "kisarinmoon",
    "whiteangelvr",
    "sonicwinddemon",
    "KoopaVr_",
    "firebrand83",
    "renaloemberheart",
    "thechaos_kitsune",
];

pub fn fallback_streamers() -> Vec<Streamer> {
    FALLBACK_LOGINS
        .iter()
        .map(|login| Streamer {
            login: (*login).into(),
            display_name: (*login).into(),
            live: false,
            title: None,
            game: None,
        })
        .collect()
}

/// Live creators first, otherwise API order.
pub fn sort_live_first(items: &mut [Streamer]) {
    items.sort_by_key(|s| !s.live);
}

pub fn live_count(items: &[Streamer]) -> usize {
    items.iter().filter(|s| s.live).count()
}

pub fn twitch_url(streamer: &Streamer) -> String {
    format!("https://twitch.tv/{}", streamer.login)
}

/// "Streaming Rust" / the stream title when live, "Offline" otherwise.
pub fn status_line(streamer: &Streamer) -> String {
    if !streamer.live {
        return "Offline".into();
    }
    match streamer.title.as_deref().filter(|t| !t.is_empty()) {
        Some(title) => title.to_owned(),
        None => format!("Streaming {}", streamer.game.as_deref().unwrap_or("")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_first_is_stable() {
        let mut items = fallback_streamers();
        items[3].live = true;
        items[7].live = true;
        sort_live_first(&mut items);
        assert_eq!(items[0].login, "whiteangelvr");
        assert_eq!(items[1].login, "renaloemberheart");
        assert_eq!(items[2].login, "therealcobratrinity");
        assert_eq!(live_count(&items), 2);
        assert_eq!(status_line(&items[0]), "Streaming ");
        assert_eq!(status_line(&items[2]), "Offline");
    }
}
