//! Website API types (mirror of the `type` declarations in the Expo app's `src/lib/api.ts`).
//!
//! Fields the TypeScript marks optional are `Option`/`#[serde(default)]`, so cached responses
//! from older API versions still load. Enums fall back to an `Other`/`Unknown` variant when a
//! newer website sends a value this build does not know yet.

use std::collections::HashMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::de;

/// `{ "items": [...] }`, the envelope used by every list endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Items<T> {
    pub items: Vec<T>,
}

// ---------------------------------------------------------------- public content

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewsItem {
    #[serde(deserialize_with = "de::string")]
    pub id: String,
    pub title: String,
    pub body: String,
    pub published_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Streamer {
    pub login: String,
    pub display_name: String,
    #[serde(deserialize_with = "de::boolean")]
    pub live: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GalleryItem {
    #[serde(deserialize_with = "de::string")]
    pub id: String,
    pub title: String,
    pub image_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameServerType {
    A2s,
    Minecraft,
    Fivem,
    Hytale,
    #[default]
    None,
    /// A type added by a newer website version.
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GameDownload {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mods: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ServerCallout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModpackRef {
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GameServer {
    #[serde(rename = "type", default)]
    pub kind: GameServerType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect: Option<String>,
}

/// A game server card. `game`/`variant`/`game_name` group several modes of one game:
/// `name` is "Minecraft — Modded", `game_name` "Minecraft", `variant` "Modded".
/// `operator` is set when someone other than FWG runs the server.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Game {
    pub name: String,
    pub url: String,
    pub genre: String,
    pub accent: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callout: Option<ServerCallout>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<GameDownload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modpack: Option<ModpackRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<GameServer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mods_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discord_channel: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerState {
    Online,
    Offline,
    Unlisted,
    Community,
    Hosted,
    Inactive,
    /// Also used for states added by a later website version (shown as "Checking").
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ServerStatus {
    pub status: ServerState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub players: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_players: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<f64>,
    #[serde(default)]
    pub connect: String,
    #[serde(default)]
    pub connect_url: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ServerSummary {
    pub hosted: u32,
    pub online: u32,
    pub players: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ServerStatusResponse {
    pub checked_at: f64,
    pub summary: ServerSummary,
    /// Keyed by server slug.
    pub items: HashMap<String, ServerStatus>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StaffMember {
    pub name: String,
    pub role: String,
    pub group: String,
    pub bio: String,
    pub initials: String,
    #[serde(default)]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AboutSection {
    pub intro: String,
    pub body: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct About {
    pub name: String,
    pub tagline: String,
    pub description: String,
    pub about: AboutSection,
    pub staff: Vec<StaffMember>,
}

// ---------------------------------------------------------------- auth

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub name: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: User,
}

// ---------------------------------------------------------------- staff administration

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminNews {
    #[serde(deserialize_with = "de::int")]
    pub id: i64,
    pub title: String,
    pub body: String,
    #[serde(deserialize_with = "de::boolean")]
    pub published: bool,
    pub published_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminGallery {
    #[serde(deserialize_with = "de::int")]
    pub id: i64,
    pub title: String,
    pub image_url: String,
    #[serde(deserialize_with = "de::boolean")]
    pub published: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StaffRole {
    Owner,
    Admin,
    Editor,
}

impl StaffRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Editor => "editor",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminStaff {
    #[serde(deserialize_with = "de::int")]
    pub id: i64,
    pub name: String,
    pub email: String,
    pub role: StaffRole,
    #[serde(deserialize_with = "de::boolean")]
    pub active: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AdminServerValues {
    /// Older website versions have no `active` value; every server was active then.
    #[serde(default = "active_default")]
    pub active: bool,
    pub description: String,
    pub host: String,
    pub port: Option<u16>,
    pub query_port: Option<u16>,
    pub connect: String,
    pub join: Vec<String>,
    pub rules: Vec<String>,
    pub notes: String,
    pub mods_url: String,
    pub discord_channel: String,
}

fn active_default() -> bool {
    true
}

/// Every field optional: used both for partial `download` values and form input.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct GameDownloadPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<String>,
    /// A number from the API, a string from form input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mods: Option<NumOrText>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AdminServerDetails {
    pub name: String,
    pub variant: String,
    pub game: String,
    pub operator: String,
    pub genre: String,
    pub accent: String,
    pub url: String,
    pub image: String,
    pub order: i64,
    pub modpack: String,
    pub modpack_manual: Vec<String>,
    pub download: GameDownloadPatch,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout: Option<ServerCallout>,
}

/// "file" servers come from `game-servers/*.json` and only player-facing values are editable;
/// "staff" servers were added in the app/website and are fully editable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerSource {
    File,
    Staff,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminServer {
    pub slug: String,
    #[serde(default)]
    pub source: Option<ServerSource>,
    pub name: String,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(rename = "type")]
    pub kind: GameServerType,
    pub values: AdminServerValues,
    #[serde(default)]
    pub details: Option<AdminServerDetails>,
    #[serde(deserialize_with = "de::boolean")]
    pub edited: bool,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub updated_by: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminServerTypeOption {
    pub value: String,
    pub label: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub query_port: Option<u16>,
}

/// Choices for the add/edit server form; they come from the website so new types, packs and
/// groups need no app update.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AdminServerOptions {
    pub types: Option<Vec<String>>,
    pub type_options: Option<Vec<AdminServerTypeOption>>,
    pub packs: Option<Vec<String>>,
    /// Ordered as the website sends them.
    pub groups: Option<IndexMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdminServers {
    pub items: Vec<AdminServer>,
    #[serde(default)]
    pub options: Option<AdminServerOptions>,
}

/// A form value the website accepts as either a number or the raw text the user typed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NumOrText {
    Num(i64),
    Text(String),
}

/// A list the website accepts as an array or as newline-separated text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextOrList {
    List(Vec<String>),
    Text(String),
}

/// Body of `POST`/`PATCH /admin/servers.php`. Only the fields that are set are sent.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AdminServerInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<NumOrText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_port: Option<NumOrText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<TextOrList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<TextOrList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mods_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord_channel: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<NumOrText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modpack: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modpack_manual: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download: Option<GameDownloadPatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout: Option<ServerCallout>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushHistory {
    #[serde(deserialize_with = "de::int")]
    pub id: i64,
    pub title: String,
    pub body: String,
    #[serde(deserialize_with = "de::int")]
    pub recipient_count: i64,
    pub status: String,
    pub created_at: String,
    #[serde(default)]
    pub sender: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Android,
    Ios,
}

// ---------------------------------------------------------------- small response bodies

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Success {
    #[serde(deserialize_with = "de::boolean")]
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Created {
    #[serde(deserialize_with = "de::int")]
    pub id: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GalleryUploaded {
    pub item: GalleryItem,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushSent {
    #[serde(deserialize_with = "de::int")]
    pub sent: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PushReceiptsChecked {
    #[serde(deserialize_with = "de::int")]
    pub checked: i64,
    #[serde(deserialize_with = "de::int")]
    pub disabled: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerCreated {
    #[serde(deserialize_with = "de::boolean")]
    pub success: bool,
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerDeleted {
    #[serde(deserialize_with = "de::boolean")]
    pub success: bool,
    #[serde(deserialize_with = "de::boolean")]
    pub deleted: bool,
}

// ---------------------------------------------------------------- RuneScape leaderboard

/// Which hiscores a board or name belongs to: RuneScape 3 or Old School.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RsGame {
    #[default]
    Rs3,
    Osrs,
}

impl RsGame {
    pub const ALL: [RsGame; 2] = [Self::Rs3, Self::Osrs];

    /// The website's query value (`rs3`, `osrs`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Rs3 => "rs3",
            Self::Osrs => "osrs",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rs3 => "RuneScape 3",
            Self::Osrs => "Old School",
        }
    }
}

/// XP gained this week or month, or the all-time totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RsPeriod {
    #[default]
    Week,
    Month,
    All,
}

impl RsPeriod {
    pub const ALL: [RsPeriod; 3] = [Self::Week, Self::Month, Self::All];

    pub fn key(self) -> &'static str {
        match self {
            Self::Week => "week",
            Self::Month => "month",
            Self::All => "all",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Week => "This week",
            Self::Month => "This month",
            Self::All => "All time",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsSkillGain {
    pub name: String,
    pub xp: i64,
}

/// One member on `GET /runescape.php`. Unranked values are `null`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsEntry {
    /// Member id, for the profile (`/runescape/player.php?id=`).
    #[serde(default)]
    pub id: i64,
    pub name: String,
    /// The chosen board's current value: XP, a boss/activity score, or quest points.
    #[serde(default)]
    pub value: Option<i64>,
    /// Skill level (skill boards) or total level (Overall).
    #[serde(default)]
    pub level: Option<i64>,
    /// "xp", "score" or "qp".
    #[serde(default)]
    pub unit: String,
    /// "IM", "HC", "HC†", "UIM", or empty for a regular account.
    #[serde(default)]
    pub account_short: String,
    #[serde(default)]
    pub quests_completed: Option<i64>,
    #[serde(default)]
    pub quests_total: Option<i64>,
    #[serde(default)]
    pub total_level: Option<i64>,
    #[serde(default)]
    pub total_xp: Option<i64>,
    #[serde(default)]
    pub rank: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// `None` on the all-time board.
    #[serde(default)]
    pub gained: Option<i64>,
    /// When the gain is counted from.
    #[serde(default)]
    pub since: Option<String>,
    /// Joined during the period, so the gain only covers part of it.
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub top_skill: Option<RsSkillGain>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsBoard {
    pub game: RsGame,
    pub period: RsPeriod,
    /// "overall", "skill:Attack", "activity:Zulrah" or "quests" (RS3).
    #[serde(default)]
    pub metric: String,
    #[serde(default)]
    pub metric_label: String,
    #[serde(default)]
    pub unit: String,
    /// Every board this game has, for the picker.
    #[serde(default)]
    pub metrics: Vec<RsMetric>,
    /// Latest snapshot time (ISO 8601), `None` while the board is empty.
    #[serde(default)]
    pub updated_at: Option<String>,
    pub items: Vec<RsEntry>,
}

/// Body of `POST /runescape/signup.php`. Empty names are sent as `""`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RsSignup {
    pub discord: String,
    pub rs3_name: String,
    pub osrs_name: String,
    pub message: String,
    pub consent: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsSignupResult {
    #[serde(deserialize_with = "de::boolean")]
    pub success: bool,
    #[serde(default)]
    pub message: String,
    /// Names that weren't found on the hiscores (the sign-up still went through).
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// A board the leaderboard can be ranked by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsMetric {
    pub value: String,
    pub label: String,
    /// "Overall", "Skills", "Quests", "Bosses and activities".
    #[serde(default)]
    pub group: String,
}

/// An achievement on `GET /runescape/feed.php` or a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsEvent {
    pub id: i64,
    #[serde(default)]
    pub member_id: i64,
    pub name: String,
    pub game: RsGame,
    /// "milestone", "death" or "log" (RS3 Adventurer's Log).
    #[serde(default)]
    pub kind: String,
    /// Follows the name: "reached 99 Attack".
    pub text: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub happened_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsSkill {
    pub name: String,
    #[serde(default)]
    pub level: Option<i64>,
    #[serde(default)]
    pub xp: Option<i64>,
    #[serde(default)]
    pub rank: Option<i64>,
    #[serde(default)]
    pub week_gained: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsActivity {
    pub name: String,
    #[serde(default)]
    pub score: i64,
    #[serde(default)]
    pub rank: Option<i64>,
    #[serde(default)]
    pub week_gained: Option<i64>,
}

/// RS3 quest summary from the Adventurer's Log (the app doesn't use the quest list itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsQuests {
    #[serde(default)]
    pub qp: i64,
    #[serde(default)]
    pub qp_max: i64,
    #[serde(default)]
    pub completed: i64,
    #[serde(default)]
    pub total: i64,
}

/// A Hardcore Ironman death: "lost Hardcore Ironman status", cause and place, and when.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsDeath {
    pub text: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub at: Option<String>,
}

/// One game on a profile. The stats are missing until the member's first update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsProfileGame {
    pub game: RsGame,
    pub name: String,
    #[serde(default)]
    pub account_label: String,
    #[serde(default)]
    pub account_short: String,
    #[serde(default)]
    pub death: Option<RsDeath>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub total_level: Option<i64>,
    #[serde(default)]
    pub total_xp: Option<i64>,
    #[serde(default)]
    pub rank: Option<i64>,
    #[serde(default)]
    pub week_gained: Option<i64>,
    #[serde(default)]
    pub skills: Vec<RsSkill>,
    #[serde(default)]
    pub activities: Vec<RsActivity>,
    #[serde(default)]
    pub quests: Option<RsQuests>,
}

/// `GET /runescape/player.php?id=`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsProfile {
    pub id: i64,
    pub name: String,
    pub games: Vec<RsProfileGame>,
    #[serde(default)]
    pub events: Vec<RsEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RsCompetitionStatus {
    #[default]
    Upcoming,
    Running,
    Ended,
}

/// A competition on `GET /runescape/competitions.php`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsCompetition {
    pub id: i64,
    pub title: String,
    pub game: RsGame,
    #[serde(default)]
    pub metric_label: String,
    /// "xp" or "score".
    #[serde(default)]
    pub unit: String,
    pub starts_at: String,
    pub ends_at: String,
    #[serde(default)]
    pub status: RsCompetitionStatus,
}

/// A player in a competition. `gained` is `None` until their stats are captured during it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsStanding {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub gained: Option<i64>,
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub end: Option<i64>,
    #[serde(default)]
    pub account_short: String,
}

/// `GET /runescape/competitions.php?id=`: the competition and its standings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RsCompetitionDetail {
    #[serde(flatten)]
    pub competition: RsCompetition,
    pub items: Vec<RsStanding>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_enum_values_fall_back() {
        let s: ServerStatus = serde_json::from_str(r#"{"status":"maintenance"}"#).unwrap();
        assert_eq!(s.status, ServerState::Unknown);
        let g: GameServer = serde_json::from_str(r#"{"type":"rcon"}"#).unwrap();
        assert_eq!(g.kind, GameServerType::Other);
    }

    #[test]
    fn php_style_numbers_and_flags_are_accepted() {
        let n: AdminNews = serde_json::from_str(
            r#"{"id":"7","title":"t","body":"b","published":"1","published_at":"2026-11-01"}"#,
        )
        .unwrap();
        assert_eq!((n.id, n.published), (7, true));
        let item: NewsItem =
            serde_json::from_str(r#"{"id":12,"title":"t","body":"b","published_at":"x"}"#).unwrap();
        assert_eq!(item.id, "12");
    }

    #[test]
    fn minimal_game_from_older_api_loads() {
        let g: Game = serde_json::from_str(
            r##"{"name":"Rust","url":"u","genre":"Survival","accent":"#f00","description":"d"}"##,
        )
        .unwrap();
        assert!(g.server.is_none() && g.operator.is_none());
    }

    #[test]
    fn server_input_only_sends_set_fields() {
        let input = AdminServerInput {
            port: Some(NumOrText::Text("28015".into())),
            join: Some(TextOrList::List(vec!["a".into()])),
            kind: Some("a2s".into()),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&input).unwrap(),
            serde_json::json!({"port":"28015","join":["a"],"type":"a2s"})
        );
    }

    #[test]
    fn server_group_order_is_preserved() {
        let o: AdminServerOptions =
            serde_json::from_str(r#"{"groups":{"zeta":"Z","alpha":"A"}}"#).unwrap();
        let keys: Vec<_> = o.groups.unwrap().into_keys().collect();
        assert_eq!(keys, ["zeta", "alpha"]);
    }
}

#[cfg(test)]
mod server_value_tests {
    use super::AdminServerValues;

    #[test]
    fn missing_active_means_active() {
        let v: AdminServerValues = serde_json::from_str(r#"{"host":"x"}"#).unwrap();
        assert!(v.active);
        let v: AdminServerValues = serde_json::from_str(r#"{"active":false}"#).unwrap();
        assert!(!v.active);
    }
}
