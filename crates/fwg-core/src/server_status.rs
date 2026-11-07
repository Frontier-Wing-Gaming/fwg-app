//! Server status text (from `src/components/server-status.tsx`). Colours stay in the UI theme.

use std::time::Duration;

use crate::models::{ServerState, ServerStatus};

/// How often the Games/Server screens re-poll `/servers.php` while visible.
pub const POLL_INTERVAL: Duration = Duration::from_secs(60);

impl ServerState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Online => "Online",
            Self::Offline => "Offline",
            Self::Unknown => "Checking",
            Self::Unlisted => "Address in Discord",
            Self::Community => "Community group",
            Self::Hosted => "Hosted by FWG",
            Self::Inactive => "Offline for now",
        }
    }
}

/// The secondary line under a status badge.
pub fn server_detail(status: Option<&ServerStatus>) -> String {
    let Some(status) = status else {
        return "Status updates every minute".into();
    };
    match status.status {
        ServerState::Online => {
            let max = status
                .max_players
                .map_or_else(|| "?".to_owned(), |m| m.to_string());
            format!("{} / {max} players", status.players.unwrap_or(0))
        }
        ServerState::Offline => "Check Discord for maintenance notes".into(),
        ServerState::Community => "Official servers · we play together".into(),
        ServerState::Unlisted => "Connect details are posted in Discord".into(),
        ServerState::Hosted => "Live status is not available for this server yet".into(),
        ServerState::Inactive => "This server is switched off for now".into(),
        ServerState::Unknown => "Status updates every minute".into(),
    }
}

/// Screen-reader text for a status badge.
pub fn accessibility_label(status: Option<&ServerStatus>) -> String {
    let state = status.map_or(ServerState::Unknown, |s| s.status);
    format!(
        "Server status: {}. {}",
        state.label(),
        server_detail(status)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details() {
        assert_eq!(server_detail(None), "Status updates every minute");
        let online = ServerStatus {
            status: ServerState::Online,
            players: Some(3),
            ..Default::default()
        };
        assert_eq!(server_detail(Some(&online)), "3 / ? players");
        assert_eq!(
            accessibility_label(Some(&online)),
            "Server status: Online. 3 / ? players"
        );
    }
}
