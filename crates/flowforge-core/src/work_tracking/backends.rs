//! Work tracking backend implementations: Kanbus, Beads, and backend resolution.

use std::path::{Path, PathBuf};

use tracing::warn;

use crate::config::WorkTrackingConfig;
use crate::types::{WorkItem, WorkStatus};
use crate::Result;

use super::claude_tasks::sync_to_claude_tasks;
use super::WorkDb;

/// Run a closure with stderr suppressed (redirected to /dev/null).
/// Kanbus's `publish_notification` uses `eprintln!` for non-critical warnings
/// when the console server isn't running. This suppresses those warnings.
#[cfg(unix)]
fn with_stderr_suppressed<T, F: FnOnce() -> T>(f: F) -> T {
    use std::os::unix::io::AsRawFd;
    let devnull = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/null")
        .ok();
    let saved_fd = devnull.as_ref().map(|dn| unsafe {
        let saved = libc::dup(2);
        libc::dup2(dn.as_raw_fd(), 2);
        saved
    });
    let result = f();
    if let Some(fd) = saved_fd {
        unsafe {
            libc::dup2(fd, 2);
            libc::close(fd);
        }
    }
    result
}

#[cfg(not(unix))]
fn with_stderr_suppressed<T, F: FnOnce() -> T>(f: F) -> T {
    f()
}

// ── WorkBackend trait ──

/// Internal trait for external work-tracking backends (kanbus, beads).
/// Claude Tasks is NOT a backend — it's an unconditional dual-write side-effect.
pub(crate) trait WorkBackend {
    /// Create an item in the external backend, returning its external ID if available.
    fn create(&self, item: &WorkItem) -> Result<Option<String>>;
    /// Update an item's status in the external backend.
    fn update_status(&self, external_id: &str, status: &str) -> Result<()>;
    /// Update an item with full field sync (title, description, assignee, priority, labels).
    /// Default: delegates to update_status for backwards compatibility.
    fn update_item(&self, external_id: &str, item: &WorkItem) -> Result<()> {
        self.update_status(external_id, &item.status.to_string())
    }
    /// Add a comment to an item in the external backend.
    /// Default: no-op (not all backends support comments).
    fn add_comment(&self, _external_id: &str, _author: &str, _text: &str) -> Result<()> {
        Ok(())
    }
    /// Pull items from the external backend into FlowForge SQLite. Returns count synced.
    fn sync_inbound(&self, db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32>;
}

// ── KanbusBackend ──

pub(crate) struct KanbusBackend {
    root: PathBuf,
}

impl KanbusBackend {
    pub(crate) fn new(config: &WorkTrackingConfig) -> Self {
        let root = config
            .kanbus
            .root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Self { root }
    }
}

impl WorkBackend for KanbusBackend {
    fn create(&self, item: &WorkItem) -> Result<Option<String>> {
        let request = kanbus::issue_creation::IssueCreationRequest {
            root: self.root.clone(),
            title: item.title.clone(),
            issue_type: Some(item.item_type.clone()),
            priority: Some(item.priority.clamp(1, 4) as u8),
            assignee: item.assignee.clone(),
            parent: item.parent_id.clone(),
            labels: item.labels.clone(),
            description: item.description.clone(),
            local: false,
            validate: false,
        };

        match with_stderr_suppressed(|| kanbus::issue_creation::create_issue(&request)) {
            Ok(result) => Ok(Some(result.issue.identifier)),
            Err(e) => {
                warn!("kanbus create failed: {e}");
                Ok(None)
            }
        }
    }

    fn update_status(&self, external_id: &str, status: &str) -> Result<()> {
        if status == "completed" {
            if let Err(e) =
                with_stderr_suppressed(|| kanbus::issue_close::close_issue(&self.root, external_id))
            {
                warn!("kanbus close failed: {e}");
            }
            return Ok(());
        }

        let kanbus_status = match status {
            "pending" => "open",
            "in_progress" => "in_progress",
            "blocked" => "blocked",
            other => other,
        };

        if let Err(e) = with_stderr_suppressed(|| {
            kanbus::issue_update::update_issue(
                &self.root,
                external_id,
                None,                // title
                None,                // description
                Some(kanbus_status), // status
                None,                // assignee
                None,                // priority
                false,               // claim
                false,               // validate
                &[],                 // add_labels
                &[],                 // remove_labels
                None,                // set_labels
                None,                // parent
            )
        }) {
            warn!("kanbus status update failed: {e}");
        }
        Ok(())
    }

    fn update_item(&self, external_id: &str, item: &WorkItem) -> Result<()> {
        if item.status == WorkStatus::Completed {
            if let Err(e) =
                with_stderr_suppressed(|| kanbus::issue_close::close_issue(&self.root, external_id))
            {
                warn!("kanbus close failed: {e}");
            }
            return Ok(());
        }

        let kanbus_status = match item.status {
            WorkStatus::Pending => "open",
            WorkStatus::InProgress => "in_progress",
            WorkStatus::Blocked => "blocked",
            WorkStatus::Completed => unreachable!(),
        };

        let root = self.root.clone();
        let ext_id = external_id.to_string();
        let title = item.title.clone();
        let description = item.description.clone();
        let assignee = item.assignee.clone();
        let priority = item.priority.clamp(1, 4) as u8;
        let parent = item.parent_id.clone();

        if let Err(e) = with_stderr_suppressed(move || {
            kanbus::issue_update::update_issue(
                &root,
                &ext_id,
                Some(&title),
                description.as_deref(),
                Some(kanbus_status),
                assignee.as_deref(),
                Some(priority),
                false,
                false,
                &[],
                &[],
                None,
                parent.as_deref(),
            )
        }) {
            warn!("kanbus item update failed: {e}");
        }
        Ok(())
    }

    fn add_comment(&self, external_id: &str, author: &str, text: &str) -> Result<()> {
        let root = self.root.clone();
        let ext_id = external_id.to_string();
        let author = author.to_string();
        let text = text.to_string();
        if let Err(e) = with_stderr_suppressed(move || {
            kanbus::issue_comment::add_comment(&root, &ext_id, &author, &text)
        }) {
            warn!("kanbus comment failed: {e}");
        }
        Ok(())
    }

    fn sync_inbound(&self, db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
        let issues = match with_stderr_suppressed(|| {
            kanbus::issue_listing::list_issues(
                &self.root,
                None,  // status (all)
                None,  // issue_type
                None,  // assignee
                None,  // label
                None,  // sort
                None,  // search
                &[],   // project_filter
                false, // include_local
                false, // local_only
            )
        }) {
            Ok(issues) => issues,
            Err(e) => {
                warn!("kanbus list failed: {e}");
                return Ok(0);
            }
        };

        let mut synced = 0u32;
        let now = chrono::Utc::now();

        for issue in &issues {
            let ext_id = &issue.identifier;
            if db.get_work_item_by_external_id(ext_id)?.is_some() {
                continue;
            }

            let status: WorkStatus = issue.status.parse().unwrap_or(WorkStatus::Pending);

            let priority = issue.priority.clamp(1, 4);

            let work_item = WorkItem {
                id: uuid::Uuid::new_v4().to_string(),
                external_id: Some(ext_id.to_string()),
                backend: "kanbus".to_string(),
                item_type: issue.issue_type.clone(),
                title: issue.title.clone(),
                description: if issue.description.is_empty() {
                    None
                } else {
                    Some(issue.description.clone())
                },
                status,
                assignee: issue.assignee.clone(),
                parent_id: issue.parent.clone(),
                priority,
                labels: issue.labels.clone(),
                created_at: now,
                updated_at: now,
                completed_at: if status == WorkStatus::Completed {
                    Some(now)
                } else {
                    None
                },
                session_id: None,
                metadata: None,
                claimed_by: None,
                claimed_at: None,
                last_heartbeat: None,
                progress: 0,
                stealable: false,
            };

            db.create_work_item(&work_item)?;
            let _ = sync_to_claude_tasks(&work_item, config);
            synced += 1;
        }

        Ok(synced)
    }
}

// ── BeadsBackend ──

pub(crate) struct BeadsBackend;

impl WorkBackend for BeadsBackend {
    fn create(&self, item: &WorkItem) -> Result<Option<String>> {
        let mut cmd = std::process::Command::new("bd");
        cmd.arg("create").arg(&item.title);

        if let Some(ref desc) = item.description {
            cmd.arg("--description").arg(desc);
        }

        match cmd.output() {
            Ok(o) if !o.status.success() => {
                warn!("bd create failed: {}", String::from_utf8_lossy(&o.stderr));
            }
            Err(e) => warn!("bd not available: {e}"),
            _ => {}
        }
        Ok(None)
    }

    fn update_status(&self, external_id: &str, status: &str) -> Result<()> {
        let result = match status {
            "completed" => std::process::Command::new("bd")
                .arg("close")
                .arg(external_id)
                .output(),
            _ => std::process::Command::new("bd")
                .arg("update")
                .arg(external_id)
                .arg("--status")
                .arg(status)
                .output(),
        };
        if let Err(e) = result {
            warn!("bd status update failed: {e}");
        }
        Ok(())
    }

    fn update_item(&self, external_id: &str, item: &WorkItem) -> Result<()> {
        if item.status == WorkStatus::Completed {
            let result = std::process::Command::new("bd")
                .arg("close")
                .arg(external_id)
                .output();
            if let Err(e) = result {
                warn!("bd close failed: {e}");
            }
            return Ok(());
        }

        let mut cmd = std::process::Command::new("bd");
        cmd.arg("update").arg(external_id);
        cmd.arg("--title").arg(&item.title);
        cmd.arg("--status").arg(item.status.to_string());
        cmd.arg("--priority")
            .arg(item.priority.clamp(0, 4).to_string());
        if let Some(ref desc) = item.description {
            cmd.arg("--description").arg(desc);
        }
        if let Some(ref assignee) = item.assignee {
            cmd.arg("--assignee").arg(assignee);
        }
        if let Some(ref parent) = item.parent_id {
            cmd.arg("--parent").arg(parent);
        }

        match cmd.output() {
            Ok(o) if !o.status.success() => {
                warn!("bd update failed: {}", String::from_utf8_lossy(&o.stderr));
            }
            Err(e) => warn!("bd update failed: {e}"),
            _ => {}
        }
        Ok(())
    }

    fn add_comment(&self, external_id: &str, _author: &str, text: &str) -> Result<()> {
        match std::process::Command::new("bd")
            .arg("comments")
            .arg("add")
            .arg(external_id)
            .arg(text)
            .output()
        {
            Ok(o) if !o.status.success() => {
                warn!("bd comment failed: {}", String::from_utf8_lossy(&o.stderr));
            }
            Err(e) => warn!("bd comment failed: {e}"),
            _ => {}
        }
        Ok(())
    }

    fn sync_inbound(&self, db: &dyn WorkDb, _config: &WorkTrackingConfig) -> Result<u32> {
        let beads_file = Path::new(".beads/issues.jsonl");
        if !beads_file.exists() {
            return Ok(0);
        }

        let content = match std::fs::read_to_string(beads_file) {
            Ok(c) => c,
            Err(_) => return Ok(0),
        };

        let mut synced = 0u32;
        let now = chrono::Utc::now();

        for line in content.lines() {
            let item: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let ext_id = item["id"].as_str().unwrap_or_default();
            if ext_id.is_empty() {
                continue;
            }

            if db.get_work_item_by_external_id(ext_id)?.is_some() {
                continue;
            }

            let status: WorkStatus = item["status"]
                .as_str()
                .unwrap_or("open")
                .parse()
                .unwrap_or(WorkStatus::Pending);

            let work_item = WorkItem {
                id: uuid::Uuid::new_v4().to_string(),
                external_id: Some(ext_id.to_string()),
                backend: "beads".to_string(),
                item_type: "task".to_string(),
                title: item["title"].as_str().unwrap_or("(untitled)").to_string(),
                description: item["body"].as_str().map(|s| s.to_string()),
                status,
                assignee: None,
                parent_id: None,
                priority: 2,
                labels: vec![],
                created_at: now,
                updated_at: now,
                completed_at: None,
                session_id: None,
                metadata: None,
                claimed_by: None,
                claimed_at: None,
                last_heartbeat: None,
                progress: 0,
                stealable: false,
            };

            db.create_work_item(&work_item)?;
            synced += 1;
        }

        Ok(synced)
    }
}

// ── Backend resolution ──

/// Resolve the active backend name and trait object.
/// Returns ("backend_name", Some(impl)) for kanbus/beads, or ("name", None) for others.
pub(crate) fn resolve_backend(config: &WorkTrackingConfig) -> (&str, Option<Box<dyn WorkBackend>>) {
    let name = detect_backend(config);
    match name {
        "kanbus" => (name, Some(Box::new(KanbusBackend::new(config)))),
        "beads" => (name, Some(Box::new(BeadsBackend))),
        other => (other, None),
    }
}

/// Detect which work tracking backend is active.
pub fn detect_backend(config: &WorkTrackingConfig) -> &str {
    if config.backend != "auto" {
        return &config.backend;
    }

    // Check for Kanbus
    if Path::new(".kanbus.yml").exists() || Path::new(".kanbus").exists() {
        return "kanbus";
    }

    // Check for Beads
    if Path::new(".beads").exists() {
        return "beads";
    }

    // Check for Claude Tasks via environment
    if std::env::var("CLAUDE_CODE_TASK_LIST_ID").is_ok() {
        return "claude_tasks";
    }

    // Check for Claude Tasks directory
    let home = dirs::home_dir().unwrap_or_default();
    if home.join(".claude/tasks").exists() {
        return "claude_tasks";
    }

    "flowforge"
}
