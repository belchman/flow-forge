use colored::Colorize;
use flowforge_core::code_symbols;
use flowforge_core::{FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;
use sha2::{Digest, Sha256};
use std::path::Path;

enum IndexResult {
    Indexed,
    Skipped,
}

pub fn run(dry_run: bool, stats: bool, file: Option<&str>, prune: bool) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    if stats {
        return show_stats(&db);
    }

    let project_dir = std::env::current_dir().map_err(|e| {
        flowforge_core::Error::Config(format!("Cannot determine current directory: {e}"))
    })?;

    let embedder = if config.vectors.embed_code {
        Some(flowforge_memory::default_embedder(&config.patterns))
    } else {
        None
    };

    // Single file mode
    if let Some(file_path) = file {
        let path = Path::new(file_path);
        if !path.exists() {
            println!("  {} File not found: {}", "✗".red(), file_path);
            return Ok(());
        }
        let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let rel = abs_path
            .strip_prefix(&project_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| abs_path.display().to_string());
        match index_file(&db, &rel, &abs_path, dry_run, embedder.as_deref())? {
            IndexResult::Indexed => println!("  {} {}", "✓".green(), rel),
            IndexResult::Skipped => println!("  {} {} (unchanged)", "→".dimmed(), rel),
        }
        return Ok(());
    }

    // Full project scan
    let mut indexed = 0u64;
    let mut skipped = 0u64;
    let mut errors = 0u64;
    let mut by_language: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    let mut valid_paths: Vec<String> = Vec::new();

    println!("{}", "Indexing project files...".bold());

    walk_project(
        &project_dir,
        &project_dir,
        &db,
        dry_run,
        embedder.as_deref(),
        &mut indexed,
        &mut skipped,
        &mut errors,
        &mut by_language,
        &mut valid_paths,
    );

    if prune && !dry_run {
        let pruned = db.delete_stale_code_entries(&valid_paths)?;
        if pruned > 0 {
            println!("  {} Pruned {} stale entries", "✓".green(), pruned);
        }
    }

    println!("\n{}", "Summary".bold());
    println!("  Indexed: {}", indexed.to_string().green());
    println!("  Skipped (unchanged): {}", skipped);
    if errors > 0 {
        println!("  Errors: {}", errors.to_string().red());
    }
    if !by_language.is_empty() {
        println!("\n{}", "By language:".dimmed());
        let mut langs: Vec<_> = by_language.into_iter().collect();
        langs.sort_by(|a, b| b.1.cmp(&a.1));
        for (lang, count) in &langs {
            println!("  {lang}: {count}");
        }
    }

    Ok(())
}

fn walk_project(
    dir: &Path,
    project_dir: &Path,
    db: &MemoryDb,
    dry_run: bool,
    embedder: Option<&dyn flowforge_memory::Embedder>,
    indexed: &mut u64,
    skipped: &mut u64,
    errors: &mut u64,
    by_language: &mut std::collections::HashMap<String, u64>,
    valid_paths: &mut Vec<String>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if code_symbols::should_skip_dir(&name_str) {
                continue;
            }
            walk_project(
                &path,
                project_dir,
                db,
                dry_run,
                embedder,
                indexed,
                skipped,
                errors,
                by_language,
                valid_paths,
            );
            continue;
        }

        if !code_symbols::is_indexable(&path) {
            continue;
        }

        let rel = path
            .strip_prefix(project_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| path.display().to_string());
        valid_paths.push(rel.clone());

        match index_file(db, &rel, &path, dry_run, embedder) {
            Ok(IndexResult::Indexed) => {
                *indexed += 1;
                if let Some(lang) = code_symbols::detect_language(&path) {
                    *by_language.entry(lang.to_string()).or_insert(0) += 1;
                }
            }
            Ok(IndexResult::Skipped) => {
                *skipped += 1;
            }
            Err(_) => {
                *errors += 1;
            }
        }
    }
}

fn index_file(
    db: &MemoryDb,
    rel_path: &str,
    abs_path: &Path,
    dry_run: bool,
    embedder: Option<&dyn flowforge_memory::Embedder>,
) -> Result<IndexResult> {
    let content = std::fs::read_to_string(abs_path).map_err(|e| {
        flowforge_core::Error::Config(format!("Cannot read {rel_path}: {e}"))
    })?;

    let hash = format!("{:x}", Sha256::digest(content.as_bytes()));

    // Check if unchanged
    if let Ok(Some(existing_hash)) = db.get_code_entry_hash(rel_path) {
        if existing_hash == hash {
            return Ok(IndexResult::Skipped);
        }
    }

    if dry_run {
        return Ok(IndexResult::Indexed);
    }

    let language = code_symbols::detect_language(abs_path).unwrap_or("unknown");
    let symbols = code_symbols::extract_symbols(&content, language);
    let description = code_symbols::extract_description(&content, language);
    let summary = code_symbols::build_summary(rel_path, &symbols, &description);
    let size = content.len() as i64;

    // Embed if embedder available
    let embedding_id = if let Some(emb) = embedder {
        let vec = emb.embed(&summary);
        // Delete old vector if re-indexing, then store new one
        let _ = db.delete_vectors_for_source("code_file", rel_path);
        db.store_vector("code_file", rel_path, &vec).ok()
    } else {
        None
    };

    let entry = flowforge_memory::db::code_index::CodeIndexEntry {
        file_path: rel_path.to_string(),
        language: language.to_string(),
        size_bytes: size,
        symbols,
        description,
        summary,
        content_hash: hash,
        indexed_at: chrono::Utc::now(),
        embedding_id,
    };

    db.upsert_code_entry(&entry)?;
    Ok(IndexResult::Indexed)
}

fn show_stats(db: &MemoryDb) -> Result<()> {
    let total = db.count_code_entries()?;
    let unvectorized = db.count_unvectorized_code_entries()?;
    let vectorized = total - unvectorized;

    println!("{}", "Code Index Statistics".bold());
    println!("  Total files: {}", total);
    println!(
        "  Vectorized: {} ({}%)",
        vectorized,
        if total > 0 {
            vectorized * 100 / total
        } else {
            0
        }
    );
    println!("  Pending: {}", unvectorized);

    // Count by language
    let entries = db.list_code_entries(10000)?;
    let mut by_lang: std::collections::HashMap<&str, u64> = std::collections::HashMap::new();
    for entry in &entries {
        *by_lang.entry(&entry.language).or_insert(0) += 1;
    }
    if !by_lang.is_empty() {
        println!("\n{}", "By language:".dimmed());
        let mut langs: Vec<_> = by_lang.into_iter().collect();
        langs.sort_by(|a, b| b.1.cmp(&a.1));
        for (lang, count) in &langs {
            println!("  {lang}: {count}");
        }
    }

    Ok(())
}
