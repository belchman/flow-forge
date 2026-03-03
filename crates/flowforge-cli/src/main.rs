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
    Status,
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
    Statusline,
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
    Current,
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
    Stats,
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
        /// Title of the work item
        #[arg(long)]
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
    Status,
    /// Show work event audit trail
    Log {
        /// Max events to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Show events since date (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,
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

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

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
        Commands::Status => commands::status::run(),
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
        Commands::Statusline => commands::statusline::run(),
        Commands::Session { action } => match action {
            SessionAction::Current => commands::session::current(),
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
        },
        Commands::Learn { action } => match action {
            LearnAction::Store { content, category } => commands::learn::store(&content, &category),
            LearnAction::Search { query, limit } => commands::learn::search(&query, limit),
            LearnAction::Stats => commands::learn::stats(),
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
            WorkAction::List { status, r#type } => {
                commands::work::list(status.as_deref(), r#type.as_deref())
            }
            WorkAction::Update { id, status } => commands::work::update(&id, &status),
            WorkAction::Close { id } => commands::work::close(&id),
            WorkAction::Sync => commands::work::sync(),
            WorkAction::Status => commands::work::status(),
            WorkAction::Log { limit, since } => commands::work::log(limit, since.as_deref()),
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
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
