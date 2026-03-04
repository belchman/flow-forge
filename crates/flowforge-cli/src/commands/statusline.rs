use colored::Colorize;
use flowforge_core::{AgentSessionStatus, FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;

const SEP: &str = "\u{2502}"; // │
const HSEP: &str = "\u{2500}"; // ─

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
            v.get("display_name")
                .and_then(|d| d.as_str())
                .or_else(|| v.get("id").and_then(|id| id.as_str()))
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
    // LINE 1: Header — brand + user + git + model + context + duration
    // ══════════════════════════════════════════════════════════════
    let display_name = session_name.unwrap_or(&project_name);
    let mut header = format!(
        "{} {}",
        "\u{258A}".bold().bright_magenta(), // ▊
        format!("FlowForge {}", display_name)
            .bold()
            .bright_magenta()
    );

    // Git branch + changes
    if let Some(ref branch) = git_branch {
        header = format!(
            "{}  {}  {} {}",
            header,
            SEP.dimmed(),
            "\u{23C7}".bright_blue(), // ⏇
            branch.bright_blue()
        );
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
            header = format!("{} {}", header, changes.join(""));
        }
    }

    // Model
    if !model.is_empty() {
        header = format!("{}  {}  {}", header, SEP.dimmed(), model.bright_magenta());
    }

    // Context remaining
    if let Some(remaining) = ctx_remaining {
        let ctx_used = 100u32.saturating_sub(remaining);
        let ctx_str = format!("\u{1F4C2} {}%", ctx_used); // 📂
        let colored = if ctx_used < 50 {
            ctx_str.bright_green().to_string()
        } else if ctx_used < 70 {
            ctx_str.bright_cyan().to_string()
        } else if ctx_used < 85 {
            ctx_str.bright_yellow().to_string()
        } else {
            ctx_str.bright_red().to_string()
        };
        header = format!("{}  {}  {}", header, SEP.dimmed(), colored);
    }

    // Session duration
    if let Some(ref db) = db {
        if let Ok(Some(session)) = db.get_current_session() {
            let secs = chrono::Utc::now()
                .signed_duration_since(session.started_at)
                .num_seconds();
            let dur = format_duration(secs);
            header = format!(
                "{}  {}  {}{}",
                header,
                SEP.dimmed(),
                "\u{23F1}".cyan(), // ⏱
                dur.cyan()
            );
        }
    }

    lines.push(header);

    // Separator
    lines.push(HSEP.repeat(53).dimmed().to_string());

    // ══════════════════════════════════════════════════════════════
    // LINE 2: Learning — patterns + vectors + HNSW + intelligence
    // ══════════════════════════════════════════════════════════════
    if let Some(ref db) = db {
        let short = db.count_patterns_short().unwrap_or(0);
        let long = db.count_patterns_long().unwrap_or(0);
        let vectors = db.count_vectors().unwrap_or(0) as u64;
        let clusters = db.get_all_clusters().map(|c| c.len()).unwrap_or(0);
        let routes = db.count_routing_weights().unwrap_or(0);

        let is_semantic = config
            .as_ref()
            .map(|c| c.patterns.semantic_embeddings)
            .unwrap_or(false);

        // Pattern progress bar (short→long promotion)
        let total_patterns = short + long;
        let long_ratio = if total_patterns > 0 {
            long as f64 / total_patterns as f64
        } else {
            0.0
        };
        let pat_bar = progress_bar(long_ratio, 5);
        let pat_color = if long > 10 {
            format!("{}", long).bright_green().to_string()
        } else if long > 0 {
            format!("{}", long).yellow().to_string()
        } else {
            format!("{}", long).red().to_string()
        };

        // HNSW performance indicator
        let hnsw_str = if vectors > 10000 {
            format!("{} HNSW 12500x", "\u{26A1}") // ⚡
                .bright_green()
                .to_string()
        } else if vectors > 1000 {
            format!("{} HNSW 150x", "\u{26A1}")
                .bright_green()
                .to_string()
        } else if vectors > 0 {
            format!("{} HNSW", "\u{26A1}").bright_yellow().to_string()
        } else {
            format!("{} brute", "\u{26A1}").dimmed().to_string()
        };

        // Intelligence score: composite of patterns, vectors, routes, clusters
        let intel = compute_intelligence(short, long, vectors, clusters as u64, routes);
        let intel_str = format!("{}%", intel);
        let intel_colored = if intel >= 80 {
            intel_str.bright_green().to_string()
        } else if intel >= 40 {
            intel_str.bright_yellow().to_string()
        } else {
            intel_str.dimmed().to_string()
        };

        let embedder_tag = if is_semantic {
            "sem384".bright_cyan().to_string()
        } else {
            "hash".dimmed().to_string()
        };

        lines.push(format!(
            "{} {}    {}  {}/{}    {} {}{} {}    {} {}    {} \u{25CF} {}",
            "\u{1F4DA}".bright_cyan(), // 📚
            "Patterns".bright_cyan(),
            pat_bar,
            pat_color,
            format!("{}", short).dimmed(),
            "\u{1F9EC}".bright_cyan(), // 🧬
            format!("{}", vectors).bright_green(),
            embedder_tag,
            hnsw_str,
            "\u{1F9E9}".bright_cyan(), // 🧩
            if clusters > 0 {
                format!("{}c", clusters).bright_green().to_string()
            } else {
                "0c".dimmed().to_string()
            },
            "\u{1F9E0}".bright_cyan(), // 🧠
            intel_colored,
        ));
    }

    // ══════════════════════════════════════════════════════════════
    // LINE 3: Swarm — agents + trust + work + mail + routes
    // ══════════════════════════════════════════════════════════════
    if let Some(ref db) = db {
        let mut swarm_parts = Vec::new();

        // Agents
        let (active_count, idle_count, agent_names) = get_agent_summary(db);
        let swarm_dot = if active_count > 0 {
            "\u{25C9}".bright_green().to_string() // ◉
        } else {
            "\u{25CB}".dimmed().to_string() // ○
        };
        swarm_parts.push(format!(
            "{} {} [{}/{}]  {} {}",
            "\u{1F916}".bright_yellow(), // 🤖
            "Swarm".bright_yellow(),
            swarm_dot,
            if active_count + idle_count > 0 {
                format!("{}", active_count + idle_count)
                    .bright_green()
                    .to_string()
            } else {
                "0".dimmed().to_string()
            },
            "\u{1F465}".bright_purple(), // 👥
            if !agent_names.is_empty() {
                agent_names.join(" ")
            } else {
                "--".dimmed().to_string()
            },
        ));

        // Session-dependent metrics (trust, trajectory, mail)
        if let Ok(Some(session)) = db.get_current_session() {
            let sid = &session.id;

            // Trust score
            if let Ok(Some(trust)) = db.get_trust_score(sid) {
                let trust_pct = (trust.score * 100.0) as u32;
                let trust_str = format!("{}%", trust_pct);
                let colored = color_by_trust(trust.score, &trust_str);
                let shield = color_by_trust(trust.score, "\u{1F6E1}\u{FE0F}"); // 🛡️
                let mut detail = format!("{} {}", shield, colored);
                if trust.denials > 0 {
                    detail = format!("{} {}", detail, format!("{}d", trust.denials).red());
                }
                swarm_parts.push(detail);
            }

            // Trajectory
            if let Ok(Some(traj)) = db.get_active_trajectory(sid) {
                let steps = db.get_trajectory_steps(&traj.id).unwrap_or_default();
                let total = steps.len();
                if total > 0 {
                    let successes = steps
                        .iter()
                        .filter(|s| s.outcome == flowforge_core::trajectory::StepOutcome::Success)
                        .count();
                    let ratio = successes as f64 / total as f64;
                    let bar = progress_bar(ratio, 4);
                    let pct = format!("{}%", (ratio * 100.0) as u32);
                    let colored = color_by_ratio(ratio, &pct);
                    swarm_parts.push(format!("{}{} {}", "\u{25B6}".dimmed(), bar, colored));
                }
            }

            // Unread mail
            if let Ok(unread) = db.get_unread_messages(sid) {
                if !unread.is_empty() {
                    swarm_parts.push(format!(
                        "{} {}",
                        "\u{1F4E8}".bright_yellow(), // 📨
                        format!("{}", unread.len()).bright_yellow()
                    ));
                }
            }

            // Session edits + commands
            if session.edits > 0 || session.commands > 0 {
                let mut activity = Vec::new();
                if session.edits > 0 {
                    activity.push(format!("\u{270E}{}", session.edits).yellow().to_string());
                }
                if session.commands > 0 {
                    activity.push(format!("\u{2318}{}", session.commands).to_string());
                }
                swarm_parts.push(activity.join(" "));
            }
        }

        // Work items (outside session scope — always show)
        let wip = db.count_work_items_by_status("in_progress").unwrap_or(0);
        let pending = db.count_work_items_by_status("pending").unwrap_or(0);
        if wip > 0 || pending > 0 {
            let mut w = Vec::new();
            if wip > 0 {
                w.push(format!("{}wip", wip).bright_blue().to_string());
            }
            if pending > 0 {
                w.push(format!("{}q", pending).dimmed().to_string());
            }
            swarm_parts.push(format!("{} {}", "\u{2690}".bright_blue(), w.join(" ")));
        }

        lines.push(swarm_parts.join("    "));
    }

    // ══════════════════════════════════════════════════════════════
    // LINE 4: Architecture — subsystem status indicators
    // ══════════════════════════════════════════════════════════════
    if let Some(ref db) = db {
        let routes = db.count_routing_weights().unwrap_or(0);
        let memories = db.count_kv().unwrap_or(0);
        let vectors = db.count_vectors().unwrap_or(0) as u64;

        let is_semantic = config
            .as_ref()
            .map(|c| c.patterns.semantic_embeddings)
            .unwrap_or(false);

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
        let hooks_color = if hooks_count >= 12 {
            format!("\u{2713}{}h", hooks_count)
                .bright_green()
                .to_string()
        } else if hooks_count > 0 {
            format!("\u{2713}{}h", hooks_count)
                .bright_yellow()
                .to_string()
        } else {
            "\u{2713}0h".dimmed().to_string()
        };

        // MCP tools count
        let mcp_path = std::env::current_dir()
            .unwrap_or_default()
            .join(".mcp.json");
        let mcp_active = mcp_path.exists();
        let mcp_str = if mcp_active {
            "MCP53".bright_green().to_string()
        } else {
            "MCP--".dimmed().to_string()
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

        // Embedder indicator
        let embed_dot = if is_semantic {
            "\u{25CF}".bright_green().to_string() // ●
        } else {
            "\u{25CF}".dimmed().to_string()
        };
        let embed_label = if is_semantic {
            "Semantic".bright_green().to_string()
        } else {
            "Hash".dimmed().to_string()
        };

        // Warnings
        let mut warn_parts = Vec::new();
        if let Ok(stealable) = db.get_stealable_items(5) {
            if !stealable.is_empty() {
                warn_parts.push(format!("{}stale", stealable.len()).yellow().to_string());
            }
        }
        let log_path = FlowForgeConfig::project_dir().join("hook-errors.log");
        if log_path.exists() {
            if let Ok(meta) = std::fs::metadata(&log_path) {
                if meta.len() > 0 {
                    warn_parts.push("err".red().to_string());
                }
            }
        }

        let warn_str = if warn_parts.is_empty() {
            String::new()
        } else {
            format!("    {} {}", "\u{26A0}".bright_red(), warn_parts.join(" "))
        };

        lines.push(format!(
            "{} {}    {} {}{}  {}  {} {}  {}  {} {}  {}  {} {} {}  {}  {}{}",
            "\u{1F527}".bright_purple(), // 🔧
            "Arch".bright_purple(),
            "Embed".cyan(),
            embed_dot,
            embed_label,
            SEP.dimmed(),
            "Vectors".cyan(),
            if vectors > 0 {
                format!("\u{25CF}{}", vectors).bright_green().to_string()
            } else {
                "\u{25CF}0".dimmed().to_string()
            },
            SEP.dimmed(),
            "DB".cyan(),
            db_str.bright_white(),
            SEP.dimmed(),
            "Routes".cyan(),
            if routes > 0 {
                format!("\u{25CF}{}", routes).bright_green().to_string()
            } else {
                "\u{25CF}0".dimmed().to_string()
            },
            format!("{}kv", memories).dimmed(),
            SEP.dimmed(),
            format!("{}  {}", hooks_color, mcp_str),
            warn_str,
        ));
    }

    // Footer separator
    lines.push(HSEP.repeat(53).dimmed().to_string());

    // Print multi-line dashboard
    println!("{}", lines.join("\n"));

    Ok(())
}

/// Print the legend explaining all statusline symbols.
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
    println!(
        "  {} branch     Git branch with +staged ~modified ?untracked",
        "\u{23C7}".bright_blue()
    );
    println!(
        "  {}            Model name (Opus 4.6, Sonnet 4.6, etc.)",
        "model".bright_magenta()
    );
    println!(
        "  {} 23%        Context window usage (green<50 cyan<70 yellow<85 red)",
        "\u{1F4C2}"
    );
    println!("  {}5m          Session duration", "\u{23F1}".cyan());
    println!();

    println!("{}", "LEARNING LINE".bold());
    println!(
        "  {} 5          Progress bar: long-term / short-term promotion",
        progress_bar(0.5, 5)
    );
    println!(
        "  {} 500sem384  HNSW vector count + embedder type",
        "\u{1F9EC}"
    );
    println!(
        "  {} HNSW 150x  Vector index speedup indicator",
        "\u{26A1}".bright_green()
    );
    println!("  {} 3c         DBSCAN topic clusters", "\u{1F9E9}");
    println!(
        "  {} 65%        Intelligence score (patterns + vectors + routes + clusters)",
        "\u{1F9E0}"
    );
    println!();

    println!("{}", "SWARM LINE".bold());
    println!(
        "  {} [0/2]      Active agent count + total, named agents",
        "\u{1F916}"
    );
    println!(
        "  {} 85%        Guidance trust score (green>=80 yellow>=50 red)",
        "\u{1F6E1}\u{FE0F}"
    );
    println!(
        "  {}{}85%   Trajectory success ratio bar",
        "\u{25B6}".dimmed(),
        progress_bar(0.85, 4)
    );
    println!(
        "  {} 2wip 1q    In-progress and queued work items",
        "\u{2690}".bright_blue()
    );
    println!("  {} 3          Unread co-agent messages", "\u{1F4E8}");
    println!();

    println!("{}", "ARCHITECTURE LINE".bold());
    println!(
        "  Embed        {}Semantic or {}Hash embedder",
        "\u{25CF}".bright_green(),
        "\u{25CF}".dimmed()
    );
    println!("  Vectors      HNSW entry count");
    println!("  DB           Database file size");
    println!("  Routes       Learned routing weights + KV memories");
    println!(
        "  {}          Hooks wired / MCP server status",
        "\u{2713}13h MCP53".bright_green()
    );
    println!(
        "  {} stale err  Stealable work items / hook errors",
        "\u{26A0}".bright_red()
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

/// Compute intelligence score (0-100) from learning metrics
fn compute_intelligence(
    short_patterns: u64,
    long_patterns: u64,
    vectors: u64,
    clusters: u64,
    routes: u64,
) -> u32 {
    // Pattern maturity: long-term patterns are worth more
    let pattern_score = ((short_patterns as f64 * 0.2) + (long_patterns as f64 * 2.0)).min(40.0);
    // Vector density: more vectors = better recall
    let vector_score = if vectors > 0 {
        ((vectors as f64).ln() * 5.0).min(25.0)
    } else {
        0.0
    };
    // Cluster formation: clusters mean the system has found structure
    let cluster_score = (clusters as f64 * 5.0).min(15.0);
    // Routing: learned routing weights indicate experience
    let route_score = (routes as f64 * 3.0).min(20.0);

    let total = pattern_score + vector_score + cluster_score + route_score;
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
