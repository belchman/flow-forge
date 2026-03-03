use colored::Colorize;
use flowforge_core::{AgentSessionStatus, FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;

// ── Symbols ──
const SYM_FORGE: &str = "\u{2B22}"; // ⬢ hexagon
const SYM_CLOCK: &str = "\u{23F1}"; // ⏱ stopwatch
const SYM_EDIT: &str = "\u{270E}"; // ✎ pencil
const SYM_CMD: &str = "\u{2318}"; // ⌘ command
const SYM_PLAY: &str = "\u{25B6}"; // ▶ play
const SYM_TRUST: &str = "\u{26A1}"; // ⚡ lightning
const SYM_AGENT: &str = "\u{2726}"; // ✦ star
const SYM_WORK: &str = "\u{2690}"; // ⚐ flag
const SYM_BRAIN: &str = "\u{2022}"; // • bullet
const SYM_MAIL: &str = "\u{2709}"; // ✉ envelope
const SYM_ROUTE: &str = "\u{2192}"; // → arrow
const SYM_WARN: &str = "\u{26A0}"; // ⚠ warning
const SYM_PIPE: &str = "\u{2502}"; // │ separator

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
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let project_name = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string());

    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path()).ok();
    let db = config
        .as_ref()
        .and_then(|c| MemoryDb::open(&c.db_path()).ok());

    let mut sections = Vec::new();

    // ── Section 1: Identity ──
    let mut identity = format!("{} {}", SYM_FORGE.cyan(), project_name.bold().cyan());
    if !model.is_empty() {
        let short_model = shorten_model(model);
        identity = format!("{} {}", identity, short_model.dimmed());
    }
    sections.push(identity);

    // ── Section 2: Session activity ──
    let current_session_id;
    if let Some(ref db) = db {
        if let Ok(Some(session)) = db.get_current_session() {
            current_session_id = Some(session.id.clone());
            let duration = format_duration(
                chrono::Utc::now()
                    .signed_duration_since(session.started_at)
                    .num_seconds(),
            );

            let edits_str = if session.edits > 0 {
                format!("{}{}", SYM_EDIT, session.edits)
                    .yellow()
                    .to_string()
            } else {
                format!("{}{}", SYM_EDIT, 0).dimmed().to_string()
            };
            let cmds_str = if session.commands > 0 {
                format!("{}{}", SYM_CMD, session.commands).to_string()
            } else {
                format!("{}{}", SYM_CMD, 0).dimmed().to_string()
            };

            sections.push(format!(
                "{}{} {} {}",
                SYM_CLOCK.dimmed(),
                duration.dimmed(),
                edits_str,
                cmds_str,
            ));

            // ── Section 3: Trajectory quality ──
            if let Ok(Some(traj)) = db.get_active_trajectory(&session.id) {
                let steps = db.get_trajectory_steps(&traj.id).unwrap_or_default();
                let total = steps.len();
                if total > 0 {
                    let successes = steps
                        .iter()
                        .filter(|s| s.outcome == flowforge_core::trajectory::StepOutcome::Success)
                        .count();
                    let failures = steps
                        .iter()
                        .filter(|s| s.outcome == flowforge_core::trajectory::StepOutcome::Failure)
                        .count();
                    let ratio = successes as f64 / total as f64;
                    let bar = progress_bar(ratio, 5);
                    let ratio_pct = format!("{}%", (ratio * 100.0) as u32);
                    let colored_pct = color_by_ratio(ratio, &ratio_pct);
                    let detail = if failures > 0 {
                        format!(" {}", format!("{}ok {}err", successes, failures).dimmed())
                    } else {
                        format!(" {}", format!("{}/{}", successes, total).dimmed())
                    };
                    sections.push(format!(
                        "{}{}{}{}",
                        SYM_PLAY.dimmed(),
                        bar,
                        colored_pct,
                        detail,
                    ));
                }
            }

            // ── Section 4: Trust score ──
            if let Ok(Some(trust)) = db.get_trust_score(&session.id) {
                let trust_pct = (trust.score * 100.0) as u32;
                let shield = color_by_trust(trust.score, SYM_TRUST);
                let trust_str = format!("{}%", trust_pct);
                let colored_trust = color_by_trust(trust.score, &trust_str);
                let mut trust_detail = format!("{}{}", shield, colored_trust);
                if trust.denials > 0 {
                    trust_detail = format!(
                        "{} {}",
                        trust_detail,
                        format!("{}deny", trust.denials).red()
                    );
                }
                if trust.asks > 0 {
                    trust_detail =
                        format!("{} {}", trust_detail, format!("{}ask", trust.asks).yellow());
                }
                sections.push(trust_detail);
            }
        } else {
            current_session_id = None;
        }
    } else {
        current_session_id = None;
    }

    // ── Section 5: Agents ──
    if let Some(ref db) = db {
        if let Ok(agents) = db.get_active_agent_sessions() {
            if !agents.is_empty() {
                let active: Vec<_> = agents
                    .iter()
                    .filter(|a| a.status == AgentSessionStatus::Active)
                    .collect();
                let idle: Vec<_> = agents
                    .iter()
                    .filter(|a| a.status == AgentSessionStatus::Idle)
                    .collect();

                let agent_display = if agents.len() <= 4 {
                    let mut parts = Vec::new();
                    for a in &active {
                        let name = shorten_agent_name(&a.agent_type);
                        let activity = if a.edits > 0 || a.commands > 0 {
                            format!(":{}/{}", a.edits, a.commands).dimmed().to_string()
                        } else {
                            String::new()
                        };
                        parts.push(format!("{}{}", name.bold().green(), activity));
                    }
                    for a in &idle {
                        let name = shorten_agent_name(&a.agent_type);
                        parts.push(name.dimmed().to_string());
                    }
                    parts.join(" ")
                } else {
                    let mut summary = Vec::new();
                    if !active.is_empty() {
                        summary.push(format!("{}", active.len()).green().bold().to_string());
                    }
                    if !idle.is_empty() {
                        summary.push(format!("{}", idle.len()).dimmed().to_string());
                    }
                    format!("{}[{}]", agents.len(), summary.join("/"))
                };

                sections.push(format!("{} {}", SYM_AGENT.bright_magenta(), agent_display));
            }
        }
    }

    // ── Section 6: Work items ──
    if let Some(ref db) = db {
        let in_progress = db.count_work_items_by_status("in_progress").unwrap_or(0);
        let pending = db.count_work_items_by_status("pending").unwrap_or(0);
        let blocked = db.count_work_items_by_status("blocked").unwrap_or(0);
        if in_progress > 0 || pending > 0 || blocked > 0 {
            let mut work_parts = Vec::new();
            if in_progress > 0 {
                work_parts.push(format!("{}wip", in_progress).bright_blue().to_string());
            }
            if pending > 0 {
                work_parts.push(format!("{}q", pending).dimmed().to_string());
            }
            if blocked > 0 {
                work_parts.push(format!("{}blk", blocked).red().to_string());
            }
            sections.push(format!(
                "{} {}",
                SYM_WORK.bright_blue(),
                work_parts.join(" "),
            ));
        }
    }

    // ── Section 7: Unread mail ──
    if let Some(ref db) = db {
        if let Some(ref sid) = current_session_id {
            if let Ok(unread) = db.get_unread_messages(sid) {
                if !unread.is_empty() {
                    sections.push(format!(
                        "{} {}",
                        SYM_MAIL.bright_yellow(),
                        format!("{}", unread.len()).bright_yellow(),
                    ));
                }
            }
        }
    }

    // ── Section 8: Routing weights ──
    if let Some(ref db) = db {
        let routes = db.count_routing_weights().unwrap_or(0);
        if routes > 0 {
            sections.push(format!(
                "{} {}",
                SYM_ROUTE.dimmed(),
                format!("{}w", routes).dimmed(),
            ));
        }
    }

    // ── Section 9: Learning (patterns + knowledge) ──
    if let Some(ref db) = db {
        let short = db.count_patterns_short().unwrap_or(0);
        let long = db.count_patterns_long().unwrap_or(0);
        let memories = db.count_kv().unwrap_or(0);

        if short + long + memories > 0 {
            let mut learn_parts = Vec::new();
            if long > 0 {
                learn_parts.push(format!("{}", long).bright_yellow().to_string());
            }
            if short > 0 {
                learn_parts.push(format!("{}", short).dimmed().to_string());
            }
            let pattern_str = if !learn_parts.is_empty() {
                format!("{}pat", learn_parts.join("/"))
            } else {
                String::new()
            };

            let mem_str = if memories > 0 {
                format!("{}mem", format!("{}", memories).bright_yellow())
            } else {
                String::new()
            };

            let combined: Vec<_> = [pattern_str, mem_str]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
            if !combined.is_empty() {
                sections.push(format!(
                    "{} {}",
                    SYM_BRAIN.bright_yellow(),
                    combined.join(" "),
                ));
            }
        }
    }

    // ── Section 10: Warnings / actionable items ──
    if let Some(ref db) = db {
        let mut warnings = Vec::new();

        // Check for stealable work items
        if let Ok(stealable) = db.get_stealable_items(5) {
            if !stealable.is_empty() {
                warnings.push(format!("{}stale", stealable.len()).yellow().to_string());
            }
        }

        // Check hook errors log
        let log_path = FlowForgeConfig::project_dir().join("hook-errors.log");
        if log_path.exists() {
            if let Ok(meta) = std::fs::metadata(&log_path) {
                if meta.len() > 0 {
                    warnings.push("hook-err".red().to_string());
                }
            }
        }

        if !warnings.is_empty() {
            sections.push(format!("{} {}", SYM_WARN.bright_red(), warnings.join(" "),));
        }
    }

    // Build final statusline
    let separator = format!(" {} ", SYM_PIPE.dimmed());
    print!("{}", sections.join(&separator));

    Ok(())
}

/// Print the legend explaining all statusline symbols.
pub fn print_legend() -> Result<()> {
    println!("{}", "FlowForge Statusline Legend".bold().cyan());
    println!("{}", "═".repeat(50).dimmed());
    println!();

    println!("{}", "IDENTITY".bold());
    println!(
        "  {} {}    Project name and model",
        SYM_FORGE.cyan(),
        "project".bold().cyan()
    );
    println!(
        "  {}         Model shorthand (op=opus, sn=sonnet, hk=haiku)",
        "op4.6".dimmed()
    );
    println!();

    println!("{}", "SESSION".bold());
    println!(
        "  {}{}        Session duration (s/m/h)",
        SYM_CLOCK.dimmed(),
        "5m".dimmed()
    );
    println!(
        "  {}{}         Files edited this session (yellow when > 0)",
        SYM_EDIT.yellow(),
        "3".yellow()
    );
    println!("  {}{:<11}Commands run this session", SYM_CMD, "12");
    println!();

    println!("{}", "TRAJECTORY".bold());
    println!(
        "  {}{}{}   Execution quality — success ratio with visual bar",
        SYM_PLAY.dimmed(),
        progress_bar(0.85, 5),
        "85%".yellow()
    );
    println!(
        "  {}          {}=green  {}=yellow  {}=red",
        "  thresholds".dimmed(),
        "\u{2265}90%".green(),
        "\u{2265}70%".yellow(),
        "<70%".red()
    );
    println!(
        "  {}           Successes and failures (or ok/total)",
        "5ok 1err".dimmed()
    );
    println!();

    println!("{}", "TRUST".bold());
    println!(
        "  {}{}       Guidance gate trust score",
        SYM_TRUST.green(),
        "85%".green()
    );
    println!(
        "  {}          {}=green  {}=yellow  {}=red",
        "  thresholds".dimmed(),
        "\u{2265}80%".green(),
        "\u{2265}50%".yellow(),
        "<50%".red()
    );
    println!(
        "  {}      Denials (red) and user-confirms (yellow)",
        "2deny 1ask".dimmed()
    );
    println!();

    println!("{}", "AGENTS".bold());
    println!(
        "  {} {}     Active sub-agents (green=active, dim=idle)",
        SYM_AGENT.bright_magenta(),
        "gen".bold().green()
    );
    println!(
        "  {}           Agent name abbreviations:",
        "  names".dimmed()
    );
    println!("                 gen=general  exp=explore  pln=plan  sim=simplifier");
    println!(
        "  {}        Agent with edit/command counts",
        "gen:3/5".dimmed()
    );
    println!(
        "  {}         Summary mode when >4 agents (active/idle)",
        "6[4/2]".dimmed()
    );
    println!();

    println!("{}", "WORK ITEMS".bold());
    println!(
        "  {} {}     In-progress work items (blue)",
        SYM_WORK.bright_blue(),
        "3wip".bright_blue()
    );
    println!("  {}          Queued/pending items (dim)", "2q".dimmed());
    println!("  {}         Blocked items (red)", "1blk".red());
    println!();

    println!("{}", "MAILBOX".bold());
    println!(
        "  {} {}          Unread co-agent messages",
        SYM_MAIL.bright_yellow(),
        "2".bright_yellow()
    );
    println!();

    println!("{}", "ROUTING".bold());
    println!(
        "  {} {}        Learned routing weights count",
        SYM_ROUTE.dimmed(),
        "15w".dimmed()
    );
    println!();

    println!("{}", "LEARNING".bold());
    let pat_example = format!("{}/{}pat", "12".bright_yellow(), "5".dimmed());
    println!(
        "  {} {}  Long-term (yellow) / short-term (dim) patterns",
        SYM_BRAIN.bright_yellow(),
        pat_example,
    );
    let mem_example = format!("{}mem", "8".bright_yellow());
    println!("  {}       Stored key-value memories", mem_example);
    println!();

    println!("{}", "WARNINGS".bold());
    println!(
        "  {} {}    Stale/abandoned work items available to steal",
        SYM_WARN.bright_red(),
        "2stale".yellow()
    );
    println!(
        "  {} {} Hook errors in .flowforge/hook-errors.log",
        SYM_WARN.bright_red(),
        "hook-err".red()
    );
    println!();

    println!("{}", "SEPARATORS".dimmed());
    println!(
        "  {}            Dim pipe separates sections",
        SYM_PIPE.dimmed()
    );
    println!();

    println!(
        "{}",
        "Run `flowforge statusline` to see the live status line.".dimmed()
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

/// Compact progress bar using Unicode block characters
fn progress_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    let bar_color = if ratio >= 0.9 {
        "\u{2588}".repeat(filled).green()
    } else if ratio >= 0.7 {
        "\u{2588}".repeat(filled).yellow()
    } else {
        "\u{2588}".repeat(filled).red()
    };

    format!("{}{}", bar_color, "\u{2591}".repeat(empty).dimmed())
}

/// Shorten model names for compact display
fn shorten_model(model: &str) -> String {
    if model.contains("opus") {
        if let Some(ver) = extract_version(model) {
            return format!("op{}", ver);
        }
        return "opus".to_string();
    }
    if model.contains("sonnet") {
        if let Some(ver) = extract_version(model) {
            return format!("sn{}", ver);
        }
        return "sonnet".to_string();
    }
    if model.contains("haiku") {
        if let Some(ver) = extract_version(model) {
            return format!("hk{}", ver);
        }
        return "haiku".to_string();
    }
    if model.len() > 12 {
        model[..12].to_string()
    } else {
        model.to_string()
    }
}

/// Extract version number like "4.6" from "claude-opus-4-6"
fn extract_version(model: &str) -> Option<String> {
    let parts: Vec<&str> = model.split('-').collect();
    for (i, part) in parts.iter().enumerate() {
        if *part == "opus" || *part == "sonnet" || *part == "haiku" {
            let version_parts: Vec<&str> = parts[i + 1..].to_vec();
            if !version_parts.is_empty() {
                let nums: Vec<&str> = version_parts
                    .iter()
                    .take_while(|p| p.chars().all(|c| c.is_ascii_digit()))
                    .copied()
                    .collect();
                if !nums.is_empty() {
                    return Some(nums.join("."));
                }
            }
        }
    }
    None
}

/// Shorten agent type names for compact display
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
