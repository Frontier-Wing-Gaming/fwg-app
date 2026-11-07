use reqwest::StatusCode;

pub const INVALID_RESPONSE: &str =
    "The server returned an invalid response. Please try again after the website API is updated.";

/// Errors from the website API. `Display` gives the message shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Non-2xx response. `message` is the API's `error` field, or "Request failed (code)".
    #[error("{message}")]
    Server { status: StatusCode, message: String },
    /// 2xx response whose body was not a JSON object/array.
    #[error("{INVALID_RESPONSE}")]
    InvalidResponse,
    /// JSON that does not match the expected shape.
    #[error("{INVALID_RESPONSE}")]
    Decode(#[source] serde_json::Error),
    #[error("Unable to reach the server. Check your connection and try again.")]
    Network(#[source] reqwest::Error),
}

impl ApiError {
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Server { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn is_unauthorized(&self) -> bool {
        self.status() == Some(StatusCode::UNAUTHORIZED)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("Unable to save data on this device: {0}")]
    Storage(#[from] std::io::Error),
}
