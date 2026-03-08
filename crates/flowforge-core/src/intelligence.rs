use std::collections::HashMap;
use std::path::Path;

use crate::project_detection::ProjectProfile;

/// A code index entry stub for generation purposes.
/// Mirrors `flowforge_memory::CodeIndexEntry` fields we need.
#[derive(Debug, Clone)]
pub struct CodeEntry {
    pub file_path: String,
    pub language: String,
    pub size_bytes: i64,
    pub symbols: Vec<String>,
    pub description: String,
}

/// File co-edit pair from the DB.
#[derive(Debug, Clone)]
pub struct CoEditPair {
    pub file_a: String,
    pub file_b: String,
    pub co_edit_count: i64,
}

/// Error hotspot from the DB.
#[derive(Debug, Clone)]
pub struct ErrorHotspot {
    pub tool_name: String,
    pub category: String,
    pub error_preview: String,
    pub occurrence_count: i64,
    pub has_resolution: bool,
}

/// Test co-occurrence from the DB.
#[derive(Debug, Clone)]
pub struct TestCoOccurrence {
    pub edited_file: String,
    pub test_file: String,
    pub occurrence_count: i64,
}

/// DB-sourced data that the generator needs. Callers fill this from MemoryDb.
#[derive(Debug, Clone, Default)]
pub struct IntelligenceData {
    pub code_entries: Vec<CodeEntry>,
    pub co_edit_pairs: Vec<CoEditPair>,
    pub error_hotspots: Vec<ErrorHotspot>,
    pub test_co_occurrences: Vec<TestCoOccurrence>,
}

/// Generates 12 intelligence sections from a project profile and code index data.
pub struct IntelligenceGenerator<'a> {
    project_dir: &'a Path,
    profile: &'a ProjectProfile,
    data: &'a IntelligenceData,
}

impl<'a> IntelligenceGenerator<'a> {
    pub fn new(
        project_dir: &'a Path,
        profile: &'a ProjectProfile,
        data: &'a IntelligenceData,
    ) -> Self {
        Self {
            project_dir,
            profile,
            data,
        }
    }

    /// Generate all 12 sections: (key, title, content, confidence).
    pub fn generate_all(&self) -> Vec<(String, String, String, f64)> {
        vec![
            self.gen_overview(),
            self.gen_folder_structure(),
            self.gen_conventions(),
            self.gen_do_not_change(),
            self.gen_api_formats(),
            self.gen_business_logic(),
            self.gen_dependency_graph(),
            self.gen_error_hotspots(),
            self.gen_test_coverage(),
            self.gen_entry_points(),
            self.gen_build_deploy(),
            self.gen_env_catalog(),
        ]
    }

    fn gen_overview(&self) -> (String, String, String, f64) {
        let p = self.profile;
        let file_count = self.data.code_entries.len();
        let mut lines = Vec::new();
        lines.push(format!("**Project:** {}", p.name));
        lines.push(format!("**Type:** {}", p.project_type));
        if !p.languages.is_empty() {
            lines.push(format!("**Languages:** {}", p.languages.join(", ")));
        }
        if !p.frameworks.is_empty() {
            lines.push(format!("**Frameworks:** {}", p.frameworks.join(", ")));
        }
        lines.push(format!("**Indexed files:** {}", file_count));
        lines.push(String::new());
        lines.push("<!-- TODO: Add a plain-English description of what this project does -->".to_string());

        ("overview".into(), "Project Overview".into(), lines.join("\n"), 0.3)
    }

    fn gen_folder_structure(&self) -> (String, String, String, f64) {
        let skip = &[
            ".git", "target", "node_modules", "vendor", "dist", "build",
            ".next", "__pycache__", ".venv", "venv", ".flowforge",
        ];

        let mut lines = Vec::new();
        lines.push("```".to_string());

        if let Ok(entries) = std::fs::read_dir(self.project_dir) {
            let mut dirs: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    e.path().is_dir() && !skip.contains(&name.as_str())
                })
                .collect();
            dirs.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

            for dir in &dirs {
                let name = dir.file_name().to_string_lossy().to_string();
                let file_count = count_files_recursive(&dir.path(), 2);
                let desc = self.infer_dir_purpose(&name);
                lines.push(format!("{name}/ ({file_count} files) — {desc}"));

                // One level deeper
                if let Ok(sub_entries) = std::fs::read_dir(dir.path()) {
                    let mut subdirs: Vec<_> = sub_entries
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            let n = e.file_name().to_string_lossy().to_string();
                            e.path().is_dir() && !skip.contains(&n.as_str())
                        })
                        .collect();
                    subdirs.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

                    for sub in subdirs.iter().take(8) {
                        let sub_name = sub.file_name().to_string_lossy().to_string();
                        let sub_count = count_files_recursive(&sub.path(), 1);
                        lines.push(format!("  {sub_name}/ ({sub_count} files)"));
                    }
                }
            }
        }

        lines.push("```".to_string());
        ("folder_structure".into(), "Folder Structure".into(), lines.join("\n"), 0.7)
    }

    fn infer_dir_purpose(&self, name: &str) -> String {
        // Check code_index entries in this directory for descriptions
        let matching: Vec<&CodeEntry> = self
            .data
            .code_entries
            .iter()
            .filter(|e| {
                e.file_path
                    .starts_with(&format!("{}/", name))
                    || e.file_path.starts_with(&format!("./{}/", name))
            })
            .collect();

        if !matching.is_empty() {
            // Pick the most descriptive one
            if let Some(best) = matching.iter().find(|e| !e.description.is_empty()) {
                let desc: String = best.description.chars().take(60).collect();
                return desc;
            }
        }

        // Fallback heuristics
        match name {
            "src" => "Source code".into(),
            "tests" | "test" | "__tests__" | "spec" => "Test files".into(),
            "docs" | "doc" => "Documentation".into(),
            "scripts" | "bin" => "Scripts and executables".into(),
            "config" | "configs" | "conf" => "Configuration files".into(),
            "lib" | "libs" => "Libraries".into(),
            "pkg" | "packages" => "Packages".into(),
            "cmd" => "Command entry points".into(),
            "internal" => "Internal packages".into(),
            "api" => "API definitions".into(),
            "crates" => "Rust workspace crates".into(),
            "agents" => "Agent definitions".into(),
            "migrations" | "db" => "Database migrations".into(),
            "public" | "static" | "assets" => "Static assets".into(),
            "components" => "UI components".into(),
            "pages" | "views" => "Page/view templates".into(),
            "models" => "Data models".into(),
            "services" => "Service layer".into(),
            "utils" | "helpers" => "Utility functions".into(),
            ".github" => "GitHub Actions & config".into(),
            _ => "Project directory".into(),
        }
    }

    fn gen_conventions(&self) -> (String, String, String, f64) {
        let mut lines = Vec::new();
        let entries = &self.data.code_entries;

        // Detect naming convention from symbols
        let mut snake_count = 0u64;
        let mut camel_count = 0u64;
        for entry in entries {
            for sym in &entry.symbols {
                if sym.contains('_') && sym.to_lowercase() == *sym {
                    snake_count += 1;
                } else if sym.len() > 1
                    && sym.chars().next().map(|c| c.is_lowercase()).unwrap_or(false)
                    && sym.chars().any(|c| c.is_uppercase())
                {
                    camel_count += 1;
                }
            }
        }

        if snake_count > 0 || camel_count > 0 {
            let convention = if snake_count > camel_count * 2 {
                "snake_case (dominant)"
            } else if camel_count > snake_count * 2 {
                "camelCase (dominant)"
            } else {
                "mixed (snake_case + camelCase)"
            };
            lines.push(format!("**Naming:** {convention}"));
        }

        // Detect error handling style from languages
        let p = self.profile;
        if p.languages.contains(&"rust".to_string()) {
            lines.push("**Error handling:** Result<T, E> pattern (Rust)".to_string());
        }
        if p.languages.contains(&"typescript".to_string())
            || p.languages.contains(&"javascript".to_string())
        {
            lines.push("**Error handling:** try/catch + async/await".to_string());
        }
        if p.languages.contains(&"python".to_string()) {
            lines.push("**Error handling:** try/except".to_string());
        }
        if p.languages.contains(&"go".to_string()) {
            lines.push("**Error handling:** if err != nil pattern (Go)".to_string());
        }

        lines.push(String::new());
        lines.push(
            "<!-- TODO: Add project-specific conventions (module organization, import patterns, testing style) -->"
                .to_string(),
        );

        ("conventions".into(), "Coding Conventions".into(), lines.join("\n"), 0.4)
    }

    fn gen_do_not_change(&self) -> (String, String, String, f64) {
        let mut lines = Vec::new();
        let dir = self.project_dir;

        // Lock files
        for lock in &[
            "Cargo.lock",
            "package-lock.json",
            "yarn.lock",
            "pnpm-lock.yaml",
            "go.sum",
            "Gemfile.lock",
            "poetry.lock",
        ] {
            if dir.join(lock).exists() {
                lines.push(format!("- **{lock}** — Auto-generated lock file, do not manually edit"));
            }
        }

        // Migration directories
        for mig_dir in &["migrations", "db/migrate", "prisma/migrations"] {
            if dir.join(mig_dir).is_dir() {
                lines.push(format!(
                    "- **{mig_dir}/** — Database migrations, never modify existing migrations"
                ));
            }
        }

        // CI/CD configs
        if dir.join(".github/workflows").is_dir() {
            lines.push(
                "- **.github/workflows/** — CI/CD pipelines, changes can break builds".to_string(),
            );
        }

        // Generated code markers: scan a sample of files
        // (We just note the pattern, not scan everything)
        lines.push(String::new());
        lines.push(
            "<!-- TODO: Add project-specific files/dirs that should not be modified (generated code, vendored deps, etc.) -->"
                .to_string(),
        );

        ("do_not_change".into(), "DO NOT CHANGE".into(), lines.join("\n"), 0.2)
    }

    fn gen_api_formats(&self) -> (String, String, String, f64) {
        let mut lines = Vec::new();

        if !self.profile.has_api {
            lines.push("No API routes detected.".to_string());
            lines.push(String::new());
            lines.push("<!-- TODO: Document API endpoints and response formats if this project has an API -->".to_string());
            return ("api_formats".into(), "API & Response Formats".into(), lines.join("\n"), 0.2);
        }

        // List detected frameworks
        for fw in &self.profile.frameworks {
            lines.push(format!("**Framework:** {fw}"));
        }
        lines.push(String::new());

        // Look for route-like symbols in code entries
        let route_keywords = &["route", "handler", "endpoint", "controller", "api", "router"];
        let route_files: Vec<&CodeEntry> = self
            .data
            .code_entries
            .iter()
            .filter(|e| {
                let path_lower = e.file_path.to_lowercase();
                route_keywords.iter().any(|kw| path_lower.contains(kw))
                    || e.symbols.iter().any(|s| {
                        let sl = s.to_lowercase();
                        route_keywords.iter().any(|kw| sl.contains(kw))
                    })
            })
            .take(10)
            .collect();

        if !route_files.is_empty() {
            lines.push("**Route files:**".to_string());
            for f in &route_files {
                let syms: Vec<&str> = f.symbols.iter().take(5).map(|s| s.as_str()).collect();
                lines.push(format!("- `{}` — {}", f.file_path, syms.join(", ")));
            }
        }

        lines.push(String::new());
        lines.push("<!-- TODO: List specific endpoints (method + path) and response format conventions -->".to_string());

        ("api_formats".into(), "API & Response Formats".into(), lines.join("\n"), 0.5)
    }

    fn gen_business_logic(&self) -> (String, String, String, f64) {
        let mut lines = Vec::new();

        // Group code entries by top-level directory
        let mut by_dir: HashMap<String, Vec<&CodeEntry>> = HashMap::new();
        for entry in &self.data.code_entries {
            let dir = entry
                .file_path
                .split('/')
                .next()
                .unwrap_or("root")
                .to_string();
            by_dir.entry(dir).or_default().push(entry);
        }

        // Sort groups by total symbol count (descending = most complex first)
        let mut groups: Vec<_> = by_dir.into_iter().collect();
        groups.sort_by(|a, b| {
            let count_a: usize = a.1.iter().map(|e| e.symbols.len()).sum();
            let count_b: usize = b.1.iter().map(|e| e.symbols.len()).sum();
            count_b.cmp(&count_a)
        });

        for (dir, entries) in groups.iter().take(6) {
            let total_symbols: usize = entries.iter().map(|e| e.symbols.len()).sum();
            lines.push(format!("### {dir}/ ({total_symbols} symbols, {} files)", entries.len()));

            // Top files by symbol count
            let mut sorted = entries.clone();
            sorted.sort_by(|a, b| b.symbols.len().cmp(&a.symbols.len()));

            for entry in sorted.iter().take(5) {
                let top_syms: Vec<&str> = entry.symbols.iter().take(5).map(|s| s.as_str()).collect();
                let desc = if entry.description.is_empty() {
                    String::new()
                } else {
                    let d: String = entry.description.chars().take(60).collect();
                    format!(" — {d}")
                };
                lines.push(format!(
                    "- `{}` ({} symbols){}: {}",
                    entry.file_path,
                    entry.symbols.len(),
                    desc,
                    top_syms.join(", ")
                ));
            }
            lines.push(String::new());
        }

        lines.push("<!-- TODO: Explain what each major module/directory does in plain English -->".to_string());

        ("business_logic".into(), "Key Business Logic".into(), lines.join("\n"), 0.3)
    }

    fn gen_dependency_graph(&self) -> (String, String, String, f64) {
        let pairs = &self.data.co_edit_pairs;
        let mut lines = Vec::new();

        if pairs.is_empty() {
            lines.push("No file co-edit data yet. This section populates as you edit files across sessions.".to_string());
            return (
                "dependency_graph".into(),
                "File Dependency Graph".into(),
                lines.join("\n"),
                0.3,
            );
        }

        lines.push("Files commonly edited together (higher count = stronger coupling):".to_string());
        lines.push(String::new());

        for pair in pairs.iter().take(20) {
            lines.push(format!(
                "- `{}` ↔ `{}` ({}x)",
                pair.file_a, pair.file_b, pair.co_edit_count
            ));
        }

        ("dependency_graph".into(), "File Dependency Graph".into(), lines.join("\n"), 0.8)
    }

    fn gen_error_hotspots(&self) -> (String, String, String, f64) {
        let hotspots = &self.data.error_hotspots;
        let mut lines = Vec::new();

        if hotspots.is_empty() {
            lines.push("No error data yet. This section populates as errors are encountered.".to_string());
            return (
                "error_hotspots".into(),
                "Error Hotspots".into(),
                lines.join("\n"),
                0.3,
            );
        }

        lines.push("Most frequent errors by tool/category:".to_string());
        lines.push(String::new());

        for hs in hotspots.iter().take(10) {
            let resolved = if hs.has_resolution { " ✓ resolved" } else { "" };
            let preview: String = hs.error_preview.chars().take(80).collect();
            lines.push(format!(
                "- **{}** [{}] ({}x{}) — {}",
                hs.tool_name, hs.category, hs.occurrence_count, resolved, preview
            ));
        }

        ("error_hotspots".into(), "Error Hotspots".into(), lines.join("\n"), 0.7)
    }

    fn gen_test_coverage(&self) -> (String, String, String, f64) {
        let test_map = &self.data.test_co_occurrences;
        let mut lines = Vec::new();

        if test_map.is_empty() {
            lines.push("No test co-occurrence data yet. This section populates as you edit source + test files together.".to_string());
            return (
                "test_coverage".into(),
                "Test Coverage Map".into(),
                lines.join("\n"),
                0.3,
            );
        }

        lines.push("Source → Test file mapping (from co-edit patterns):".to_string());
        lines.push(String::new());

        for tc in test_map.iter().take(20) {
            lines.push(format!(
                "- `{}` → `{}` ({}x)",
                tc.edited_file, tc.test_file, tc.occurrence_count
            ));
        }

        ("test_coverage".into(), "Test Coverage Map".into(), lines.join("\n"), 0.8)
    }

    fn gen_entry_points(&self) -> (String, String, String, f64) {
        let p = self.profile;
        let mut lines = Vec::new();

        if !p.entry_points.is_empty() {
            lines.push("**Entry points:**".to_string());
            for ep in &p.entry_points {
                lines.push(format!("- `{ep}`"));
            }
            lines.push(String::new());
        }

        if !p.config_files.is_empty() {
            lines.push("**Config files:**".to_string());
            for cf in &p.config_files {
                lines.push(format!("- `{cf}`"));
            }
            lines.push(String::new());
        }

        if !p.test_dirs.is_empty() {
            lines.push("**Test directories:**".to_string());
            for td in &p.test_dirs {
                lines.push(format!("- `{td}/`"));
            }
        }

        ("entry_points".into(), "Entry Points & Config".into(), lines.join("\n"), 0.6)
    }

    fn gen_build_deploy(&self) -> (String, String, String, f64) {
        let p = self.profile;
        let mut lines = Vec::new();

        if !p.build_commands.is_empty() {
            lines.push("**Build:**".to_string());
            for cmd in &p.build_commands {
                lines.push(format!("- `{cmd}`"));
            }
            lines.push(String::new());
        }

        if !p.test_commands.is_empty() {
            lines.push("**Test:**".to_string());
            for cmd in &p.test_commands {
                lines.push(format!("- `{cmd}`"));
            }
            lines.push(String::new());
        }

        // Check for Makefile targets
        let makefile = self.project_dir.join("Makefile");
        if makefile.exists() {
            if let Ok(content) = std::fs::read_to_string(&makefile) {
                let targets: Vec<&str> = content
                    .lines()
                    .filter(|l| {
                        !l.starts_with('\t')
                            && !l.starts_with('#')
                            && !l.starts_with(' ')
                            && l.contains(':')
                            && !l.contains(":=")
                    })
                    .filter_map(|l| l.split(':').next())
                    .filter(|t| !t.contains('/') && !t.starts_with('.'))
                    .take(10)
                    .collect();
                if !targets.is_empty() {
                    lines.push("**Makefile targets:**".to_string());
                    for t in targets {
                        lines.push(format!("- `make {t}`"));
                    }
                }
            }
        }

        lines.push(String::new());
        lines.push("<!-- TODO: Add deploy commands and infrastructure details -->".to_string());

        ("build_deploy".into(), "Build & Deploy Commands".into(), lines.join("\n"), 0.5)
    }

    fn gen_env_catalog(&self) -> (String, String, String, f64) {
        let mut vars: Vec<(String, String)> = Vec::new();
        let dir = self.project_dir;

        // Scan .env.example / .env.sample
        for env_file in &[".env.example", ".env.sample", ".env.template"] {
            let path = dir.join(env_file);
            if path.exists() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }
                        if let Some((key, val)) = line.split_once('=') {
                            let desc = if val.is_empty() {
                                String::new()
                            } else {
                                format!(" (default: `{}`)", val.trim())
                            };
                            vars.push((key.trim().to_string(), desc));
                        }
                    }
                }
            }
        }

        // Scan Dockerfile for ENV lines
        let dockerfile = dir.join("Dockerfile");
        if dockerfile.exists() {
            if let Ok(content) = std::fs::read_to_string(&dockerfile) {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if let Some(rest) = trimmed.strip_prefix("ENV ") {
                        if let Some((key, _)) = rest.split_once('=') {
                            let key = key.trim().to_string();
                            if !vars.iter().any(|(k, _)| k == &key) {
                                vars.push((key, " (from Dockerfile)".to_string()));
                            }
                        }
                    }
                }
            }
        }

        let mut lines = Vec::new();
        if vars.is_empty() {
            lines.push("No environment variables detected from .env.example, Dockerfile, or docker-compose.".to_string());
            lines.push(String::new());
            lines.push("<!-- TODO: Document required environment variables -->".to_string());
        } else {
            for (key, desc) in &vars {
                lines.push(format!("- `{key}`{desc}"));
            }
        }

        ("env_catalog".into(), "Environment Variables".into(), lines.join("\n"), 0.4)
    }
}

/// Count files recursively up to a given depth.
fn count_files_recursive(dir: &Path, max_depth: u32) -> usize {
    if max_depth == 0 {
        return 0;
    }
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                count += 1;
            } else if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    count += count_files_recursive(&path, max_depth - 1);
                }
            }
        }
    }
    count
}
