use flowforge_core::hook::{self, TeammateIdleInput};
use flowforge_core::{AgentSessionStatus, FlowForgeConfig, Result, TeamMemberStatus};
use flowforge_memory::MemoryDb;
use flowforge_tmux::TmuxStateManager;

pub fn run() -> Result<()> {
    let v = hook::parse_stdin_value()?;
    let input = TeammateIdleInput::from_value(&v)?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    let teammate_name = input.teammate_name.as_deref().unwrap_or("unknown");

    // Update tmux state
    let state_mgr = TmuxStateManager::new(FlowForgeConfig::tmux_state_path());
    let _ = state_mgr.update_member_status(teammate_name, TeamMemberStatus::Idle, None);
    let _ = state_mgr.add_event(format!("{} went idle", teammate_name));

    // Persist idle status to DB
    let db_path = config.db_path();
    if db_path.exists() {
        if let Ok(db) = MemoryDb::open(&db_path) {
            let _ = db.update_agent_session_status(teammate_name, AgentSessionStatus::Idle);

            // Detect and handle stale work items
            if config.work_tracking.work_stealing.enabled {
                let _ = flowforge_core::work_tracking::detect_stale(&db, &config.work_tracking);
            }
        }
    }

    Ok(())
}
