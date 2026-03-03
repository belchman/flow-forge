use colored::Colorize;
use flowforge_core::{FlowForgeConfig, MailboxMessage, Result};
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

pub fn send(work_item: &str, from: &str, to: Option<&str>, message: &str) -> Result<()> {
    let db = open_db()?;

    // Resolve from_session_id from current session
    let from_session_id = db
        .get_current_session()?
        .map(|s| s.id)
        .unwrap_or_else(|| "cli".to_string());

    let msg = MailboxMessage {
        id: 0,
        work_item_id: work_item.to_string(),
        from_session_id,
        from_agent_name: from.to_string(),
        to_session_id: None,
        to_agent_name: to.map(|s| s.to_string()),
        message_type: "text".to_string(),
        content: message.to_string(),
        priority: 2,
        read_at: None,
        created_at: chrono::Utc::now(),
        metadata: None,
    };

    let id = db.send_mailbox_message(&msg)?;
    let target = to.unwrap_or("all agents");
    println!(
        "{} Message #{} sent to {} on work item {}",
        "OK".green(),
        id,
        target,
        &work_item[..work_item.len().min(8)]
    );
    Ok(())
}

pub fn read(session_id: Option<&str>) -> Result<()> {
    let db = open_db()?;
    let sid = match session_id {
        Some(id) => id.to_string(),
        None => match db.get_current_session()? {
            Some(s) => s.id,
            None => {
                println!("{}", "No active session".yellow());
                return Ok(());
            }
        },
    };

    let unread = db.get_unread_messages(&sid)?;
    if unread.is_empty() {
        println!("No unread messages");
        return Ok(());
    }

    println!("{} {} unread messages:", "Mailbox".bold(), unread.len());
    println!("{}", "─".repeat(60));

    for msg in &unread {
        let target = msg.to_agent_name.as_deref().unwrap_or("broadcast");
        println!(
            "[#{}] From {} -> {} ({}): {}",
            msg.id,
            msg.from_agent_name.cyan(),
            target,
            msg.message_type,
            msg.content
        );
    }

    let count = db.mark_messages_read(&sid)?;
    println!("\n{} {} messages marked as read", "OK".green(), count);
    Ok(())
}

pub fn history(work_item_id: &str, limit: usize) -> Result<()> {
    let db = open_db()?;
    let messages = db.get_mailbox_history(work_item_id, limit)?;

    if messages.is_empty() {
        println!("No messages for this work item");
        return Ok(());
    }

    println!(
        "{} for work item {}",
        "Mailbox History".bold(),
        &work_item_id[..work_item_id.len().min(8)]
    );
    println!("{}", "─".repeat(60));

    for msg in &messages {
        let read_status = if msg.read_at.is_some() {
            "read".dimmed().to_string()
        } else {
            "unread".yellow().to_string()
        };
        println!(
            "[#{}] {} -> {}: {} [{}]",
            msg.id,
            msg.from_agent_name.cyan(),
            msg.to_agent_name.as_deref().unwrap_or("all"),
            msg.content,
            read_status,
        );
    }
    Ok(())
}

pub fn agents(work_item_id: &str) -> Result<()> {
    let db = open_db()?;
    let agent_sessions = db.get_agents_on_work_item(work_item_id)?;

    if agent_sessions.is_empty() {
        println!("No agents on this work item");
        return Ok(());
    }

    println!(
        "{} on work item {}",
        "Agents".bold(),
        &work_item_id[..work_item_id.len().min(8)]
    );
    println!("{}", "─".repeat(50));

    for a in &agent_sessions {
        println!("  {} ({}) - {}", a.agent_id, a.agent_type, a.status);
    }
    Ok(())
}
