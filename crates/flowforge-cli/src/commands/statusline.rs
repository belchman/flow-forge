use colored::Colorize;
use flowforge_core::{AgentSessionStatus, FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;

const SEP: &str = "\u{2502}"; // │

#[allow(clippy::print_literal, clippy::format_in_format_args)]
pub fn run() -> Result<()> {
    // Read stdin (Claude Code pipes JSON context)
    let stdin_data: serde_json::Value = {
        let mut buf = String::new();
        match std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf) {
            Ok(_) if !buf.trim().is_empty() => {
                serde_json::from_str(&buf).unwrap_or(serde_json::json!({}))
            }
            _ => serde_json::json!({}),
        }
    };

    let model = stdin_data
        .get("model")
        .and_then(|v| {
            // Handle both string and object formats
            v.as_str().or_else(|| {
                v.get("display_name")
                    .and_then(|d| d.as_str())
                    .or_else(|| v.get("id").and_then(|id| id.as_str()))
            })
        })
        .unwrap_or("");

    let project_name = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    let ctx_remaining: Option<u32> = stdin_data
        .get("context_window")
        .and_then(|cw| cw.get("remaining_percentage"))
        .and_then(|v| v.as_f64())
        .map(|f| f as u32);

    let session_name: Option<&str> = stdin_data
        .get("session_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());

    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path()).ok();
    let db = config
        .as_ref()
        .and_then(|c| MemoryDb::open(&c.db_path()).ok());

    // Get git info
    let git_branch = std::process::Command::new("git")
        .args(["branch", "--show-current", "--no-color"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .filter(|s| !s.is_empty());

    let git_status = std::process::Command::new("git")
        .args(["status", "--porcelain", "--no-renames"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let (staged, modified, untracked) = parse_git_porcelain(&git_status);

    let mut lines = Vec::new();

    // ══════════════════════════════════════════════════════════════
    // LINE 1: Header — project + git + model + context + duration
    // ══════════════════════════════════════════════════════════════
    let display_name = session_name.unwrap_or(&project_name);
    let mut header_parts: Vec<String> =
        vec![display_name.bold().bright_magenta().to_string()];

    // Git branch + changes
    if let Some(ref branch) = git_branch {
        let mut git_str = branch.bright_blue().to_string();
        let mut changes = Vec::new();
        if staged > 0 {
            changes.push(format!("+{}", staged).bright_green().to_string());
        }
        if modified > 0 {
            changes.push(format!("~{}", modified).bright_yellow().to_string());
        }
        if untracked > 0 {
            changes.push(format!("?{}", untracked).dimmed().to_string());
        }
        if !changes.is_empty() {
            git_str = format!("{} {}", git_str, changes.join(""));
        }
        header_parts.push(git_str);
    }

    // Model
    if !model.is_empty() {
        header_parts.push(shorten_model(model).bright_magenta().to_string());
    }

    // Context remaining
    if let Some(remaining) = ctx_remaining {
        let ctx_used = 100u32.saturating_sub(remaining);
        let ctx_str = format!("ctx {}%", ctx_used);
        let colored = if ctx_used < 50 {
            ctx_str.bright_green()
        } else if ctx_used < 70 {
            ctx_str.bright_cyan()
        } else if ctx_used < 85 {
            ctx_str.bright_yellow()
        } else {
            ctx_str.bright_red()
        };
        header_parts.push(colored.to_string());
    }

    // Session duration
    if let Some(ref db) = db {
        if let Ok(Some(session)) = db.get_current_session() {
            let secs = chrono::Utc::now()
                .signed_duration_since(session.started_at)
                .num_seconds();
            header_parts.push(format_duration(secs).cyan().to_string());
        }
    }

    let sep = format!("  {}  ", SEP.dimmed());
    lines.push(header_parts.join(&sep));

    // ══════════════════════════════════════════════════════════════
    // LINE 2: Intelligence + Session metrics
    // ══════════════════════════════════════════════════════════════
    if let Some(ref db) = db {
        let long = db.count_patterns_long().unwrap_or(0);
        let mut parts: Vec<String> = Vec::new();

        // Proven patterns
        let long_str = if long > 10 {
            format!("{}", long).bright_green().to_string()
        } else if long > 0 {
            format!("{}", long).yellow().to_string()
        } else {
            "0".dimmed().to_string()
        };
        parts.push(format!("{} proven", long_str));

        // Trajectory success rate
        let traj_rate = db.recent_trajectory_success_rate(20).unwrap_or(0.0);
        let traj_pct = (traj_rate * 100.0) as u32;
        let traj_str = format!("traj {}%", traj_pct);
        parts.push(color_by_ratio(traj_rate, &traj_str));

        // Routing accuracy (only if enough data)
        let (routing_hits, routing_total) = db.routing_accuracy_stats().unwrap_or((0, 0));
        if routing_total > 2 {
            let route_rate = routing_hits as f64 / routing_total as f64;
            let route_pct = (route_rate * 100.0) as u32;
            let route_str = format!("route {}%", route_pct);
            parts.push(color_by_ratio(route_rate, &route_str));
        }

        // Session-dependent metrics
        if let Ok(Some(session)) = db.get_current_session() {
            let sid = &session.id;

            // Separator
            parts.push(SEP.dimmed().to_string());

            // Trust score
            if let Ok(Some(trust)) = db.get_trust_score(sid) {
                let trust_pct = (trust.score * 100.0) as u32;
                let trust_str = format!("trust {}%", trust_pct);
                let mut detail = color_by_trust(trust.score, &trust_str);
                if trust.denials > 0 {
                    detail = format!("{} {}", detail, format!("{}deny", trust.denials).red());
                }
                parts.push(detail);
            }

            // Session error count
            let errs = db.count_session_failures(sid).unwrap_or(0);
            let err_str = format!("{} errs", errs);
            if errs == 0 {
                parts.push(err_str.green().to_string());
            } else {
                parts.push(err_str.red().to_string());
            }

            // Checkpoint count
            let cps = db.list_checkpoints(sid).map(|c| c.len()).unwrap_or(0);
            parts.push(format!("{} cp", cps).dimmed().to_string());

            // Separator
            parts.push(SEP.dimmed().to_string());

            // Session activity
            let mut activity = Vec::new();
            if session.edits > 0 {
                activity.push(format!("{} edits", session.edits));
            }
            if session.commands > 0 {
                activity.push(format!("{} cmds", session.commands));
            }
            if !activity.is_empty() {
                parts.push(activity.join(" ").dimmed().to_string());
            }
        }

        lines.push(parts.join("  "));
    }

    // ══════════════════════════════════════════════════════════════
    // LINE 3: Work + Agents + Warnings (only if content exists)
    // ══════════════════════════════════════════════════════════════
    if let Some(ref db) = db {
        let mut line3_parts: Vec<String> = Vec::new();

        // Agents (only when active)
        let current_session_id = db.get_current_session().ok().flatten().map(|s| s.id);
        let (active_count, idle_count, agent_names) =
            get_agent_summary(db, current_session_id.as_deref());
        let total_agents = active_count + idle_count;
        if total_agents > 0 {
            line3_parts.push(format!(
                "{} agents ({})",
                format!("{}", total_agents).bright_green(),
                if !agent_names.is_empty() {
                    agent_names.join(" ")
                } else {
                    "--".dimmed().to_string()
                }
            ));
        }

        // Unread mail
        if let Some(ref sid) = current_session_id {
            if let Ok(unread) = db.get_unread_messages(sid) {
                if !unread.is_empty() {
                    line3_parts
                        .push(format!("{} mail", unread.len()).bright_yellow().to_string());
                }
            }
        }

        // Work items
        let wip =
            db.count_work_items_by_status(flowforge_core::WorkStatus::InProgress).unwrap_or(0);
        let pending =
            db.count_work_items_by_status(flowforge_core::WorkStatus::Pending).unwrap_or(0);
        if wip > 0 || pending > 0 {
            let mut w = Vec::new();
            if wip > 0 {
                w.push(format!("{} work active", wip));
            }
            if pending > 0 {
                w.push(format!("{} work pending", pending));
            }
            line3_parts.push(w.join("  ").bright_blue().to_string());
        }

        // Warnings
        let mut warn_parts = Vec::new();
        if let Ok(stealable) = db.get_stealable_items(5) {
            if !stealable.is_empty() {
                warn_parts.push(format!("{} stale", stealable.len()).yellow().to_string());
            }
        }
        let log_path = FlowForgeConfig::project_dir().join("hook-errors.log");
        if log_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&log_path) {
                let err_count = content.lines().filter(|l| !l.trim().is_empty()).count();
                if err_count > 0 {
                    warn_parts.push(format!("{} hook-err", err_count).red().to_string());
                }
            }
        }
        if !warn_parts.is_empty() {
            line3_parts.push(format!("!! {}", warn_parts.join(" ")));
        }

        // Only print line 3 if there's content
        if !line3_parts.is_empty() {
            lines.push(line3_parts.join("  {}  ").replace("{}", &SEP.dimmed().to_string()));
        }
    }

    // Print multi-line dashboard
    println!("{}", lines.join("\n"));

    Ok(())
}

/// Print the legend explaining all statusline symbols.
#[allow(clippy::print_literal)]
pub fn print_legend() -> Result<()> {
    println!("{}", "FlowForge Dashboard Legend".bold().cyan());
    println!();

    println!("{}", "HEADER LINE".bold());
    println!("  project      Project or session name");
    println!("  branch       Git branch with +staged ~modified ?untracked");
    println!("  op4.6        Model name (Opus 4.6, Sonnet 4.6, etc.)");
    println!("  ctx 23%      Context window usage (green<50 cyan<70 yellow<85 red)");
    println!("  5m           Session duration");
    println!();

    println!("{}", "INTELLIGENCE + SESSION LINE".bold());
    println!("  N proven     Long-term validated patterns");
    println!("  traj N%      Trajectory success rate (last 20 judged)");
    println!("  route N%     Routing accuracy (shown when >2 data points)");
    println!("  trust N%     Guidance trust score (green>=80 yellow>=50 red)");
    println!("  N errs       Distinct tool failures this session (green=0 red>0)");
    println!("  N cp         Checkpoint count (rollback safety points)");
    println!("  N edits      File edits this session");
    println!("  N cmds       Commands run this session");
    println!();

    println!("{}", "WORK + AGENTS LINE (shown when applicable)".bold());
    println!("  N agents     Active/idle agents with shortened names");
    println!("  N mail       Unread co-agent messages");
    println!("  N work active   In-progress work items");
    println!("  N work pending  Pending work items");
    println!("  !! stale     Stealable work items (warning)");
    println!(
        "  !! {}    Hook error log not empty (warning)",
        "hook-err".red()
    );
    println!();

    println!(
        "{}",
        "Run `flowforge statusline` to see the live dashboard.".dimmed()
    );

    Ok(())
}

// ── Helpers ──

fn format_duration(secs: i64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

fn color_by_ratio(ratio: f64, text: &str) -> String {
    if ratio >= 0.9 {
        text.green().to_string()
    } else if ratio >= 0.7 {
        text.yellow().to_string()
    } else {
        text.red().to_string()
    }
}

fn color_by_trust(score: f64, text: &str) -> String {
    if score >= 0.8 {
        text.green().to_string()
    } else if score >= 0.5 {
        text.yellow().to_string()
    } else {
        text.red().to_string()
    }
}

/// Parse git status --porcelain output into (staged, modified, untracked) counts
fn parse_git_porcelain(output: &str) -> (u32, u32, u32) {
    let (mut staged, mut modified, mut untracked) = (0, 0, 0);
    for line in output.lines() {
        if line.len() < 2 {
            continue;
        }
        let bytes = line.as_bytes();
        let x = bytes[0] as char;
        let y = bytes[1] as char;
        if x == '?' && y == '?' {
            untracked += 1;
        } else {
            if x != ' ' && x != '?' {
                staged += 1;
            }
            if y != ' ' && y != '?' {
                modified += 1;
            }
        }
    }
    (staged, modified, untracked)
}

/// Get agent summary: (active_count, idle_count, formatted_names)
/// When parent_session_id is provided, shows agents from that session and
/// their sub-agents (team lead children) via recursive query.
fn get_agent_summary(
    db: &MemoryDb,
    parent_session_id: Option<&str>,
) -> (usize, usize, Vec<String>) {
    // Clean up orphaned agent sessions before display
    let _ = db.cleanup_orphaned_agent_sessions();

    let agents = if let Some(sid) = parent_session_id {
        db.get_agent_sessions_recursive(sid)
            .unwrap_or_default()
            .into_iter()
            .filter(|a| a.ended_at.is_none())
            .collect()
    } else {
        db.get_active_agent_sessions().unwrap_or_default()
    };
    let active: Vec<_> = agents
        .iter()
        .filter(|a| a.status == AgentSessionStatus::Active)
        .collect();
    let idle: Vec<_> = agents
        .iter()
        .filter(|a| a.status == AgentSessionStatus::Idle)
        .collect();

    let mut names = Vec::new();
    for a in &active {
        let name = shorten_agent_name(&a.agent_type);
        names.push(name.bold().green().to_string());
    }
    for a in &idle {
        let name = shorten_agent_name(&a.agent_type);
        names.push(name.dimmed().to_string());
    }

    (active.len(), idle.len(), names)
}

/// Shorten agent type names for compact display
fn shorten_model(model: &str) -> String {
    match model {
        m if m.contains("opus-4-6") || m.contains("opus-4.6") => "op4.6".to_string(),
        m if m.contains("sonnet-4-6") || m.contains("sonnet-4.6") => "sn4.6".to_string(),
        m if m.contains("haiku-4-5") || m.contains("haiku-4.5") => "hk4.5".to_string(),
        m if m.contains("opus-4-5") || m.contains("opus-4.5") => "op4.5".to_string(),
        m if m.contains("sonnet-4-5") || m.contains("sonnet-4.5") => "sn4.5".to_string(),
        m if m.contains("opus-4-0") || m.contains("opus-4") => "op4".to_string(),
        m if m.contains("sonnet-4-0") || m.contains("sonnet-4") => "sn4".to_string(),
        m if m.contains("sonnet-3-5") || m.contains("sonnet-3.5") => "sn3.5".to_string(),
        m if m.contains("haiku-3-5") || m.contains("haiku-3.5") => "hk3.5".to_string(),
        m => m.to_string(),
    }
}

fn shorten_agent_name(agent_type: &str) -> String {
    match agent_type {
        "general-purpose" | "general" => "gen".to_string(),
        "Explore" | "explore" => "exp".to_string(),
        "Plan" | "plan" => "pln".to_string(),
        "code-simplifier" => "sim".to_string(),
        "claude-code-guide" => "guide".to_string(),
        "statusline-setup" => "sline".to_string(),
        "test-runner" => "test".to_string(),
        t if t.len() > 6 => t[..6].to_string(),
        t => t.to_string(),
    }
}
