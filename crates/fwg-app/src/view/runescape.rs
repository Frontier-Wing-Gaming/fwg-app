//! RuneScape leaderboard, profile and competition data → the rows the Slint pages show.

use fwg_core::{
    format,
    models::{
        RsBoard, RsCompetition, RsCompetitionDetail, RsCompetitionStatus, RsEntry, RsEvent, RsGame,
        RsMetric, RsPeriod, RsProfile, RsProfileGame,
    },
};
use slint::ModelRc;

use super::model;
use crate::{RsBoardChoice, RsCompRow, RsFeedRow, RsLine, RsRow};

fn number(value: Option<i64>) -> String {
    value.map_or_else(|| "—".to_owned(), format::thousands)
}

/// "xp" → " XP", "qp" → " QP", "score" (bosses, clues, minigames) → no suffix.
fn suffix(unit: &str) -> &'static str {
    match unit {
        "score" => "",
        "qp" => " QP",
        _ => " XP",
    }
}

fn metric_kind(metric: &str) -> &str {
    metric.split(':').next().unwrap_or("overall")
}

/// Leaderboard rows: the gain (week/month) or current value (all time) on the right, the rest on a
/// line underneath. The unit and details follow the board: Overall, a skill, a boss or quests.
pub fn rows(board: &RsBoard) -> ModelRc<RsRow> {
    let kind = metric_kind(&board.metric);
    let unit = if board.unit.is_empty() {
        "xp"
    } else {
        &board.unit
    };
    let gains = board.period != RsPeriod::All && kind != "quests";
    model(
        board
            .items
            .iter()
            .enumerate()
            .map(|(i, entry)| row(i, entry, kind, unit, gains))
            .collect(),
    )
}

fn row(index: usize, entry: &RsEntry, kind: &str, unit: &str, gains: bool) -> RsRow {
    // Boards saved by older app versions have no `value`; Overall's value is the total XP.
    let current = entry.value.or(if kind == "overall" || kind.is_empty() {
        entry.total_xp
    } else {
        None
    });
    let gained = entry.gained.unwrap_or(0);
    let value = if gains {
        if gained > 0 {
            format!("+{}{}", format::thousands(gained), suffix(unit))
        } else {
            format!("0{}", suffix(unit))
        }
    } else {
        format!("{}{}", number(current), suffix(unit))
    };

    let mut detail = Vec::new();
    match kind {
        "skill" => detail.push(format!("Level {}", number(entry.level))),
        "activity" if gains => detail.push(format!("Total {}", number(current))),
        "quests" => {
            if let (Some(done), Some(total)) = (entry.quests_completed, entry.quests_total) {
                detail.push(format!("{done} of {total} quests"));
            }
        }
        "activity" => {}
        _ => detail.push(format!("Total level {}", number(entry.total_level))),
    }
    if gains {
        if let Some(skill) = &entry.top_skill {
            detail.push(format!(
                "Top skill: {} +{}",
                skill.name,
                format::thousands(skill.xp)
            ));
        }
        if entry.partial
            && let Some(since) = &entry.since
        {
            detail.push(format!("Joined {}", format::date_label(since)));
        }
    } else if kind != "quests"
        && let Some(rank) = entry.rank
    {
        detail.push(format!("Hiscores rank {}", format::thousands(rank)));
    }
    RsRow {
        id: i32::try_from(entry.id).unwrap_or(0),
        place: (index + 1).to_string().into(),
        name: entry.name.as_str().into(),
        badge: entry.account_short.as_str().into(),
        value: value.into(),
        gained: gains && gained > 0,
        detail: detail.join(" · ").into(),
    }
}

/// The note under a board: how gains are counted and when the stats last updated.
pub fn footnote(board: &RsBoard) -> String {
    if board.items.is_empty() {
        return String::new();
    }
    if board.metric == "quests" {
        return "Quest points from the RuneScape 3 Adventurer's Log, checked daily. Members whose profile is private aren't listed.".to_owned();
    }
    let mut text = String::new();
    if board.period != RsPeriod::All {
        text.push_str(if board.unit == "score" {
            "Score gained"
        } else {
            "XP gained"
        });
        text.push_str(" since the last update before this period began. Members who joined during it count from their first update. ");
    }
    text.push_str("Updated daily from the official Jagex hiscores");
    if let Some(updated) = &board.updated_at {
        text.push_str(", last on ");
        text.push_str(&format::date_label(updated));
    }
    text.push('.');
    text
}

/// The board picker, with a heading over the first board of each group.
pub fn board_choices(metrics: &[RsMetric], selected: &str) -> ModelRc<RsBoardChoice> {
    let mut last_group = "";
    model(
        metrics
            .iter()
            .map(|m| {
                let heading = if m.group != last_group && m.group != "Overall" {
                    m.group.to_uppercase()
                } else {
                    String::new()
                };
                last_group = &m.group;
                RsBoardChoice {
                    value: m.value.as_str().into(),
                    label: m.label.as_str().into(),
                    heading: heading.into(),
                    selected: m.value == selected,
                }
            })
            .collect(),
    )
}

/// Achievements: "Settled reached 99 Attack" over "Old School · Nov 16, 2026". A Hardcore death
/// also shows its cause; Adventurer's Log details only repeat the title, so they're left out.
pub fn feed_rows(events: &[RsEvent]) -> ModelRc<RsFeedRow> {
    model(
        events
            .iter()
            .map(|e| {
                let mut detail = vec![e.game.label().to_owned()];
                if let Some(at) = &e.happened_at {
                    detail.push(format::date_label(at));
                }
                if e.kind == "death"
                    && let Some(cause) = e.detail.as_deref().filter(|d| !d.is_empty())
                {
                    detail.push(cause.to_owned());
                }
                RsFeedRow {
                    title: format!("{} {}", e.name, e.text).into(),
                    detail: detail.join(" · ").into(),
                    kind: e.kind.as_str().into(),
                }
            })
            .collect(),
    )
}

fn status(status: RsCompetitionStatus) -> (&'static str, &'static str) {
    match status {
        RsCompetitionStatus::Upcoming => ("upcoming", "UPCOMING"),
        RsCompetitionStatus::Running => ("running", "RUNNING NOW"),
        RsCompetitionStatus::Ended => ("ended", "FINISHED"),
    }
}

fn range(c: &RsCompetition) -> String {
    format!(
        "{} – {}",
        format::datetime_label(&c.starts_at),
        format::datetime_label(&c.ends_at)
    )
}

pub fn competition_rows(items: &[RsCompetition]) -> ModelRc<RsCompRow> {
    model(
        items
            .iter()
            .map(|c| {
                let (key, label) = status(c.status);
                RsCompRow {
                    id: i32::try_from(c.id).unwrap_or(0),
                    title: c.title.as_str().into(),
                    status: key.into(),
                    status_label: label.into(),
                    detail: format!("{} · {} · {}", c.game.label(), c.metric_label, range(c))
                        .into(),
                }
            })
            .collect(),
    )
}

/// What the competition page shows.
pub struct CompetitionView {
    pub title: String,
    pub eyebrow: String,
    pub status: &'static str,
    pub status_label: &'static str,
    pub detail: String,
    pub rows: ModelRc<RsRow>,
    pub footnote: String,
}

pub fn competition(detail: &RsCompetitionDetail) -> CompetitionView {
    let c = &detail.competition;
    let (key, label) = status(c.status);
    let unit = suffix(&c.unit);
    let upcoming = c.status == RsCompetitionStatus::Upcoming;
    let rows = if upcoming {
        Vec::new()
    } else {
        detail
            .items
            .iter()
            .enumerate()
            .map(|(i, s)| RsRow {
                id: i32::try_from(s.id).unwrap_or(0),
                place: s
                    .gained
                    .map_or_else(|| "—".to_owned(), |_| (i + 1).to_string())
                    .into(),
                name: s.name.as_str().into(),
                badge: s.account_short.as_str().into(),
                value: match s.gained {
                    Some(g) if g > 0 => format!("+{}{unit}", format::thousands(g)),
                    Some(_) => format!("0{unit}"),
                    None => "waiting".to_owned(),
                }
                .into(),
                gained: s.gained.is_some_and(|g| g > 0),
                detail: match (s.start, s.end) {
                    (Some(start), Some(end)) => format!(
                        "Start {} · Now {}",
                        format::thousands(start),
                        format::thousands(end)
                    ),
                    _ => "No stats captured during the competition yet.".to_owned(),
                }
                .into(),
            })
            .collect()
    };
    let detail_text = if upcoming {
        format!(
            "{}. Everyone on the {} board takes part automatically; stats are captured just before the start and just before the end.",
            range(c),
            c.game.label()
        )
    } else {
        range(c)
    };
    let footnote = match c.status {
        _ if rows.is_empty() && !upcoming => "Nobody on this board yet.".to_owned(),
        RsCompetitionStatus::Running => {
            "Standings refresh every few hours. Final stats are captured just before the end."
                .to_owned()
        }
        RsCompetitionStatus::Ended => "Final standings.".to_owned(),
        RsCompetitionStatus::Upcoming => String::new(),
    };
    CompetitionView {
        title: c.title.clone(),
        eyebrow: format!("{} · {}", c.game.label(), c.metric_label).to_uppercase(),
        status: key,
        status_label: label,
        detail: detail_text,
        rows: model(rows),
        footnote,
    }
}

fn line(kind: &str, title: impl Into<String>, value: impl Into<String>) -> RsLine {
    RsLine {
        kind: kind.into(),
        title: title.into().into(),
        value: value.into().into(),
        ..Default::default()
    }
}

/// A profile as a flat list of lines (see `RsLine` in types.slint): per game a heading, any
/// Hardcore death, the totals, quests (RS3), then skills and bosses with this week's gains.
pub fn profile_lines(profile: &RsProfile) -> ModelRc<RsLine> {
    let mut lines = Vec::new();
    for game in &profile.games {
        game_lines(game, &mut lines);
    }
    model(lines)
}

fn game_lines(game: &RsProfileGame, lines: &mut Vec<RsLine>) {
    let mut eyebrow = game.game.label().to_uppercase();
    if !game.account_short.is_empty() {
        eyebrow.push_str(" · ");
        eyebrow.push_str(&game.account_label.to_uppercase());
    }
    lines.push(RsLine {
        detail: eyebrow.into(),
        ..line("section", game.name.as_str(), "")
    });
    if let Some(death) = &game.death {
        let mut detail: Vec<String> = death.detail.iter().cloned().collect();
        if let Some(at) = &death.at {
            detail.push(format::date_label(at));
        }
        lines.push(RsLine {
            detail: detail.join(" · ").into(),
            highlight: true,
            ..line("note", format!("{} {}", game.name, death.text), "")
        });
    }
    if game.total_xp.is_none() && game.skills.is_empty() {
        lines.push(RsLine {
            detail: "No stats yet. They appear after the next daily update.".into(),
            ..line("note", "", "")
        });
        return;
    }
    lines.push(line("stat", "Total level", number(game.total_level)));
    lines.push(line("stat", "Total XP", number(game.total_xp)));
    let week = game.week_gained.unwrap_or(0);
    lines.push(RsLine {
        highlight: week > 0,
        ..line(
            "stat",
            "XP this week",
            if week > 0 {
                format!("+{}", format::thousands(week))
            } else {
                "0".to_owned()
            },
        )
    });
    if let Some(rank) = game.rank {
        lines.push(line("stat", "Hiscores rank", format::thousands(rank)));
    }
    if game.game == RsGame::Rs3
        && let Some(q) = game.quests.as_ref().filter(|q| q.total > 0)
    {
        lines.push(RsLine {
            detail: format!(
                "{} of {} quest points",
                format::thousands(q.qp),
                format::thousands(q.qp_max)
            )
            .into(),
            ..line("stat", "Quests", format!("{} / {}", q.completed, q.total))
        });
    }
    if !game.skills.is_empty() {
        lines.push(line("heading", "SKILLS", ""));
        for skill in &game.skills {
            let gained = skill.week_gained.unwrap_or(0);
            let mut detail = format!("{} XP", number(skill.xp));
            if gained > 0 {
                detail.push_str(&format!(" · +{} this week", format::thousands(gained)));
            }
            lines.push(RsLine {
                detail: detail.into(),
                highlight: gained > 0,
                ..line(
                    "row",
                    skill.name.as_str(),
                    format!("Lv {}", number(skill.level)),
                )
            });
        }
    }
    if !game.activities.is_empty() {
        lines.push(line("heading", "BOSSES AND ACTIVITIES", ""));
        for activity in &game.activities {
            let gained = activity.week_gained.unwrap_or(0);
            lines.push(RsLine {
                detail: if gained > 0 {
                    format!("+{} this week", format::thousands(gained)).into()
                } else {
                    Default::default()
                },
                highlight: gained > 0,
                ..line(
                    "row",
                    activity.name.as_str(),
                    format::thousands(activity.score),
                )
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fwg_core::models::{RsActivity, RsDeath, RsQuests, RsSkill, RsSkillGain, RsStanding};
    use slint::Model;

    fn entry(name: &str, gained: Option<i64>, partial: bool) -> RsEntry {
        RsEntry {
            id: 2,
            name: name.into(),
            value: Some(5_800_000_000),
            level: Some(3232),
            unit: "xp".into(),
            account_short: String::new(),
            quests_completed: None,
            quests_total: None,
            total_level: Some(3232),
            total_xp: Some(5_800_000_000),
            rank: Some(34),
            updated_at: None,
            gained,
            since: Some("2026-11-16T19:19:17Z".into()),
            partial,
            top_skill: gained.filter(|g| *g > 0).map(|xp| RsSkillGain {
                name: "Attack".into(),
                xp,
            }),
        }
    }

    fn board(period: RsPeriod, metric: &str, unit: &str, items: Vec<RsEntry>) -> RsBoard {
        RsBoard {
            game: RsGame::Rs3,
            period,
            metric: metric.into(),
            metric_label: String::new(),
            unit: unit.into(),
            metrics: vec![],
            updated_at: Some("2026-11-16".into()),
            items,
        }
    }

    #[test]
    fn overall_rows() {
        let items = vec![
            entry("Le Me", Some(1_000_000), false),
            entry("New", Some(0), true),
        ];
        let week = rows(&board(RsPeriod::Week, "overall", "xp", items.clone()));
        let first = week.row_data(0).unwrap();
        assert_eq!(
            (
                first.id,
                first.place.as_str(),
                first.value.as_str(),
                first.gained
            ),
            (2, "1", "+1,000,000 XP", true)
        );
        assert_eq!(
            first.detail.as_str(),
            "Total level 3,232 · Top skill: Attack +1,000,000"
        );
        let second = week.row_data(1).unwrap();
        assert_eq!((second.value.as_str(), second.gained), ("0 XP", false));
        assert!(second.detail.contains("Joined "));

        let all = board(RsPeriod::All, "overall", "xp", items);
        let row = rows(&all).row_data(0).unwrap();
        assert_eq!(row.value.as_str(), "5,800,000,000 XP");
        assert_eq!(row.detail.as_str(), "Total level 3,232 · Hiscores rank 34");
        assert_eq!(
            footnote(&all),
            "Updated daily from the official Jagex hiscores, last on Nov 16, 2026."
        );
    }

    #[test]
    fn older_cached_boards_still_show_total_xp() {
        let mut old = entry("Le Me", None, false);
        old.value = None;
        old.unit = String::new();
        let row = rows(&board(RsPeriod::All, "", "", vec![old]))
            .row_data(0)
            .unwrap();
        assert_eq!(row.value.as_str(), "5,800,000,000 XP");
    }

    #[test]
    fn skill_boss_and_quest_rows() {
        let mut skill = entry("Le Me", None, false);
        (skill.value, skill.level) = (Some(13_034_431), Some(99));
        let row = rows(&board(RsPeriod::All, "skill:Attack", "xp", vec![skill]))
            .row_data(0)
            .unwrap();
        assert_eq!(row.value.as_str(), "13,034,431 XP");
        assert_eq!(row.detail.as_str(), "Level 99 · Hiscores rank 34");

        let mut boss = entry("Settled", Some(93), false);
        (boss.value, boss.top_skill, boss.account_short) = (Some(333), None, "UIM".into());
        let b = board(RsPeriod::Week, "activity:Zulrah", "score", vec![boss]);
        let row = rows(&b).row_data(0).unwrap();
        assert_eq!(
            (row.value.as_str(), row.detail.as_str(), row.badge.as_str()),
            ("+93", "Total 333", "UIM")
        );
        assert!(footnote(&b).starts_with("Score gained"));

        let mut quests = entry("Le Me", None, false);
        (quests.value, quests.quests_completed, quests.quests_total) =
            (Some(476), Some(360), Some(366));
        // Quest points have no period: the week board still shows the total.
        let row = rows(&board(RsPeriod::Week, "quests", "qp", vec![quests]))
            .row_data(0)
            .unwrap();
        assert_eq!(
            (row.value.as_str(), row.detail.as_str()),
            ("476 QP", "360 of 366 quests")
        );
    }

    #[test]
    fn board_picker_groups() {
        let m = |v: &str, g: &str| RsMetric {
            value: v.into(),
            label: v.into(),
            group: g.into(),
        };
        let choices = board_choices(
            &[
                m("overall", "Overall"),
                m("skill:Attack", "Skills"),
                m("skill:Defence", "Skills"),
                m("activity:Zulrah", "Bosses and activities"),
            ],
            "skill:Defence",
        );
        let headings: Vec<_> = choices.iter().map(|c| c.heading.to_string()).collect();
        assert_eq!(headings, ["", "SKILLS", "", "BOSSES AND ACTIVITIES"]);
        assert!(choices.row_data(2).unwrap().selected);
    }

    #[test]
    fn feed_and_competitions() {
        let event = |kind: &str, detail: Option<&str>| RsEvent {
            id: 1,
            member_id: 2,
            name: "Settled".into(),
            game: RsGame::Osrs,
            kind: kind.into(),
            text: "lost Hardcore Ironman status".into(),
            detail: detail.map(Into::into),
            happened_at: Some("2026-11-16".into()),
        };
        let feed = feed_rows(&[
            event("death", Some("Lumbridge")),
            event("log", Some("repeat")),
        ]);
        assert_eq!(
            feed.row_data(0).unwrap().title.as_str(),
            "Settled lost Hardcore Ironman status"
        );
        assert_eq!(
            feed.row_data(0).unwrap().detail.as_str(),
            "Old School · Nov 16, 2026 · Lumbridge"
        );
        assert_eq!(
            feed.row_data(1).unwrap().detail.as_str(),
            "Old School · Nov 16, 2026"
        );

        let comp = |status| RsCompetition {
            id: 1,
            title: "SOTW".into(),
            game: RsGame::Osrs,
            metric_label: "Attack".into(),
            unit: "xp".into(),
            starts_at: "2026-11-16".into(),
            ends_at: "2026-11-23".into(),
            status,
        };
        let row = competition_rows(&[comp(RsCompetitionStatus::Running)])
            .row_data(0)
            .unwrap();
        assert_eq!(
            (row.status.as_str(), row.detail.as_str()),
            (
                "running",
                "Old School · Attack · Nov 16, 2026 – Nov 23, 2026"
            )
        );

        let standing = |name: &str, gained: Option<i64>| RsStanding {
            id: 4,
            name: name.into(),
            gained,
            start: gained.map(|_| 1),
            end: gained.map(|g| g + 1),
            account_short: String::new(),
        };
        let ended = competition(&RsCompetitionDetail {
            competition: comp(RsCompetitionStatus::Ended),
            items: vec![standing("A", Some(123_456)), standing("B", None)],
        });
        assert_eq!(ended.status, "ended");
        let a = ended.rows.row_data(0).unwrap();
        let b = ended.rows.row_data(1).unwrap();
        assert_eq!(
            (a.place.as_str(), a.value.as_str(), a.detail.as_str()),
            ("1", "+123,456 XP", "Start 1 · Now 123,457")
        );
        assert_eq!((b.place.as_str(), b.value.as_str()), ("—", "waiting"));
        assert_eq!(ended.footnote, "Final standings.");

        let upcoming = competition(&RsCompetitionDetail {
            competition: comp(RsCompetitionStatus::Upcoming),
            items: vec![standing("A", Some(0))],
        });
        assert_eq!(upcoming.rows.row_count(), 0);
        assert!(upcoming.detail.contains("takes part automatically"));
    }

    #[test]
    fn profile() {
        let game = |g: RsGame| RsProfileGame {
            game: g,
            name: "Le Me".into(),
            account_label: "Regular account".into(),
            account_short: String::new(),
            death: None,
            updated_at: None,
            total_level: Some(3232),
            total_xp: Some(5_800_000_000),
            rank: Some(34),
            week_gained: Some(5),
            skills: vec![RsSkill {
                name: "Attack".into(),
                level: Some(120),
                xp: Some(200_000_000),
                rank: Some(1),
                week_gained: Some(5),
            }],
            activities: vec![RsActivity {
                name: "Zulrah".into(),
                score: 2500,
                rank: None,
                week_gained: Some(0),
            }],
            quests: Some(RsQuests {
                qp: 476,
                qp_max: 476,
                completed: 360,
                total: 366,
            }),
        };
        let mut fallen = game(RsGame::Osrs);
        fallen.account_short = "HC†".into();
        fallen.account_label = "Fallen Hardcore".into();
        fallen.death = Some(RsDeath {
            text: "lost Hardcore Ironman status".into(),
            detail: Some("Lumbridge".into()),
            at: None,
        });
        fallen.total_xp = None;
        fallen.skills.clear();
        let lines = profile_lines(&RsProfile {
            id: 2,
            name: "Le Me".into(),
            games: vec![game(RsGame::Rs3), fallen],
            events: vec![],
        });
        let all: Vec<_> = lines
            .iter()
            .map(|l| format!("{}|{}|{}|{}", l.kind, l.title, l.value, l.detail))
            .collect();
        assert_eq!(all[0], "section|Le Me||RUNESCAPE 3");
        assert!(all.contains(&"stat|XP this week|+5|".to_owned()));
        assert!(all.contains(&"stat|Quests|360 / 366|476 of 476 quest points".to_owned()));
        assert!(all.contains(&"row|Attack|Lv 120|200,000,000 XP · +5 this week".to_owned()));
        assert!(all.contains(&"row|Zulrah|2,500|".to_owned()));
        assert!(all.contains(&"section|Le Me||OLD SCHOOL · FALLEN HARDCORE".to_owned()));
        assert!(all.contains(&"note|Le Me lost Hardcore Ironman status||Lumbridge".to_owned()));
        assert!(all.last().unwrap().contains("No stats yet"));
    }
}
