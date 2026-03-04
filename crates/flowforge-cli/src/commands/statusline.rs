use colored::Colorize;
use flowforge_core::{AgentSessionStatus, FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;

const SEP: &str = "\u{2502}"; // │
const HSEP: &str = "\u{2500}"; // ─

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
    // LINE 1: Header — brand + git + model + context + duration
    // ══════════════════════════════════════════════════════════════
    let display_name = session_name.unwrap_or(&project_name);
    let mut header_parts: Vec<String> = vec![format!(
        "{} {}",
        "\u{258A}".bold().bright_magenta(), // ▊
        format!("FlowForge {}", display_name)
            .bold()
            .bright_magenta()
    )];

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

    // Separator
    lines.push(HSEP.repeat(53).dimmed().to_string());

    // ══════════════════════════════════════════════════════════════
    // LINE 2: Learn — pattern progress + clusters + routes + IQ
    // ══════════════════════════════════════════════════════════════
    if let Some(ref db) = db {
        let short = db.count_patterns_short().unwrap_or(0);
        let long = db.count_patterns_long().unwrap_or(0);
        let clusters = db.get_all_clusters().map(|c| c.len()).unwrap_or(0);
        let routes = db.count_routing_weights().unwrap_or(0);
        let vectors = db.count_vectors().unwrap_or(0) as u64;

        let intel = compute_intelligence(short, long, vectors, clusters as u64, routes, Some(db));

        let mut learn_parts: Vec<String> = Vec::new();

        // Pattern counts: "N proven  N recent"
        let long_str = if long > 10 {
            format!("{}", long).bright_green().to_string()
        } else if long > 0 {
            format!("{}", long).yellow().to_string()
        } else {
            "0".dimmed().to_string()
        };
        learn_parts.push(format!(
            "{} proven  {} recent",
            long_str,
            format!("{}", short).dimmed()
        ));

        // Clusters
        if clusters > 0 {
            learn_parts.push(format!("{} clusters", clusters).bright_green().to_string());
        }

        // Routes
        if routes > 0 {
            learn_parts.push(format!("{} routes", routes).bright_green().to_string());
        }

        // Intelligence score
        let intel_str = format!("IQ {}%", intel);
        let intel_colored = if intel >= 80 {
            intel_str.bright_green().to_string()
        } else if intel >= 40 {
            intel_str.bright_yellow().to_string()
        } else {
            intel_str.dimmed().to_string()
        };
        learn_parts.push(intel_colored);

        lines.push(format!(
            "{}  {}",
            "Learn:".bright_cyan(),
            learn_parts.join("  ")
        ));
    }

    // ══════════════════════════════════════════════════════════════
    // LINE 3: Swarm — agents + trust + trajectory + work + warnings
    // ══════════════════════════════════════════════════════════════
    if let Some(ref db) = db {
        let mut swarm_parts: Vec<String> = Vec::new();

        // Agents: "N active (names)" or "no agents"
        let (active_count, idle_count, agent_names) = get_agent_summary(db);
        let total = active_count + idle_count;
        if total > 0 {
            swarm_parts.push(format!(
                "{} active ({})",
                format!("{}", active_count).bright_green(),
                if !agent_names.is_empty() {
                    agent_names.join(" ")
                } else {
                    "--".dimmed().to_string()
                }
            ));
        } else {
            swarm_parts.push("no agents".dimmed().to_string());
        }

        // Session-dependent metrics
        if let Ok(Some(session)) = db.get_current_session() {
            let sid = &session.id;

            // Trust score
            if let Ok(Some(trust)) = db.get_trust_score(sid) {
                let trust_pct = (trust.score * 100.0) as u32;
                let trust_str = format!("trust {}%", trust_pct);
                let mut detail = color_by_trust(trust.score, &trust_str);
                if trust.denials > 0 {
                    detail = format!("{} {}", detail, format!("{}deny", trust.denials).red());
                }
                swarm_parts.push(detail);
            }

            // Trajectory progress
            if let Ok(Some(traj)) = db.get_active_trajectory(sid) {
                let steps = db.get_trajectory_steps(&traj.id).unwrap_or_default();
                let step_count = steps.len();
                if step_count > 0 {
                    let successes = steps
                        .iter()
                        .filter(|s| s.outcome == flowforge_core::trajectory::StepOutcome::Success)
                        .count();
                    let ratio = successes as f64 / step_count as f64;
                    let bar = progress_bar(ratio, 4);
                    let pct = format!("{}%", (ratio * 100.0) as u32);
                    let colored = color_by_ratio(ratio, &pct);
                    swarm_parts.push(format!("traj {}{}", bar, colored));
                }
            }

            // Unread mail
            if let Ok(unread) = db.get_unread_messages(sid) {
                if !unread.is_empty() {
                    swarm_parts.push(format!("{} mail", unread.len()).bright_yellow().to_string());
                }
            }

            // Session activity
            if session.edits > 0 || session.commands > 0 {
                let mut activity = Vec::new();
                if session.edits > 0 {
                    activity.push(format!("{} edits", session.edits));
                }
                if session.commands > 0 {
                    activity.push(format!("{} cmds", session.commands));
                }
                swarm_parts.push(activity.join(" ").dimmed().to_string());
            }
        }

        // Work items
        let wip = db.count_work_items_by_status("in_progress").unwrap_or(0);
        let pending = db.count_work_items_by_status("pending").unwrap_or(0);
        if wip > 0 || pending > 0 {
            let mut w = Vec::new();
            if wip > 0 {
                w.push(format!("{} work active", wip));
            }
            if pending > 0 {
                w.push(format!("{} work pending", pending));
            }
            swarm_parts.push(w.join("  ").bright_blue().to_string());
        }

        // Warnings (keep prominent, not buried in debug)
        let mut warn_parts = Vec::new();
        if let Ok(stealable) = db.get_stealable_items(5) {
            if !stealable.is_empty() {
                warn_parts.push(format!("{} stale", stealable.len()).yellow().to_string());
            }
        }
        let log_path = FlowForgeConfig::project_dir().join("hook-errors.log");
        if log_path.exists() {
            if let Ok(meta) = std::fs::metadata(&log_path) {
                if meta.len() > 0 {
                    warn_parts.push("hook-err".red().to_string());
                }
            }
        }
        if !warn_parts.is_empty() {
            swarm_parts.push(format!("!! {}", warn_parts.join(" ")));
        }

        lines.push(format!(
            "{}  {}",
            "Swarm:".bright_yellow(),
            swarm_parts.join("  ")
        ));
    }

    // ══════════════════════════════════════════════════════════════
    // LINE 4: Debug — infrastructure internals (dimmed)
    // ══════════════════════════════════════════════════════════════
    if let Some(ref db) = db {
        let vectors = db.count_vectors().unwrap_or(0) as u64;
        let memories = db.count_kv().unwrap_or(0);

        let is_semantic = config
            .as_ref()
            .map(|c| c.patterns.semantic_embeddings)
            .unwrap_or(false);

        let embedder = if is_semantic { "sem384" } else { "hash128" };

        let hnsw = if vectors > 10000 {
            "HNSW:12500x"
        } else if vectors > 1000 {
            "HNSW:150x"
        } else if vectors > 0 {
            "HNSW"
        } else {
            "brute"
        };

        // DB size
        let db_size = config
            .as_ref()
            .and_then(|c| std::fs::metadata(c.db_path()).ok())
            .map(|m| m.len())
            .unwrap_or(0);
        let db_str = if db_size > 1024 * 1024 {
            format!("{:.1}MB", db_size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{}KB", db_size / 1024)
        };

        // Hooks count
        let hooks_path = std::env::current_dir()
            .unwrap_or_default()
            .join(".claude/settings.json");
        let hooks_count = if hooks_path.exists() {
            std::fs::read_to_string(&hooks_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get("hooks").cloned())
                .map(|h| h.as_object().map(|o| o.len()).unwrap_or(0))
                .unwrap_or(0)
        } else {
            0
        };

        // MCP
        let mcp_path = std::env::current_dir()
            .unwrap_or_default()
            .join(".mcp.json");
        let mcp_str = if mcp_path.exists() { "MCP53" } else { "MCP--" };

        let debug_parts = [
            embedder.to_string(),
            format!("{}vec", vectors),
            hnsw.to_string(),
            db_str,
            format!("{}hooks", hooks_count),
            mcp_str.to_string(),
            format!("{}kv", memories),
        ];

        lines.push(
            format!("debug:  {}", debug_parts.join("  "))
                .dimmed()
                .to_string(),
        );
    }

    // Footer separator
    lines.push(HSEP.repeat(53).dimmed().to_string());

    // Print multi-line dashboard
    println!("{}", lines.join("\n"));

    Ok(())
}

/// Print the legend explaining all statusline symbols.
#[allow(clippy::print_literal)]
pub fn print_legend() -> Result<()> {
    println!("{}", "FlowForge Dashboard Legend".bold().cyan());
    println!("{}", HSEP.repeat(53).dimmed());
    println!();

    println!("{}", "HEADER LINE".bold());
    println!(
        "  {} {}  Brand + project/session name",
        "\u{258A}".bold().bright_magenta(),
        "FlowForge".bold().bright_magenta()
    );
    println!("  branch       Git branch with +staged ~modified ?untracked");
    println!("  op4.6        Model name (Opus 4.6, Sonnet 4.6, etc.)");
    println!("  ctx 23%      Context window usage (green<50 cyan<70 yellow<85 red)");
    println!("  5m           Session duration");
    println!();

    println!("{}", "LEARN LINE".bold());
    println!("  N proven     Long-term patterns (promoted from short-term)");
    println!("  N recent     Short-term patterns (not yet promoted)");
    println!("  N clusters   DBSCAN topic clusters");
    println!("  N routes     Learned agent routing weights");
    println!("  IQ N%        Intelligence score (70% outcomes + 30% volume)");
    println!();

    println!("{}", "SWARM LINE".bold());
    println!("  N active     Active agents with shortened names");
    println!("  trust N%     Guidance trust score (green>=80 yellow>=50 red)");
    println!(
        "  traj {}N%  Trajectory success ratio bar",
        progress_bar(0.85, 4)
    );
    println!("  N mail       Unread co-agent messages");
    println!("  N edits      File edits this session");
    println!("  N cmds       Commands run this session");
    println!("  N work active   In-progress work items");
    println!("  N work pending  Pending work items");
    println!("  !! stale     Stealable work items (warning)");
    println!(
        "  !! {}    Hook error log not empty (warning)",
        "hook-err".red()
    );
    println!();

    println!("{}", "DEBUG LINE (infrastructure)".bold());
    println!("  sem384       Semantic embedder (AllMiniLM, 384-dim)");
    println!("  hash128      Hash embedder (xxhash n-gram, 128-dim)");
    println!("  Nvec         Vector count in HNSW index");
    println!("  HNSW:Nx      Index speedup tier (brute / HNSW / HNSW:150x / HNSW:12500x)");
    println!("  N.NMB        Database file size");
    println!("  Nhooks       Claude Code hooks wired");
    println!("  MCPN         MCP server tool count");
    println!("  Nkv          Key-value memory entries");
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

/// Compact progress bar using Unicode block/dot characters
fn progress_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let fill_char = "\u{25CF}"; // ●
    let empty_char = "\u{25CB}"; // ○

    let bar_color = if ratio >= 0.9 {
        fill_char.repeat(filled).green()
    } else if ratio >= 0.7 {
        fill_char.repeat(filled).yellow()
    } else if ratio > 0.0 {
        fill_char.repeat(filled).red()
    } else {
        fill_char.repeat(filled).dimmed()
    };

    format!("[{}{}]", bar_color, empty_char.repeat(empty).dimmed())
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

/// Outcome metrics for intelligence score v2
struct OutcomeMetrics {
    trajectory_success_rate: f64,
    routing_accuracy: f64,
    routing_total: u64,
    promotion_rate: f64,
    route_count: u64,
}

fn get_outcome_metrics(db: &MemoryDb) -> OutcomeMetrics {
    let trajectory_success_rate = db.recent_trajectory_success_rate(20).unwrap_or(0.0);
    let (routing_hits, routing_total) = db.routing_accuracy_stats().unwrap_or((0, 0));
    let routing_accuracy = if routing_total > 0 {
        routing_hits as f64 / routing_total as f64
    } else {
        0.0
    };

    let short = db.count_patterns_short().unwrap_or(0);
    let long = db.count_patterns_long().unwrap_or(0);
    let total_patterns = short + long;
    let promotion_rate = if total_patterns > 0 {
        long as f64 / total_patterns as f64
    } else {
        0.0
    };

    let route_count = db.count_routing_weights().unwrap_or(0);

    OutcomeMetrics {
        trajectory_success_rate,
        routing_accuracy,
        routing_total,
        promotion_rate,
        route_count,
    }
}

/// Compute intelligence score (0-100) — outcome-based (70%) + volume (30%)
fn compute_intelligence(
    short_patterns: u64,
    long_patterns: u64,
    vectors: u64,
    clusters: u64,
    _routes: u64,
    db: Option<&MemoryDb>,
) -> u32 {
    // Volume (30%): patterns (15 max) + vectors (10 max) + clusters (5 max)
    let pattern_score = ((short_patterns as f64 * 0.1) + (long_patterns as f64 * 1.0)).min(15.0);
    let vector_score = if vectors > 0 {
        ((vectors as f64).ln() * 2.0).min(10.0)
    } else {
        0.0
    };
    let cluster_score = (clusters as f64 * 2.5).min(5.0);
    let volume = pattern_score + vector_score + cluster_score;

    // Outcome (70%): trajectory success (30) + routing accuracy (20) + promotion (10) + routes (10)
    let outcome = if let Some(db) = db {
        let m = get_outcome_metrics(db);
        let traj_score = m.trajectory_success_rate * 30.0;
        let routing_score = if m.routing_total > 2 {
            m.routing_accuracy * 20.0
        } else {
            0.0
        };
        let promo_score = m.promotion_rate * 10.0;
        let route_score = (m.route_count as f64 * 2.0).min(10.0);
        traj_score + routing_score + promo_score + route_score
    } else {
        0.0
    };

    let total = volume + outcome;
    (total.min(100.0)) as u32
}

/// Get agent summary: (active_count, idle_count, formatted_names)
fn get_agent_summary(db: &MemoryDb) -> (usize, usize, Vec<String>) {
    let agents = db.get_active_agent_sessions().unwrap_or_default();
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
