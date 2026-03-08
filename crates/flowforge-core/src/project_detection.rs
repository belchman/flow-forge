use std::path::Path;

/// Detected project profile — framework, language, entry points, etc.
#[derive(Debug, Clone, Default)]
pub struct ProjectProfile {
    pub name: String,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub project_type: String,
    pub entry_points: Vec<String>,
    pub test_dirs: Vec<String>,
    pub config_files: Vec<String>,
    pub has_api: bool,
    pub build_commands: Vec<String>,
    pub test_commands: Vec<String>,
}

/// Detect project type, languages, frameworks, and entry points from the project directory.
pub fn detect_project(project_dir: &Path) -> ProjectProfile {
    let mut profile = ProjectProfile::default();

    // Detect from Cargo.toml (Rust)
    let cargo_toml = project_dir.join("Cargo.toml");
    if cargo_toml.exists() {
        if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
            profile.languages.push("rust".to_string());
            profile.config_files.push("Cargo.toml".to_string());
            profile.build_commands.push("cargo build".to_string());
            profile.test_commands.push("cargo test".to_string());

            if content.contains("[workspace]") {
                profile.project_type = "rust-workspace".to_string();
            } else {
                profile.project_type = "rust".to_string();
            }

            // Extract package name
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("name") && trimmed.contains('=') {
                    if let Some(name) = trimmed.split('=').nth(1) {
                        let name = name.trim().trim_matches('"').trim_matches('\'');
                        if !name.is_empty() && profile.name.is_empty() {
                            profile.name = name.to_string();
                        }
                    }
                }
            }

            // Detect web frameworks from dependencies
            if content.contains("actix-web") || content.contains("actix_web") {
                profile.frameworks.push("actix-web".to_string());
                profile.has_api = true;
            }
            if content.contains("axum") {
                profile.frameworks.push("axum".to_string());
                profile.has_api = true;
            }
            if content.contains("rocket") {
                profile.frameworks.push("rocket".to_string());
                profile.has_api = true;
            }
            if content.contains("warp") {
                profile.frameworks.push("warp".to_string());
                profile.has_api = true;
            }
        }
    }

    // Detect from package.json (Node.js/TypeScript)
    let package_json = project_dir.join("package.json");
    if package_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&package_json) {
            if !profile.languages.contains(&"javascript".to_string()) {
                profile.languages.push("javascript".to_string());
            }
            profile.config_files.push("package.json".to_string());

            // Check for TypeScript
            if project_dir.join("tsconfig.json").exists() || content.contains("typescript") {
                if !profile.languages.contains(&"typescript".to_string()) {
                    profile.languages.push("typescript".to_string());
                }
                profile.config_files.push("tsconfig.json".to_string());
            }

            // Extract name
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(name) = parsed.get("name").and_then(|n| n.as_str()) {
                    if profile.name.is_empty() {
                        profile.name = name.to_string();
                    }
                }

                // Extract scripts for build/test commands
                if let Some(scripts) = parsed.get("scripts").and_then(|s| s.as_object()) {
                    if scripts.contains_key("build") {
                        profile.build_commands.push("npm run build".to_string());
                    }
                    if scripts.contains_key("test") {
                        profile.test_commands.push("npm test".to_string());
                    }
                    if scripts.contains_key("dev") {
                        profile.build_commands.push("npm run dev".to_string());
                    }
                }

                // Detect frameworks from dependencies
                let all_deps = merge_deps(&parsed);
                if all_deps.contains("next") {
                    profile.frameworks.push("next.js".to_string());
                    profile.has_api = true;
                    if profile.project_type.is_empty() {
                        profile.project_type = "nextjs-app".to_string();
                    }
                }
                if all_deps.contains("express") {
                    profile.frameworks.push("express".to_string());
                    profile.has_api = true;
                }
                if all_deps.contains("react") && !all_deps.contains("next") {
                    profile.frameworks.push("react".to_string());
                    if profile.project_type.is_empty() {
                        profile.project_type = "react-app".to_string();
                    }
                }
                if all_deps.contains("vue") {
                    profile.frameworks.push("vue".to_string());
                }
                if all_deps.contains("fastify") {
                    profile.frameworks.push("fastify".to_string());
                    profile.has_api = true;
                }
            }

            if profile.project_type.is_empty() {
                profile.project_type = "node".to_string();
            }
        }
    }

    // Detect from pyproject.toml or requirements.txt (Python)
    let pyproject = project_dir.join("pyproject.toml");
    let requirements = project_dir.join("requirements.txt");
    if pyproject.exists() || requirements.exists() {
        if !profile.languages.contains(&"python".to_string()) {
            profile.languages.push("python".to_string());
        }

        let py_content = pyproject
            .exists()
            .then(|| std::fs::read_to_string(&pyproject).ok())
            .flatten()
            .unwrap_or_default()
            + &requirements
                .exists()
                .then(|| std::fs::read_to_string(&requirements).ok())
                .flatten()
                .unwrap_or_default();

        if pyproject.exists() {
            profile.config_files.push("pyproject.toml".to_string());
        }
        if requirements.exists() {
            profile.config_files.push("requirements.txt".to_string());
        }

        profile.test_commands.push("pytest".to_string());

        if py_content.contains("fastapi") {
            profile.frameworks.push("fastapi".to_string());
            profile.has_api = true;
            if profile.project_type.is_empty() {
                profile.project_type = "python-fastapi".to_string();
            }
        }
        if py_content.contains("django") {
            profile.frameworks.push("django".to_string());
            profile.has_api = true;
            if profile.project_type.is_empty() {
                profile.project_type = "python-django".to_string();
            }
        }
        if py_content.contains("flask") {
            profile.frameworks.push("flask".to_string());
            profile.has_api = true;
            if profile.project_type.is_empty() {
                profile.project_type = "python-flask".to_string();
            }
        }

        if profile.project_type.is_empty() {
            profile.project_type = "python".to_string();
        }
    }

    // Detect from go.mod (Go)
    let go_mod = project_dir.join("go.mod");
    if go_mod.exists() {
        if let Ok(content) = std::fs::read_to_string(&go_mod) {
            if !profile.languages.contains(&"go".to_string()) {
                profile.languages.push("go".to_string());
            }
            profile.config_files.push("go.mod".to_string());
            profile.build_commands.push("go build ./...".to_string());
            profile.test_commands.push("go test ./...".to_string());

            // Extract module name
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("module ") {
                    let module_name = rest.trim();
                    if profile.name.is_empty() {
                        // Use last segment of module path as name
                        if let Some(last) = module_name.rsplit('/').next() {
                            profile.name = last.to_string();
                        }
                    }
                    break;
                }
            }

            if content.contains("gin-gonic") {
                profile.frameworks.push("gin".to_string());
                profile.has_api = true;
            }
            if content.contains("labstack/echo") {
                profile.frameworks.push("echo".to_string());
                profile.has_api = true;
            }
            if content.contains("go-chi/chi") {
                profile.frameworks.push("chi".to_string());
                profile.has_api = true;
            }

            if profile.project_type.is_empty() {
                profile.project_type = "go".to_string();
            }
        }
    }

    // Detect Docker
    if project_dir.join("Dockerfile").exists() {
        profile.config_files.push("Dockerfile".to_string());
    }
    if project_dir.join("docker-compose.yml").exists()
        || project_dir.join("docker-compose.yaml").exists()
    {
        profile.config_files.push("docker-compose.yml".to_string());
    }

    // Detect CI/CD
    if project_dir.join(".github/workflows").is_dir() {
        profile.config_files.push(".github/workflows/".to_string());
    }
    if project_dir.join(".gitlab-ci.yml").exists() {
        profile.config_files.push(".gitlab-ci.yml".to_string());
    }

    // Detect Makefile
    if project_dir.join("Makefile").exists() {
        profile.config_files.push("Makefile".to_string());
    }

    // Detect monorepo indicators
    if project_dir.join("turbo.json").exists()
        || project_dir.join("pnpm-workspace.yaml").exists()
        || project_dir.join("lerna.json").exists()
    {
        if profile.project_type.is_empty() || profile.project_type == "node" {
            profile.project_type = "monorepo".to_string();
        }
    }

    // Detect common entry points
    for entry in &[
        "src/main.rs",
        "src/lib.rs",
        "src/index.ts",
        "src/index.js",
        "index.ts",
        "index.js",
        "app.py",
        "main.py",
        "manage.py",
        "main.go",
        "cmd/main.go",
    ] {
        if project_dir.join(entry).exists() {
            profile.entry_points.push(entry.to_string());
        }
    }

    // Detect test directories
    for test_dir in &[
        "tests",
        "test",
        "src/test",
        "src/tests",
        "__tests__",
        "spec",
    ] {
        if project_dir.join(test_dir).is_dir() {
            profile.test_dirs.push(test_dir.to_string());
        }
    }

    // Fallback name from directory
    if profile.name.is_empty() {
        if let Some(dir_name) = project_dir.file_name().and_then(|n| n.to_str()) {
            profile.name = dir_name.to_string();
        }
    }

    if profile.project_type.is_empty() {
        profile.project_type = "unknown".to_string();
    }

    profile
}

/// Merge dependencies and devDependencies keys from a package.json Value.
fn merge_deps(parsed: &serde_json::Value) -> std::collections::HashSet<String> {
    let mut deps = std::collections::HashSet::new();
    for key in &["dependencies", "devDependencies"] {
        if let Some(obj) = parsed.get(*key).and_then(|v| v.as_object()) {
            deps.extend(obj.keys().cloned());
        }
    }
    deps
}
