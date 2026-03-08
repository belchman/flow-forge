/// Symbol extraction for project code indexing.
///
/// Detects language from file extension, extracts function/struct/class definitions
/// via line-based pattern matching, and builds summary text for embedding.

use std::path::Path;

/// File extensions considered for indexing.
pub const EXTENSION_WHITELIST: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "toml", "md", "yaml", "yml", "json", "sh",
];

/// Directories to skip during indexing.
pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".flowforge",
    "vendor",
    ".claude",
    ".vscode",
    "dist",
    "build",
];

/// Detect language from file extension.
pub fn detect_language(path: &Path) -> Option<&'static str> {
    path.extension()?.to_str().and_then(|ext| match ext {
        "rs" => Some("rust"),
        "ts" | "tsx" => Some("typescript"),
        "js" | "jsx" => Some("javascript"),
        "py" => Some("python"),
        "go" => Some("go"),
        "toml" => Some("toml"),
        "md" => Some("markdown"),
        "yaml" | "yml" => Some("yaml"),
        "json" => Some("json"),
        "sh" => Some("shell"),
        _ => None,
    })
}

/// Check if a file extension is in the whitelist.
pub fn is_indexable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| EXTENSION_WHITELIST.contains(&ext))
        .unwrap_or(false)
}

/// Check if a directory should be skipped.
pub fn should_skip_dir(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

/// Extract symbol definitions from source code.
pub fn extract_symbols(content: &str, language: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        match language {
            "rust" => {
                // pub fn name, pub struct Name, pub enum Name, pub trait Name, pub type Name, pub mod name
                let rest = if let Some(r) = trimmed.strip_prefix("pub ") {
                    Some(r)
                } else {
                    trimmed.strip_prefix("pub(crate) ")
                };
                if let Some(rest) = rest {
                    for keyword in &["fn ", "struct ", "enum ", "trait ", "type ", "mod "] {
                        if let Some(after) = rest.strip_prefix(keyword) {
                            if let Some(name) = extract_identifier(after) {
                                symbols.push(name);
                            }
                            break;
                        }
                    }
                }
            }
            "typescript" | "javascript" => {
                if let Some(rest) = trimmed.strip_prefix("export ") {
                    let rest = rest.strip_prefix("default ").unwrap_or(rest);
                    for keyword in &[
                        "function ",
                        "class ",
                        "interface ",
                        "type ",
                        "const ",
                        "enum ",
                    ] {
                        if let Some(after) = rest.strip_prefix(keyword) {
                            if let Some(name) = extract_identifier(after) {
                                symbols.push(name);
                            }
                            break;
                        }
                    }
                }
            }
            "python" => {
                for keyword in &["def ", "class "] {
                    if let Some(after) = trimmed.strip_prefix(keyword) {
                        if let Some(name) = extract_identifier(after) {
                            symbols.push(name);
                        }
                        break;
                    }
                }
            }
            "go" => {
                if let Some(after) = trimmed.strip_prefix("func ") {
                    // Skip methods: func (r *Receiver) Name
                    if !after.starts_with('(') {
                        if let Some(name) = extract_identifier(after) {
                            symbols.push(name);
                        }
                    }
                }
                if let Some(after) = trimmed.strip_prefix("type ") {
                    if let Some(name) = extract_identifier(after) {
                        symbols.push(name);
                    }
                }
            }
            _ => {}
        }
    }
    symbols.dedup();
    symbols
}

/// Extract the first doc comment from source code, max 200 chars.
pub fn extract_description(content: &str, language: &str) -> String {
    let mut desc = String::new();
    match language {
        "rust" => {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(doc) = trimmed
                    .strip_prefix("/// ")
                    .or_else(|| trimmed.strip_prefix("//! "))
                {
                    if desc.len() + doc.len() < 200 {
                        if !desc.is_empty() {
                            desc.push(' ');
                        }
                        desc.push_str(doc);
                    } else {
                        break;
                    }
                } else if !trimmed.starts_with("///")
                    && !trimmed.starts_with("//!")
                    && !trimmed.is_empty()
                {
                    if !desc.is_empty() {
                        break;
                    }
                }
            }
        }
        "python" => {
            let mut in_docstring = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if !in_docstring {
                    if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
                        in_docstring = true;
                        let rest = &trimmed[3..];
                        if rest.ends_with("\"\"\"") || rest.ends_with("'''") {
                            desc = rest[..rest.len() - 3].to_string();
                            break;
                        }
                        desc = rest.to_string();
                    }
                } else {
                    if trimmed.ends_with("\"\"\"") || trimmed.ends_with("'''") {
                        if desc.len() < 200 {
                            if !desc.is_empty() {
                                desc.push(' ');
                            }
                            desc.push_str(&trimmed[..trimmed.len() - 3]);
                        }
                        break;
                    }
                    if desc.len() < 200 {
                        if !desc.is_empty() {
                            desc.push(' ');
                        }
                        desc.push_str(trimmed);
                    }
                }
            }
        }
        "typescript" | "javascript" => {
            let mut in_jsdoc = false;
            for line in content.lines() {
                let trimmed = line.trim();
                if !in_jsdoc {
                    if trimmed.starts_with("/**") {
                        in_jsdoc = true;
                        let rest = trimmed.strip_prefix("/**").unwrap().trim();
                        if let Some(rest) = rest.strip_suffix("*/") {
                            desc = rest.trim().to_string();
                            break;
                        }
                        if !rest.is_empty() {
                            desc = rest.to_string();
                        }
                    }
                } else {
                    if trimmed.ends_with("*/") {
                        let line_content = trimmed.strip_suffix("*/").unwrap().trim();
                        let line_content =
                            line_content.strip_prefix("* ").unwrap_or(line_content);
                        if desc.len() < 200 && !line_content.is_empty() {
                            if !desc.is_empty() {
                                desc.push(' ');
                            }
                            desc.push_str(line_content);
                        }
                        break;
                    }
                    let line_content = trimmed
                        .strip_prefix("* ")
                        .or_else(|| trimmed.strip_prefix("*"))
                        .unwrap_or(trimmed);
                    if desc.len() < 200 && !line_content.is_empty() {
                        if !desc.is_empty() {
                            desc.push(' ');
                        }
                        desc.push_str(line_content);
                    }
                }
            }
        }
        _ => {}
    }
    desc.truncate(200);
    desc
}

/// Build a summary string for embedding from file path, symbols, and description.
pub fn build_summary(path: &str, symbols: &[String], description: &str) -> String {
    let filename = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);

    let sym_str = if symbols.is_empty() {
        String::new()
    } else {
        let sym_list: Vec<&str> = symbols.iter().take(20).map(|s| s.as_str()).collect();
        format!(" symbols: {}", sym_list.join(", "))
    };

    let desc_str = if description.is_empty() {
        String::new()
    } else {
        format!(" — {description}")
    };

    format!("{filename}{desc_str}{sym_str}")
}

/// Extract an identifier (alphanumeric + underscore) from the start of a string.
fn extract_identifier(s: &str) -> Option<String> {
    let name: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language(Path::new("src/main.rs")), Some("rust"));
        assert_eq!(detect_language(Path::new("app.ts")), Some("typescript"));
        assert_eq!(detect_language(Path::new("app.tsx")), Some("typescript"));
        assert_eq!(detect_language(Path::new("script.py")), Some("python"));
        assert_eq!(detect_language(Path::new("main.go")), Some("go"));
        assert_eq!(detect_language(Path::new("Makefile")), None);
    }

    #[test]
    fn test_extract_rust_symbols() {
        let code = "\
pub fn hello() {}
pub struct MyStruct {}
pub enum MyEnum { A, B }
pub trait MyTrait {}
fn private_fn() {}
pub(crate) fn crate_fn() {}
";
        let symbols = extract_symbols(code, "rust");
        assert!(symbols.contains(&"hello".to_string()));
        assert!(symbols.contains(&"MyStruct".to_string()));
        assert!(symbols.contains(&"MyEnum".to_string()));
        assert!(symbols.contains(&"MyTrait".to_string()));
        assert!(symbols.contains(&"crate_fn".to_string()));
        assert!(!symbols.contains(&"private_fn".to_string()));
    }

    #[test]
    fn test_extract_typescript_symbols() {
        let code = "\
export function greet() {}
export class MyClass {}
export interface Config {}
export type Result = string;
export const API_KEY = 'x';
const private = 1;
";
        let symbols = extract_symbols(code, "typescript");
        assert!(symbols.contains(&"greet".to_string()));
        assert!(symbols.contains(&"MyClass".to_string()));
        assert!(symbols.contains(&"Config".to_string()));
        assert!(symbols.contains(&"Result".to_string()));
        assert!(symbols.contains(&"API_KEY".to_string()));
        assert!(!symbols.contains(&"private".to_string()));
    }

    #[test]
    fn test_extract_python_symbols() {
        let code = "\
def hello():
    pass

class MyClass:
    def method(self):
        pass
";
        let symbols = extract_symbols(code, "python");
        assert!(symbols.contains(&"hello".to_string()));
        assert!(symbols.contains(&"MyClass".to_string()));
        assert!(symbols.contains(&"method".to_string()));
    }

    #[test]
    fn test_extract_go_symbols() {
        let code = "\
func Main() {}
func (r *Router) Handle() {}
type Config struct {}
type Handler interface {}
";
        let symbols = extract_symbols(code, "go");
        assert!(symbols.contains(&"Main".to_string()));
        assert!(symbols.contains(&"Config".to_string()));
        assert!(symbols.contains(&"Handler".to_string()));
        // Method should be skipped (starts with '(')
        assert!(!symbols.contains(&"Handle".to_string()));
    }

    #[test]
    fn test_extract_description_rust() {
        let code = "/// This is a module for testing.\n/// It does stuff.\npub fn test() {}";
        let desc = extract_description(code, "rust");
        assert!(desc.contains("module for testing"));
        assert!(desc.contains("does stuff"));
    }

    #[test]
    fn test_extract_description_python() {
        let code = "\"\"\"A simple module for testing.\"\"\"";
        let desc = extract_description(code, "python");
        assert_eq!(desc, "A simple module for testing.");
    }

    #[test]
    fn test_build_summary() {
        let summary = build_summary(
            "src/main.rs",
            &["main".to_string(), "run".to_string()],
            "Entry point",
        );
        assert!(summary.contains("main.rs"));
        assert!(summary.contains("Entry point"));
        assert!(summary.contains("main"));
    }

    #[test]
    fn test_is_indexable() {
        assert!(is_indexable(Path::new("main.rs")));
        assert!(is_indexable(Path::new("app.ts")));
        assert!(!is_indexable(Path::new("image.png")));
        assert!(!is_indexable(Path::new("binary")));
    }

    #[test]
    fn test_should_skip_dir() {
        assert!(should_skip_dir(".git"));
        assert!(should_skip_dir("target"));
        assert!(should_skip_dir("node_modules"));
        assert!(!should_skip_dir("src"));
    }

    #[test]
    fn test_extract_identifier() {
        assert_eq!(extract_identifier("hello_world("), Some("hello_world".to_string()));
        assert_eq!(extract_identifier("MyStruct {"), Some("MyStruct".to_string()));
        assert_eq!(extract_identifier(""), None);
        assert_eq!(extract_identifier("("), None);
    }
}
