use flowforge_core::hook::{self, PreCompactInput};
use flowforge_core::{FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;

pub fn run() -> Result<()> {
    let v = hook::parse_stdin_value()?;
    let _input = PreCompactInput::from_value(&v)?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    let db_path = config.db_path();

    let mut guidance = vec![
        "[FlowForge Compaction Guidance]".to_string(),
        "Key context to preserve:".to_string(),
    ];

    if db_path.exists() {
        if let Ok(db) = MemoryDb::open(&db_path) {
            // Include current session stats
            if let Ok(Some(session)) = db.get_current_session() {
                guidance.push(format!(
                    "- Current session: {} edits, {} commands",
                    session.edits, session.commands
                ));
            }

            // Include recent patterns
            if let Ok(patterns) = db.get_top_patterns(5) {
                if !patterns.is_empty() {
                    guidance.push("- Active patterns:".to_string());
                    for p in &patterns {
                        guidance.push(format!(
                            "  - [{}] {} (conf: {:.0}%)",
                            p.category,
                            p.content,
                            p.confidence * 100.0
                        ));
                    }
                }
            }
        }
    }

    guidance
        .push("- Use `flowforge memory search <query>` to recall stored knowledge.".to_string());
    guidance.push("- Use `flowforge session current` to check session state.".to_string());

    let output = flowforge_core::hook::ContextOutput::with_context(guidance.join("\n"));
    hook::write_stdout(&output)?;

    Ok(())
}
