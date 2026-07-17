use serde::Serialize;

/// Application-wide error type. Serializes to a plain string so the frontend
/// receives a readable message from a rejected `invoke`.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Msg(String),

    #[error("бинарник не найден: {0}")]
    BinaryMissing(String),

    #[error("процесс уже запущен")]
    AlreadyRunning,

    #[error("процесс не запущен")]
    NotRunning,

    #[error("некорректный адрес: {0}")]
    InvalidAddress(String),

    #[error("сеть недоступна: не удалось подобрать рабочую стратегию")]
    NoWorkingStrategy,

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("сеть: {0}")]
    Http(String),
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        AppError::Http(e.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Msg(format!("sqlite: {e}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Msg(e.to_string())
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Msg(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Msg(s.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
