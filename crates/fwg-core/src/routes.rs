//! Screens a push notification may open (the allow-list in the Expo `_layout.tsx`).

/// A notification's `data.route` is only followed if it is one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Route {
    News,
    Streams,
    Gallery,
    Games,
    About,
    More,
}

impl Route {
    pub const ALL: [Route; 6] = [
        Self::News,
        Self::Streams,
        Self::Gallery,
        Self::Games,
        Self::About,
        Self::More,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::News => "/news",
            Self::Streams => "/streams",
            Self::Gallery => "/gallery",
            Self::Games => "/games",
            Self::About => "/about",
            Self::More => "/more",
        }
    }

    /// Exact match only, so a crafted notification cannot open staff screens.
    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.path() == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allowed_paths_parse() {
        assert_eq!(Route::from_path("/news"), Some(Route::News));
        assert_eq!(Route::from_path("/admin"), None);
        assert_eq!(Route::from_path("/news/../admin"), None);
    }
}
