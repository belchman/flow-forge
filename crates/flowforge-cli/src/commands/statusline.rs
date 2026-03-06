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

    let session_cost: Option<f64> = stdin_data
        .get("cost")
        .and_then(|c| c.get("total_cost_usd"))
        .and_then(|v| v.as_f64());

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

    // Session cost
    if let Some(cost) = session_cost {
        let cost_str = format!("${:.2}", cost);
        let colored = if cost < 1.0 {
            cost_str.green()
        } else if cost < 5.0 {
            cost_str.yellow()
        } else {
            cost_str.bright_red()
        };
        header_parts.push(colored.to_string());
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

            // Hook health: called_ok / total_configured
            let hook_health = compute_hook_health(db, sid);
            if hook_health.total > 0 {
                let h_str = format!("{}/{} hooks", hook_health.ok, hook_health.total);
                if hook_health.ok == hook_health.total {
                    parts.push(h_str.green().to_string());
                } else if hook_health.ok as f64 / hook_health.total as f64 >= 0.8 {
                    parts.push(h_str.yellow().to_string());
                } else {
                    parts.push(h_str.red().to_string());
                }
            }

            // Session activity (with separator only when there's content)
            let mut activity = Vec::new();
            if session.edits > 0 {
                activity.push(format!("{} edits", session.edits));
            }
            if session.commands > 0 {
                activity.push(format!("{} cmds", session.commands));
            }
            if !activity.is_empty() {
                parts.push(SEP.dimmed().to_string());
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

        // Agents: live/total_spawned (names)
        let current_session_id = db.get_current_session().ok().flatten().map(|s| s.id);
        let agents = get_agent_summary(db, current_session_id.as_deref());
        let live = agents.active + agents.idle;
        if agents.total_spawned > 0 {
            let count_str = if live > 0 {
                format!("{}/{} agents", live, agents.total_spawned)
                    .bright_green()
                    .to_string()
            } else {
                format!("0/{} agents", agents.total_spawned)
                    .dimmed()
                    .to_string()
            };
            if !agents.names.is_empty() {
                line3_parts.push(format!("{} ({})", count_str, agents.names.join(" ")));
            } else {
                line3_parts.push(count_str);
            }
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

        // Work items (with title of first active/pending item)
        let wip_items = db
            .list_work_items(&flowforge_core::WorkFilter {
                status: Some(flowforge_core::WorkStatus::InProgress),
                ..Default::default()
            })
            .unwrap_or_default();
        let pending_items = db
            .list_work_items(&flowforge_core::WorkFilter {
                status: Some(flowforge_core::WorkStatus::Pending),
                ..Default::default()
            })
            .unwrap_or_default();
        let wip = wip_items.len();
        let pending = pending_items.len();
        if wip > 0 || pending > 0 {
            // Show title of the most relevant work item
            let lead_item = wip_items.first().or(pending_items.first());
            if let Some(item) = lead_item {
                let title = truncate_str(&item.title, 35);
                line3_parts.push(title.bright_cyan().to_string());
            }
            let mut w = Vec::new();
            if wip > 0 {
                w.push(format!("{} active", wip));
            }
            if pending > 0 {
                w.push(format!("{} pending", pending));
            }
            line3_parts.push(w.join(" ").bright_blue().to_string());
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
    println!("  $1.23        Session cost (green<$1 yellow<$5 red)");
    println!();

    println!("{}", "INTELLIGENCE + SESSION LINE".bold());
    println!("  N proven     Long-term validated patterns");
    println!("  traj N%      Trajectory success rate (last 20 judged)");
    println!("  route N%     Routing accuracy (shown when >2 data points)");
    println!("  trust N%     Guidance trust score (green>=80 yellow>=50 red)");
    println!("  N errs       Distinct tool failures this session (green=0 red>0)");
    println!("  N cp         Checkpoint count (rollback safety points)");
    println!("  N/M hooks    Hooks working / total configured (green=all yellow>=80% red)");
    println!("  N edits      File edits this session");
    println!("  N cmds       Commands run this session");
    println!();

    println!("{}", "WORK + AGENTS LINE (shown when applicable)".bold());
    println!("  N/M agents   Live / total spawned agents (with shortened names)");
    println!("  N mail       Unread co-agent messages");
    println!("  title…       Title of lead work item (truncated)");
    println!("  N active     In-progress work items");
    println!("  N pending    Pending work items");
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

struct AgentSummary {
    active: usize,
    idle: usize,
    total_spawned: usize,
    names: Vec<String>,
}

/// Get agent summary for the session.
/// When parent_session_id is provided, shows agents from that session and
/// their sub-agents (team lead children) via recursive query.
fn get_agent_summary(db: &MemoryDb, parent_session_id: Option<&str>) -> AgentSummary {
    // Clean up orphaned agent sessions before display
    let _ = db.cleanup_orphaned_agent_sessions();

    let all_agents = if let Some(sid) = parent_session_id {
        db.get_agent_sessions_recursive(sid).unwrap_or_default()
    } else {
        db.get_active_agent_sessions().unwrap_or_default()
    };

    let total_spawned = all_agents.len();
    let live: Vec<_> = all_agents
        .into_iter()
        .filter(|a| a.ended_at.is_none())
        .collect();

    let active: Vec<_> = live
        .iter()
        .filter(|a| a.status == AgentSessionStatus::Active)
        .collect();
    let idle: Vec<_> = live
        .iter()
        .filter(|a| a.status == AgentSessionStatus::Idle)
        .collect();

    let mut names = Vec::new();
    for a in &active {
        names.push(shorten_agent_name(&a.agent_type).bold().green().to_string());
    }
    for a in &idle {
        names.push(shorten_agent_name(&a.agent_type).dimmed().to_string());
    }

    AgentSummary {
        active: active.len(),
        idle: idle.len(),
        total_spawned,
        names,
    }
}

/// Shorten agent type names for compact display
fn shorten_model(model: &str) -> String {
    // Normalize: lowercase, replace spaces with hyphens for uniform matching
    let m = model.to_lowercase().replace(' ', "-");
    match m.as_str() {
        s if s.contains("opus-4.6") || s.contains("opus-4-6") => "op4.6".to_string(),
        s if s.contains("sonnet-4.6") || s.contains("sonnet-4-6") => "sn4.6".to_string(),
        s if s.contains("haiku-4.5") || s.contains("haiku-4-5") => "hk4.5".to_string(),
        s if s.contains("opus-4.5") || s.contains("opus-4-5") => "op4.5".to_string(),
        s if s.contains("sonnet-4.5") || s.contains("sonnet-4-5") => "sn4.5".to_string(),
        s if s.contains("opus-4") => "op4".to_string(),
        s if s.contains("sonnet-4") => "sn4".to_string(),
        s if s.contains("sonnet-3.5") || s.contains("sonnet-3-5") => "sn3.5".to_string(),
        s if s.contains("haiku-3.5") || s.contains("haiku-3-5") => "hk3.5".to_string(),
        _ => model.to_string(),
    }
}

struct HookHealth {
    total: usize, // configured hook event types
    ok: usize,    // called this session with zero errors
}

fn compute_hook_health(db: &MemoryDb, session_id: &str) -> HookHealth {
    // Count configured hooks from settings.json
    let total = count_configured_hooks();

    // Get session metrics to find which hooks have been called and which errored
    let metrics = db.get_session_metrics(session_id).unwrap_or_default();

    let mut called: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut errored: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (name, value) in &metrics {
        if let Some(hook) = name.strip_prefix("hook_calls:") {
            if *value > 0.0 {
                called.insert(hook.to_string());
            }
        }
        if let Some(hook) = name.strip_prefix("hook_errors:") {
            if *value > 0.0 {
                errored.insert(hook.to_string());
            }
        }
    }

    let ok = called.iter().filter(|h| !errored.contains(*h)).count();
    HookHealth { total, ok }
}

fn count_configured_hooks() -> usize {
    let settings_path = FlowForgeConfig::project_dir()
        .parent()
        .unwrap_or(".".as_ref())
        .join(".claude/settings.json");
    let content = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let val: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return 0,
    };
    val.get("hooks")
        .and_then(|h| h.as_object())
        .map(|obj| obj.len())
        .unwrap_or(0)
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
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
