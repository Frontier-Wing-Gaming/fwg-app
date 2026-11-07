//! Game server helpers (port of `src/lib/games.ts`).

use std::collections::HashMap;

use crate::models::{Game, GameServerType};

/// Servers that share a `game` key (several modes of one game) become one group, in API order.
pub fn group_games(items: &[Game]) -> Vec<Vec<&Game>> {
    let mut index: HashMap<&str, usize> = HashMap::new();
    let mut groups: Vec<Vec<&Game>> = Vec::new();
    for game in items {
        let key = game
            .game
            .as_deref()
            .or(game.slug.as_deref())
            .unwrap_or(&game.name);
        match index.get(key) {
            Some(&i) => groups[i].push(game),
            None => {
                index.insert(key, groups.len());
                groups.push(vec![game]);
            }
        }
    }
    groups
}

fn has_server(game: &Game) -> bool {
    game.server
        .as_ref()
        .is_some_and(|s| s.kind != GameServerType::None)
}

/// A dedicated server FWG runs itself: not a community group and not run by someone else.
pub fn is_fwg_hosted(game: &Game) -> bool {
    has_server(game) && !has_operator(game)
}

/// A server run by a community member rather than FWG.
pub fn is_operated(game: &Game) -> bool {
    has_server(game) && has_operator(game)
}

fn has_operator(game: &Game) -> bool {
    game.operator.as_deref().is_some_and(|o| !o.is_empty())
}

/// Eyebrow text for a server card: the genre, plus who runs it when that isn't FWG.
pub fn genre_label(game: &Game) -> String {
    match game.operator.as_deref().filter(|o| !o.is_empty()) {
        Some(op) => format!("{} · Run by {op}", game.genre),
        None => game.genre.clone(),
    }
}

/// The plain game name ("Minecraft") of a server that may be one mode of several.
pub fn base_name(game: &Game) -> &str {
    if let Some(name) = game.game_name.as_deref().filter(|n| !n.is_empty()) {
        return name;
    }
    match game.variant.as_deref().filter(|v| !v.is_empty()) {
        Some(variant) => {
            let suffix = format!(" — {variant}");
            game.name
                .strip_suffix(suffix.as_str())
                .unwrap_or(&game.name)
        }
        None => &game.name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::GameServer;

    fn game(name: &str, key: Option<&str>) -> Game {
        Game {
            name: name.into(),
            game: key.map(Into::into),
            genre: "Sandbox".into(),
            ..Default::default()
        }
    }

    #[test]
    fn groups_by_game_key_in_order() {
        let items = [
            game("MC — Modded", Some("mc")),
            game("Rust", None),
            game("MC — Vanilla", Some("mc")),
        ];
        let groups = group_games(&items);
        let names: Vec<Vec<&str>> = groups
            .iter()
            .map(|g| g.iter().map(|x| x.name.as_str()).collect())
            .collect();
        assert_eq!(names, [vec!["MC — Modded", "MC — Vanilla"], vec!["Rust"]]);
    }

    #[test]
    fn hosting_and_labels() {
        let mut g = game("Rust", None);
        assert!(!is_fwg_hosted(&g) && !is_operated(&g));
        g.server = Some(GameServer {
            kind: GameServerType::A2s,
            ..Default::default()
        });
        assert!(is_fwg_hosted(&g));
        assert_eq!(genre_label(&g), "Sandbox");
        g.operator = Some("Koopa".into());
        assert!(is_operated(&g) && !is_fwg_hosted(&g));
        assert_eq!(genre_label(&g), "Sandbox · Run by Koopa");
    }

    #[test]
    fn base_name_strips_variant() {
        let mut g = game("Minecraft — Modded", None);
        assert_eq!(base_name(&g), "Minecraft — Modded");
        g.variant = Some("Modded".into());
        assert_eq!(base_name(&g), "Minecraft");
        g.game_name = Some("MC".into());
        assert_eq!(base_name(&g), "MC");
    }
}
