use colored::Colorize;
use flowforge_core::{
    AgentSessionStatus, Checkpoint, FlowForgeConfig, Result, SessionFork, SessionInfo,
};
use flowforge_memory::MemoryDb;

fn open_db() -> Result<MemoryDb> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db_path = config.db_path();
    if !db_path.exists() {
        return Err(flowforge_core::Error::Config(
            "FlowForge not initialized. Run `flowforge init --project` first.".to_string(),
        ));
    }
    MemoryDb::open(&db_path)
}

pub fn current() -> Result<()> {
    let db = open_db()?;
    match db.get_current_session()? {
        Some(session) => {
            println!("{}", "Current Session".bold());
            println!("ID:       {}", session.id.cyan());
            println!("Started:  {}", session.started_at);
            println!("CWD:      {}", session.cwd);
            println!("Edits:    {}", session.edits);
            println!("Commands: {}", session.commands);
        }
        None => {
            println!("{}", "No active session".yellow());
        }
    }
    Ok(())
}

pub fn list(limit: usize) -> Result<()> {
    let db = open_db()?;
    let sessions = db.list_sessions(limit)?;
    if sessions.is_empty() {
        println!("No sessions recorded");
        return Ok(());
    }

    println!(
        "{:<10} {:<20} {:<6} {:<6} Status",
        "ID", "Started", "Edits", "Cmds"
    );
    println!("{}", "─".repeat(60));

    for session in &sessions {
        let status = if session.ended_at.is_some() {
            "ended".dimmed().to_string()
        } else {
            "active".green().to_string()
        };

        println!(
            "{:<10} {:<20} {:<6} {:<6} {}",
            &session.id[..8],
            session.started_at.format("%Y-%m-%d %H:%M"),
            session.edits,
            session.commands,
            status,
        );
    }
    Ok(())
}

pub fn metrics() -> Result<()> {
    let db = open_db()?;
    let sessions = db.list_sessions(100)?;

    let total_sessions = sessions.len();
    let total_edits: u64 = sessions.iter().map(|s| s.edits).sum();
    let total_commands: u64 = sessions.iter().map(|s| s.commands).sum();
    let active = sessions.iter().filter(|s| s.ended_at.is_none()).count();

    println!("{}", "Session Metrics".bold());
    println!("Total sessions: {}", total_sessions);
    println!("Active:         {}", active);
    println!("Total edits:    {}", total_edits);
    println!("Total commands: {}", total_commands);

    if total_sessions > 0 {
        println!(
            "Avg edits/session:    {:.1}",
            total_edits as f64 / total_sessions as f64
        );
        println!(
            "Avg commands/session: {:.1}",
            total_commands as f64 / total_sessions as f64
        );
    }

    Ok(())
}

pub fn agents(session_id: Option<&str>) -> Result<()> {
    let db = open_db()?;

    let parent_id = match session_id {
        Some(id) => id.to_string(),
        None => match db.get_current_session()? {
            Some(s) => s.id,
            None => {
                println!("{}", "No active session".yellow());
                return Ok(());
            }
        },
    };

    let agent_sessions = db.get_agent_sessions(&parent_id)?;
    if agent_sessions.is_empty() {
        println!("No agent sessions for this session");
        return Ok(());
    }

    println!(
        "{:<10} {:<14} {:<12} {:<20} {:<10} {:<6} {:<6}",
        "ID", "Agent Type", "Status", "Started", "Duration", "Edits", "Cmds"
    );
    println!("{}", "─".repeat(80));

    for a in &agent_sessions {
        let status_str = match a.status {
            AgentSessionStatus::Active => "active".green().to_string(),
            AgentSessionStatus::Idle => "idle".yellow().to_string(),
            AgentSessionStatus::Completed => "completed".dimmed().to_string(),
            AgentSessionStatus::Error => "error".red().to_string(),
        };

        let duration = if let Some(end) = a.ended_at {
            let secs = (end - a.started_at).num_seconds();
            format!("{}s", secs)
        } else {
            let secs = (chrono::Utc::now() - a.started_at).num_seconds();
            format!("{}s+", secs)
        };

        let id_short = if a.id.len() >= 8 { &a.id[..8] } else { &a.id };

        println!(
            "{:<10} {:<14} {:<12} {:<20} {:<10} {:<6} {:<6}",
            id_short,
            a.agent_type,
            status_str,
            a.started_at.format("%Y-%m-%d %H:%M"),
            duration,
            a.edits,
            a.commands,
        );
    }
    Ok(())
}

fn resolve_session_id(db: &MemoryDb, session_id: Option<&str>) -> Result<String> {
    match session_id {
        Some(id) => Ok(id.to_string()),
        None => match db.get_current_session()? {
            Some(s) => Ok(s.id),
            None => Err(flowforge_core::Error::NotFound(
                "No active session".to_string(),
            )),
        },
    }
}

pub fn history(session_id: Option<&str>, limit: usize, offset: usize) -> Result<()> {
    let db = open_db()?;
    let sid = resolve_session_id(&db, session_id)?;
    let total = db.get_conversation_message_count(&sid)?;
    let messages = db.get_conversation_messages(&sid, limit, offset)?;

    if messages.is_empty() {
        println!(
            "{}",
            "No conversation messages. Ingest a transcript first.".yellow()
        );
        return Ok(());
    }

    println!(
        "{} (showing {}-{} of {})",
        "Conversation History".bold(),
        offset,
        offset + messages.len(),
        total
    );
    println!("{}", "─".repeat(80));

    for msg in &messages {
        let role_colored = match msg.role.as_str() {
            "user" => msg.role.green().to_string(),
            "assistant" => msg.role.cyan().to_string(),
            "system" => msg.role.yellow().to_string(),
            _ => msg.role.dimmed().to_string(),
        };

        let content_preview: String = msg.content.chars().take(120).collect();
        println!(
            "[{}] {} ({}): {}",
            msg.message_index, role_colored, msg.message_type, content_preview
        );
    }
    Ok(())
}

pub fn ingest(path: &str, session_id: Option<&str>) -> Result<()> {
    let db = open_db()?;
    let sid = match session_id {
        Some(id) => id.to_string(),
        None => {
            // Use current session or generate from filename
            match db.get_current_session()? {
                Some(s) => s.id,
                None => {
                    // Derive from path
                    let p = std::path::Path::new(path);
                    p.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown")
                        .to_string()
                }
            }
        }
    };

    let count = db.ingest_transcript(&sid, path)?;
    println!(
        "{} Ingested {} messages for session {}",
        "OK".green(),
        count,
        &sid[..sid.len().min(8)]
    );
    Ok(())
}

pub fn checkpoint(name: &str, session_id: Option<&str>, description: Option<&str>) -> Result<()> {
    let db = open_db()?;
    let sid = resolve_session_id(&db, session_id)?;
    let message_index = db.get_latest_message_index(&sid)?;

    // Optionally capture git ref
    let git_ref = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        });

    let cp = Checkpoint {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: sid.clone(),
        name: name.to_string(),
        message_index,
        description: description.map(|s| s.to_string()),
        git_ref,
        created_at: chrono::Utc::now(),
        metadata: None,
    };

    db.create_checkpoint(&cp)?;
    println!(
        "{} Checkpoint '{}' created at message index {} (session {})",
        "OK".green(),
        name,
        message_index,
        &sid[..sid.len().min(8)]
    );
    Ok(())
}

pub fn checkpoints(session_id: Option<&str>) -> Result<()> {
    let db = open_db()?;
    let sid = resolve_session_id(&db, session_id)?;
    let cps = db.list_checkpoints(&sid)?;

    if cps.is_empty() {
        println!("No checkpoints for this session");
        return Ok(());
    }

    println!(
        "{:<10} {:<20} {:<8} {:<12} Created",
        "ID", "Name", "Index", "Git Ref"
    );
    println!("{}", "─".repeat(70));

    for cp in &cps {
        println!(
            "{:<10} {:<20} {:<8} {:<12} {}",
            &cp.id[..cp.id.len().min(8)],
            cp.name,
            cp.message_index,
            cp.git_ref
                .as_deref()
                .unwrap_or("-")
                .chars()
                .take(10)
                .collect::<String>(),
            cp.created_at.format("%Y-%m-%d %H:%M"),
        );
    }
    Ok(())
}

pub fn fork(
    session_id: Option<&str>,
    checkpoint_name: Option<&str>,
    at_index: Option<u32>,
    reason: Option<&str>,
) -> Result<()> {
    let db = open_db()?;
    let sid = resolve_session_id(&db, session_id)?;

    // Determine fork point
    let (fork_index, checkpoint_id) = if let Some(cp_name) = checkpoint_name {
        let cp = db.get_checkpoint_by_name(&sid, cp_name)?.ok_or_else(|| {
            flowforge_core::Error::NotFound(format!("Checkpoint '{cp_name}' not found"))
        })?;
        (cp.message_index, Some(cp.id))
    } else if let Some(idx) = at_index {
        (idx, None)
    } else {
        // Fork at latest
        let latest = db.get_latest_message_index(&sid)?;
        (latest.saturating_sub(1), None)
    };

    // Create new session for the fork
    let new_session_id = uuid::Uuid::new_v4().to_string();
    let new_session = SessionInfo {
        id: new_session_id.clone(),
        started_at: chrono::Utc::now(),
        ended_at: None,
        cwd: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string()),
        edits: 0,
        commands: 0,
        summary: Some(format!(
            "Forked from {} at index {}",
            &sid[..sid.len().min(8)],
            fork_index
        )),
        transcript_path: None,
    };
    db.create_session(&new_session)?;

    // Copy conversation messages
    let copied = db.fork_conversation(&sid, &new_session_id, fork_index)?;

    // Record fork
    let fork_record = SessionFork {
        id: uuid::Uuid::new_v4().to_string(),
        source_session_id: sid.clone(),
        target_session_id: new_session_id.clone(),
        fork_message_index: fork_index,
        checkpoint_id,
        reason: reason.map(|s| s.to_string()),
        created_at: chrono::Utc::now(),
    };
    db.create_session_fork(&fork_record)?;

    println!(
        "{} Forked session {} -> {} at index {} ({} messages copied)",
        "OK".green(),
        &sid[..sid.len().min(8)],
        &new_session_id[..8],
        fork_index,
        copied
    );
    Ok(())
}

pub fn forks(session_id: Option<&str>) -> Result<()> {
    let db = open_db()?;
    let sid = resolve_session_id(&db, session_id)?;
    let fork_list = db.get_session_forks(&sid)?;

    if fork_list.is_empty() {
        println!("No forks for this session");
        return Ok(());
    }

    println!(
        "{:<10} {:<10} {:<10} {:<8} Created",
        "ID", "Source", "Target", "Index"
    );
    println!("{}", "─".repeat(55));

    for f in &fork_list {
        println!(
            "{:<10} {:<10} {:<10} {:<8} {}",
            &f.id[..f.id.len().min(8)],
            &f.source_session_id[..f.source_session_id.len().min(8)],
            &f.target_session_id[..f.target_session_id.len().min(8)],
            f.fork_message_index,
            f.created_at.format("%Y-%m-%d %H:%M"),
        );
    }
    Ok(())
}
