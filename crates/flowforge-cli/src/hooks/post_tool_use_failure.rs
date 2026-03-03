use flowforge_core::hook::{self, PostToolUseFailureInput};
use flowforge_core::{FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;
use sha2::{Digest, Sha256};

pub fn run() -> Result<()> {
    let v = hook::parse_stdin_value()?;
    let input = PostToolUseFailureInput::from_value(&v)?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    // Record failed tool use for error pattern tracking
    if config.hooks.learning {
        let db_path = config.db_path();
        if db_path.exists() {
            if let Ok(db) = MemoryDb::open(&db_path) {
                let error_msg = input.error.as_deref().unwrap_or("unknown error");
                let pattern = format!(
                    "tool_failure:{} - {}",
                    input.tool_name,
                    &error_msg[..error_msg.len().min(100)]
                );
                let store = flowforge_memory::PatternStore::new(&db, &config.patterns);
                let _ = store.store_short_term(&pattern, "error_pattern");
            }
        }
    }

    // Record trajectory failure step
    let db_path = config.db_path();
    if db_path.exists() {
        if let Ok(db) = MemoryDb::open(&db_path) {
            if let Ok(Some(session)) = db.get_current_session() {
                if let Ok(Some(trajectory)) = db.get_active_trajectory(&session.id) {
                    let input_str = serde_json::to_string(&input.tool_input).unwrap_or_default();
                    let input_hash = format!("{:x}", Sha256::digest(input_str.as_bytes()));
                    let _ = db.record_trajectory_step(
                        &trajectory.id,
                        &input.tool_name,
                        Some(&input_hash),
                        flowforge_core::trajectory::StepOutcome::Failure,
                        None,
                    );
                }
            }
        }
    }

    Ok(())
}
