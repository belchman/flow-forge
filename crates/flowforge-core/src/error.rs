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

    #[error("SQLite error: {0}")]
    Sqlite(String),

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
