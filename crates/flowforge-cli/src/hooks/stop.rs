use chrono::Utc;
use flowforge_core::hook::{self, StopInput};
use flowforge_core::{FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;

pub fn run() -> Result<()> {
    let v = hook::parse_stdin_value()?;
    let _input = StopInput::from_value(&v)?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    let db_path = config.db_path();
    if !db_path.exists() {
        return Ok(());
    }

    let db = MemoryDb::open(&db_path)?;

    // End current session if active
    if let Ok(Some(session)) = db.get_current_session() {
        db.end_session(&session.id, Utc::now())?;
    }

    Ok(())
}
