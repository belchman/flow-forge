use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Hook error: {0}")]
    Hook(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Memory error: {0}")]
    Memory(String),

    #[error("Database error{}: {message}", if *.transient { " (transient)" } else { "" })]
    Database { message: String, transient: bool },

    #[error("Tmux error: {0}")]
    Tmux(String),

    #[error("MCP error: {0}")]
    Mcp(String),

    #[error("Conversation error: {0}")]
    Conversation(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Guidance error: {0}")]
    Guidance(String),

    #[error("Plugin error: {0}")]
    Plugin(String),
}

impl Error {
    /// Returns `true` for transient errors that may succeed on retry (e.g. SQLITE_BUSY).
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Error::Database {
                transient: true,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_transient_database_error() {
        let err = Error::Database {
            message: "database is locked".to_string(),
            transient: true,
        };
        assert!(err.is_transient());
        assert!(err.to_string().contains("(transient)"));
    }

    #[test]
    fn test_is_not_transient_database_error() {
        let err = Error::Database {
            message: "no such table: foo".to_string(),
            transient: false,
        };
        assert!(!err.is_transient());
        assert!(!err.to_string().contains("(transient)"));
    }

    #[test]
    fn test_is_transient_other_variants() {
        assert!(!Error::Database {
            message: "test".to_string(),
            transient: false,
        }
        .is_transient());
        assert!(!Error::Config("test".to_string()).is_transient());
        assert!(!Error::Memory("test".to_string()).is_transient());
    }

    #[test]
    fn test_database_error_display() {
        let transient = Error::Database {
            message: "database is locked".to_string(),
            transient: true,
        };
        assert_eq!(
            transient.to_string(),
            "Database error (transient): database is locked"
        );

        let permanent = Error::Database {
            message: "disk I/O error".to_string(),
            transient: false,
        };
        assert_eq!(permanent.to_string(), "Database error: disk I/O error");
    }
}
