mod commands;
mod hooks;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "flowforge",
    about = "Agent orchestration for Claude Code",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize FlowForge in the current project
    Init {
        /// Initialize for the current project
        #[arg(long)]
        project: bool,
        /// Also set up global config
        #[arg(long)]
        global: bool,
    },
    /// Handle Claude Code hooks
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
    /// Show FlowForge status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Memory operations
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
    /// Session management
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// Pattern learning operations
    Learn {
        #[command(subcommand)]
        action: LearnAction,
    },
    /// Agent management
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Route a task to the best agent
    Route {
        /// Task description to route
        task: String,
    },
    /// Output status line for Claude Code terminal display
    Statusline {
        /// Show legend explaining all statusline symbols
        #[arg(long)]
        legend: bool,
    },
    /// tmux monitor management
    Tmux {
        #[command(subcommand)]
        action: TmuxAction,
    },
    /// Start the MCP server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Work tracking operations
    Work {
        #[command(subcommand)]
        action: WorkAction,
    },
    /// Co-agent mailbox operations
    Mailbox {
        #[command(subcommand)]
        action: MailboxAction,
    },
    /// Error recovery intelligence
    Error {
        #[command(subcommand)]
        action: ErrorAction,
    },
    /// Guidance control plane management
    Guidance {
        #[command(subcommand)]
        action: GuidanceAction,
    },
    /// Plugin management
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
    /// View and modify FlowForge configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Test all hooks with realistic Claude Code payloads
    TestHooks {
        /// Filter to a specific hook event (e.g. "pre-tool-use", "session-start")
        #[arg(long)]
        event: Option<String>,
        /// Show stdin/stdout/stderr/timing details for each hook
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Subcommand)]
enum HookEvent {
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    Notification,
    UserPromptSubmit,
    SessionStart,
    SessionEnd,
    Stop,
    PreCompact,
    SubagentStart,
    SubagentStop,
    TeammateIdle,
    TaskCompleted,
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Get a value by key
    Get {
        key: String,
        #[arg(long, default_value = "default")]
        namespace: String,
    },
    /// Set a key-value pair
    Set {
        key: String,
        value: String,
        #[arg(long, default_value = "default")]
        namespace: String,
    },
    /// Delete a key
    Delete {
        key: String,
        #[arg(long, default_value = "default")]
        namespace: String,
    },
    /// List keys in a namespace
    List {
        #[arg(long, default_value = "default")]
        namespace: String,
    },
    /// Search memory by query
    Search {
        query: String,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum SessionAction {
    /// Show current session info
    Current {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List recent sessions
    List {
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Show session metrics
    Metrics,
    /// List agent sessions for current or specified session
    Agents {
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Show conversation history for a session
    History {
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Manually ingest a transcript file
    Ingest {
        /// Path to JSONL transcript file
        path: String,
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Create a checkpoint at the current conversation position
    Checkpoint {
        /// Checkpoint name
        name: String,
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// List checkpoints for a session
    Checkpoints {
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Fork a session's conversation at a checkpoint or index
    Fork {
        #[arg(long)]
        session_id: Option<String>,
        #[arg(long)]
        checkpoint: Option<String>,
        #[arg(long)]
        at_index: Option<u32>,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Show fork tree for a session
    Forks {
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Show hook timing metrics for a session
    HookTiming {
        #[arg(long)]
        session_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum LearnAction {
    /// Store a new pattern
    Store {
        content: String,
        #[arg(long, default_value = "general")]
        category: String,
    },
    /// Search patterns
    Search {
        query: String,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Show learning statistics
    Stats {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List recorded trajectories
    Trajectories {
        /// Filter by session ID
        #[arg(long)]
        session: Option<String>,
        /// Filter by status (recording, completed, failed, judged)
        #[arg(long)]
        status: Option<String>,
        /// Max results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Show trajectory details
    Trajectory {
        /// Trajectory ID
        id: String,
    },
    /// Judge a trajectory
    Judge {
        /// Trajectory ID
        id: String,
    },
    /// Download semantic embedding model
    DownloadModel,
    /// Show topic clusters
    Clusters,
    /// Auto-tune DBSCAN clustering parameters
    TuneClusters,
    /// Show failure patterns and optionally mine new ones from trajectories
    Patterns {
        /// Mine new patterns from failed trajectories
        #[arg(long)]
        mine: bool,
        /// Minimum occurrences to consider a mined pattern significant
        #[arg(long, default_value = "2")]
        min_occurrences: u32,
    },
    /// Show file co-edit dependencies
    Dependencies {
        /// File path to show dependencies for (omit for full graph)
        #[arg(long)]
        file: Option<String>,
        /// Max results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum AgentAction {
    /// List all loaded agents
    List,
    /// Show info about a specific agent
    Info { name: String },
    /// Search for agents
    Search { query: String },
}

#[derive(Subcommand)]
enum TmuxAction {
    /// Start the tmux monitor
    Start,
    /// Update the tmux display
    Update,
    /// Stop the tmux monitor
    Stop,
    /// Show current tmux state
    Status,
}

#[derive(Subcommand)]
enum McpAction {
    /// Start the MCP server (JSON-RPC over stdio)
    Serve,
}

#[derive(Subcommand)]
enum WorkAction {
    /// Create a new work item
    Create {
        /// Title of the work item (positional or --title)
        #[arg(value_name = "TITLE")]
        title: String,
        /// Type: task, epic, bug, story, sub-task
        #[arg(long, default_value = "task")]
        r#type: String,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Parent work item ID
        #[arg(long)]
        parent: Option<String>,
        /// Priority (0=critical, 1=high, 2=normal, 3=low)
        #[arg(long, default_value_t = 2)]
        priority: i32,
    },
    /// List work items
    List {
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Filter by type
        #[arg(long)]
        r#type: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a work item's status
    Update {
        /// Work item ID (prefix match supported)
        id: String,
        /// New status: pending, in_progress, blocked, completed
        #[arg(long)]
        status: String,
    },
    /// Close a work item
    Close {
        /// Work item ID (prefix match supported)
        id: String,
    },
    /// Sync with external backend
    Sync,
    /// Show work tracking status
    Status {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show work event audit trail
    Log {
        /// Max events to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Show events since date (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,
    },
    /// Claim a work item
    Claim {
        /// Work item ID
        id: String,
    },
    /// Release a claimed work item
    Release {
        /// Work item ID
        id: String,
    },
    /// List stealable work items
    Stealable,
    /// Steal a stealable work item
    Steal {
        /// Work item ID (steals highest priority if omitted)
        id: Option<String>,
    },
    /// Show work distribution across agents
    Load,
    /// Update heartbeat for a claimed work item
    Heartbeat {
        /// Work item ID (optional — updates all claimed items if omitted)
        id: Option<String>,
    },
    /// Show full details for a work item
    Get {
        /// Work item ID (prefix match supported)
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a work item
    Delete {
        /// Work item ID (prefix match supported)
        id: String,
    },
}

#[derive(Subcommand)]
enum MailboxAction {
    /// Send a message to co-agents
    Send {
        /// Work item ID (coordination hub)
        #[arg(long)]
        work_item: String,
        /// Sender agent name
        #[arg(long)]
        from: String,
        /// Optional target agent name (omit for broadcast)
        #[arg(long)]
        to: Option<String>,
        /// Message content
        message: String,
    },
    /// Read unread messages for the current session
    Read {
        #[arg(long)]
        session_id: Option<String>,
    },
    /// Show mailbox history for a work item
    History {
        /// Work item ID
        work_item_id: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// List agents on a work item
    Agents {
        /// Work item ID
        work_item_id: String,
    },
}

#[derive(Subcommand)]
enum ErrorAction {
    /// List known error patterns with occurrence counts
    List {
        /// Max results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Find resolutions for an error by text
    Find {
        /// Error text to search for
        error_text: String,
    },
    /// Show error recovery statistics
    Stats,
}

#[derive(Subcommand)]
enum GuidanceAction {
    /// List all guidance rules and gates
    Rules,
    /// Show trust score for current or specified session
    Trust {
        /// Session ID (defaults to current)
        #[arg(long)]
        session: Option<String>,
    },
    /// Show audit trail of gate decisions
    Audit {
        /// Session ID (defaults to current)
        #[arg(long)]
        session: Option<String>,
        /// Max results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Verify audit hash chain integrity
    Verify {
        /// Session ID (defaults to current)
        #[arg(long)]
        session: Option<String>,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Pretty-print the resolved configuration
    Show,
    /// Get a config value by dot-notation key (e.g., patterns.short_term_max)
    Get {
        /// Dot-notation key (e.g., guidance.enabled)
        key: String,
    },
    /// Set a config value by dot-notation key
    Set {
        /// Dot-notation key (e.g., patterns.short_term_max)
        key: String,
        /// New value
        value: String,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    /// List installed plugins
    List,
    /// Show plugin details
    Info {
        /// Plugin name
        name: String,
    },
    /// Enable a plugin
    Enable {
        /// Plugin name
        name: String,
    },
    /// Disable a plugin
    Disable {
        /// Plugin name
        name: String,
    },
}

fn main() {
    let cli = Cli::parse();

    // Only enable tracing for non-hook commands. Any stderr output during
    // hook execution causes Claude Code to display a hook error in the TUI.
    let is_hook = matches!(cli.command, Commands::Hook { .. });
    if is_hook {
        // Suppress default panic output to stderr — Claude Code treats any
        // stderr as a hook error. Our run_safe wrapper catches panics and
        // logs them to .flowforge/hook-errors.log instead.
        std::panic::set_hook(Box::new(|_| {}));
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
            )
            .with_writer(std::io::stderr)
            .init();
    }

    let result = match cli.command {
        Commands::Init { project, global } => commands::init::run(project, global),
        Commands::Hook { event } => match event {
            HookEvent::PreToolUse => hooks::run_safe("pre-tool-use", hooks::pre_tool_use::run),
            HookEvent::PostToolUse => hooks::run_safe("post-tool-use", hooks::post_tool_use::run),
            HookEvent::PostToolUseFailure => {
                hooks::run_safe("post-tool-use-failure", hooks::post_tool_use_failure::run)
            }
            HookEvent::Notification => hooks::run_safe("notification", hooks::notification::run),
            HookEvent::UserPromptSubmit => {
                hooks::run_safe("user-prompt-submit", hooks::user_prompt_submit::run)
            }
            HookEvent::SessionStart => hooks::run_safe("session-start", hooks::session_start::run),
            HookEvent::SessionEnd => hooks::run_safe("session-end", hooks::session_end::run),
            HookEvent::Stop => hooks::run_safe("stop", hooks::stop::run),
            HookEvent::PreCompact => hooks::run_safe("pre-compact", hooks::pre_compact::run),
            HookEvent::SubagentStart => {
                hooks::run_safe("subagent-start", hooks::subagent_start::run)
            }
            HookEvent::SubagentStop => hooks::run_safe("subagent-stop", hooks::subagent_stop::run),
            HookEvent::TeammateIdle => hooks::run_safe("teammate-idle", hooks::teammate_idle::run),
            HookEvent::TaskCompleted => {
                hooks::run_safe("task-completed", hooks::task_completed::run)
            }
        },
        Commands::Status { json } => commands::status::run(json),
        Commands::Memory { action } => match action {
            MemoryAction::Get { key, namespace } => commands::memory::get(&key, &namespace),
            MemoryAction::Set {
                key,
                value,
                namespace,
            } => commands::memory::set(&key, &value, &namespace),
            MemoryAction::Delete { key, namespace } => commands::memory::delete(&key, &namespace),
            MemoryAction::List { namespace } => commands::memory::list(&namespace),
            MemoryAction::Search { query, limit } => commands::memory::search(&query, limit),
        },
        Commands::Statusline { legend } => {
            if legend {
                commands::statusline::print_legend()
            } else {
                commands::statusline::run()
            }
        }
        Commands::Session { action } => match action {
            SessionAction::Current { json } => commands::session::current(json),
            SessionAction::List { limit } => commands::session::list(limit),
            SessionAction::Metrics => commands::session::metrics(),
            SessionAction::Agents { session_id } => {
                commands::session::agents(session_id.as_deref())
            }
            SessionAction::History {
                session_id,
                limit,
                offset,
            } => commands::session::history(session_id.as_deref(), limit, offset),
            SessionAction::Ingest { path, session_id } => {
                commands::session::ingest(&path, session_id.as_deref())
            }
            SessionAction::Checkpoint {
                name,
                session_id,
                description,
            } => {
                commands::session::checkpoint(&name, session_id.as_deref(), description.as_deref())
            }
            SessionAction::Checkpoints { session_id } => {
                commands::session::checkpoints(session_id.as_deref())
            }
            SessionAction::Fork {
                session_id,
                checkpoint,
                at_index,
                reason,
            } => commands::session::fork(
                session_id.as_deref(),
                checkpoint.as_deref(),
                at_index,
                reason.as_deref(),
            ),
            SessionAction::Forks { session_id } => commands::session::forks(session_id.as_deref()),
            SessionAction::HookTiming { session_id, json } => {
                commands::session::hook_timing(session_id.as_deref(), json)
            }
        },
        Commands::Learn { action } => match action {
            LearnAction::Store { content, category } => commands::learn::store(&content, &category),
            LearnAction::Search { query, limit } => commands::learn::search(&query, limit),
            LearnAction::Stats { json } => commands::learn::stats(json),
            LearnAction::Trajectories {
                session,
                status,
                limit,
            } => commands::learn::trajectories(session.as_deref(), status.as_deref(), limit),
            LearnAction::Trajectory { id } => commands::learn::trajectory(&id),
            LearnAction::Judge { id } => commands::learn::judge(&id),
            LearnAction::DownloadModel => commands::learn::download_model(),
            LearnAction::Clusters => commands::learn::clusters(),
            LearnAction::TuneClusters => commands::learn::tune_clusters(),
            LearnAction::Patterns {
                mine,
                min_occurrences,
            } => commands::learn::patterns(mine, min_occurrences),
            LearnAction::Dependencies { file, limit } => {
                commands::learn::dependencies(file.as_deref(), limit)
            }
        },
        Commands::Agent { action } => match action {
            AgentAction::List => commands::agent::list(),
            AgentAction::Info { name } => commands::agent::info(&name),
            AgentAction::Search { query } => commands::agent::search(&query),
        },
        Commands::Route { task } => commands::route::run(&task),
        Commands::Tmux { action } => match action {
            TmuxAction::Start => commands::tmux::start(),
            TmuxAction::Update => commands::tmux::update(),
            TmuxAction::Stop => commands::tmux::stop(),
            TmuxAction::Status => commands::tmux::status(),
        },
        Commands::Mcp { action } => match action {
            McpAction::Serve => commands::mcp::serve(),
        },
        Commands::Work { action } => match action {
            WorkAction::Create {
                title,
                r#type,
                description,
                parent,
                priority,
            } => commands::work::create(
                &r#type,
                &title,
                description.as_deref(),
                parent.as_deref(),
                priority,
            ),
            WorkAction::List { status, r#type, json } => {
                commands::work::list(status.as_deref(), r#type.as_deref(), json)
            }
            WorkAction::Update { id, status } => commands::work::update(&id, &status),
            WorkAction::Close { id } => commands::work::close(&id),
            WorkAction::Sync => commands::work::sync(),
            WorkAction::Status { json } => commands::work::status(json),
            WorkAction::Log { limit, since } => commands::work::log(limit, since.as_deref()),
            WorkAction::Claim { id } => commands::work::claim(&id),
            WorkAction::Release { id } => commands::work::release(&id),
            WorkAction::Stealable => commands::work::stealable(),
            WorkAction::Steal { id } => commands::work::steal(id.as_deref()),
            WorkAction::Load => commands::work::load(),
            WorkAction::Heartbeat { id } => commands::work::heartbeat(id.as_deref()),
            WorkAction::Get { id, json } => commands::work::get(&id, json),
            WorkAction::Delete { id } => commands::work::delete(&id),
        },
        Commands::Mailbox { action } => match action {
            MailboxAction::Send {
                work_item,
                from,
                to,
                message,
            } => commands::mailbox::send(&work_item, &from, to.as_deref(), &message),
            MailboxAction::Read { session_id } => commands::mailbox::read(session_id.as_deref()),
            MailboxAction::History {
                work_item_id,
                limit,
            } => commands::mailbox::history(&work_item_id, limit),
            MailboxAction::Agents { work_item_id } => commands::mailbox::agents(&work_item_id),
        },
        Commands::Error { action } => match action {
            ErrorAction::List { limit } => commands::error::list(limit),
            ErrorAction::Find { error_text } => commands::error::find(&error_text),
            ErrorAction::Stats => commands::error::stats(),
        },
        Commands::Guidance { action } => match action {
            GuidanceAction::Rules => commands::guidance::rules(),
            GuidanceAction::Trust { session } => commands::guidance::trust(session.as_deref()),
            GuidanceAction::Audit { session, limit } => {
                commands::guidance::audit(session.as_deref(), limit)
            }
            GuidanceAction::Verify { session } => commands::guidance::verify(session.as_deref()),
        },
        Commands::Config { action } => match action {
            ConfigAction::Show => commands::config::show(),
            ConfigAction::Get { key } => commands::config::get(&key),
            ConfigAction::Set { key, value } => commands::config::set(&key, &value),
        },
        Commands::Plugin { action } => match action {
            PluginAction::List => commands::plugin::list(),
            PluginAction::Info { name } => commands::plugin::info(&name),
            PluginAction::Enable { name } => commands::plugin::enable(&name),
            PluginAction::Disable { name } => commands::plugin::disable(&name),
        },
        Commands::TestHooks { event, verbose } => {
            commands::test_hooks::run(event.as_deref(), verbose)
        }
    };

    if let Err(e) = result {
        if !is_hook {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}
