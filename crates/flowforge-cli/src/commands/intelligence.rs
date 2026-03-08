use colored::Colorize;
use flowforge_core::intelligence::{
    CodeEntry, CoEditPair, ErrorHotspot, IntelligenceData, IntelligenceGenerator, TestCoOccurrence,
};
use flowforge_core::project_detection::detect_project;
use flowforge_core::{FlowForgeConfig, Result};
use flowforge_memory::{IntelligenceSection, MemoryDb};

pub fn generate(force: bool, section: Option<&str>, dry_run: bool) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    let code_count = db.count_code_entries()?;
    if code_count == 0 {
        println!(
            "{} No code index entries found. Run `flowforge index` first to populate the code index.",
            "✗".red()
        );
        return Ok(());
    }

    if !force && db.has_intelligence()? {
        println!(
            "{} Intelligence already exists ({} section(s)). Use --force to regenerate.",
            "→".dimmed(),
            db.list_intelligence_sections()?.len()
        );
        return Ok(());
    }

    let project_dir = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let profile = detect_project(&project_dir);

    println!(
        "{} Detected: {} ({})",
        "→".dimmed(),
        profile.name.bold(),
        profile.project_type
    );

    // Load data from DB
    let code_entries: Vec<CodeEntry> = db
        .list_code_entries(5000)?
        .into_iter()
        .map(|e| CodeEntry {
            file_path: e.file_path,
            language: e.language,
            size_bytes: e.size_bytes,
            symbols: e.symbols,
            description: e.description,
        })
        .collect();

    let co_edit_pairs = load_co_edit_pairs(&db);
    let error_hotspots = load_error_hotspots(&db);
    let test_co_occurrences = load_test_co_occurrences(&db);

    let data = IntelligenceData {
        code_entries,
        co_edit_pairs,
        error_hotspots,
        test_co_occurrences,
    };

    let generator = IntelligenceGenerator::new(&project_dir, &profile, &data);
    let sections = generator.generate_all();

    if dry_run {
        println!("\n{}", "Dry run — sections that would be generated:".bold());
        for (key, title, content, confidence) in &sections {
            if let Some(filter) = section {
                if key != filter {
                    continue;
                }
            }
            let todo_count = content.matches("TODO").count();
            println!(
                "  {} {} (confidence: {:.0}%, TODOs: {})",
                "•".cyan(),
                title,
                confidence * 100.0,
                todo_count
            );
        }
        return Ok(());
    }

    // Store sections
    let embedder = flowforge_memory::default_embedder(&config.patterns);
    let mut stored = 0;

    for (key, title, content, confidence) in &sections {
        if let Some(filter) = section {
            if key != filter {
                continue;
            }
        }

        // Embed the section content
        let vec = embedder.embed(&format!("{title}: {content}"));
        let embedding_id = db.store_vector("project_intel", key, &vec)?;

        let intel_section = IntelligenceSection {
            section_key: key.clone(),
            section_title: title.clone(),
            content: content.clone(),
            auto_generated: true,
            confidence: *confidence,
            embedding_id: Some(embedding_id),
            project_type: Some(profile.project_type.clone()),
            updated_at: chrono::Utc::now(),
        };
        db.upsert_intelligence_section(&intel_section)?;
        stored += 1;

        let todo_count = content.matches("TODO").count();
        println!(
            "  {} {} (confidence: {:.0}%{})",
            "✓".green(),
            title,
            confidence * 100.0,
            if todo_count > 0 {
                format!(", {} TODO(s)", todo_count)
            } else {
                String::new()
            }
        );
    }

    // Auto-export markdown
    let md = db.get_intelligence_markdown()?;
    let export_path = FlowForgeConfig::project_dir().join("PROJECT_INTELLIGENCE.md");
    std::fs::write(&export_path, &md).map_err(|e| flowforge_core::Error::Config(e.to_string()))?;

    println!(
        "\n{} Generated {} section(s), exported to {}",
        "✓".green(),
        stored,
        export_path.display()
    );

    Ok(())
}

pub fn show(section: Option<&str>, json: bool) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    if let Some(key) = section {
        match db.get_intelligence_section(key)? {
            Some(s) => {
                if json {
                    print_section_json(&s);
                } else {
                    print_section(&s);
                }
            }
            None => println!("Section '{}' not found.", key),
        }
    } else {
        let sections = db.list_intelligence_sections()?;
        if sections.is_empty() {
            println!("No intelligence sections found. Run `flowforge intelligence generate` first.");
            return Ok(());
        }
        if json {
            let arr: Vec<_> = sections
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "key": s.section_key,
                        "title": s.section_title,
                        "content": s.content,
                        "auto_generated": s.auto_generated,
                        "confidence": s.confidence,
                        "project_type": s.project_type,
                        "updated_at": s.updated_at.to_rfc3339(),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&arr).unwrap_or_default());
        } else {
            for s in &sections {
                print_section(s);
                println!();
            }
        }
    }
    Ok(())
}

pub fn export(output: Option<&str>) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    if !db.has_intelligence()? {
        println!("No intelligence sections found. Run `flowforge intelligence generate` first.");
        return Ok(());
    }

    let md = db.get_intelligence_markdown()?;
    let path = output
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|| FlowForgeConfig::project_dir().join("PROJECT_INTELLIGENCE.md"));

    std::fs::write(&path, &md).map_err(|e| flowforge_core::Error::Config(e.to_string()))?;
    println!("{} Exported to {}", "✓".green(), path.display());
    Ok(())
}

pub fn status() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    let sections = db.list_intelligence_sections()?;
    if sections.is_empty() {
        println!("No intelligence sections. Run `flowforge intelligence generate`.");
        return Ok(());
    }

    let total = sections.len();
    let avg_confidence: f64 = sections.iter().map(|s| s.confidence).sum::<f64>() / total as f64;
    let needs_refinement: Vec<_> = sections.iter().filter(|s| s.confidence < 0.6).collect();
    let auto_gen = sections.iter().filter(|s| s.auto_generated).count();
    let refined = total - auto_gen;

    println!("{}", "Project Intelligence Status".bold());
    println!("  Total sections: {}", total);
    println!("  Auto-generated: {}, Refined: {}", auto_gen, refined);
    println!("  Average confidence: {:.0}%", avg_confidence * 100.0);

    if !needs_refinement.is_empty() {
        println!(
            "\n  {} Sections needing refinement (confidence < 60%):",
            "→".yellow()
        );
        for s in &needs_refinement {
            println!(
                "    {} {} ({:.0}%)",
                "•".red(),
                s.section_title,
                s.confidence * 100.0
            );
        }
    }

    // Check staleness
    let latest_index = db.get_latest_code_index_time().unwrap_or(None);
    let oldest_intel = sections
        .iter()
        .min_by_key(|s| s.updated_at)
        .map(|s| s.updated_at.to_rfc3339());

    if let (Some(idx_time), Some(intel_time)) = (latest_index, oldest_intel) {
        if idx_time > intel_time {
            println!(
                "\n  {} Intelligence may be stale — code index updated after last generation.",
                "⚠".yellow()
            );
        }
    }

    Ok(())
}

fn print_section(s: &IntelligenceSection) {
    let gen_tag = if s.auto_generated {
        "[auto]".dimmed().to_string()
    } else {
        "[refined]".green().to_string()
    };
    println!(
        "{} {} (confidence: {:.0}%) {}",
        format!("[{}]", s.section_key).cyan(),
        s.section_title.bold(),
        s.confidence * 100.0,
        gen_tag,
    );
    println!("{}", s.content);
}

fn print_section_json(s: &IntelligenceSection) {
    let json = serde_json::json!({
        "key": s.section_key,
        "title": s.section_title,
        "content": s.content,
        "auto_generated": s.auto_generated,
        "confidence": s.confidence,
        "project_type": s.project_type,
        "updated_at": s.updated_at.to_rfc3339(),
    });
    println!("{}", serde_json::to_string_pretty(&json).unwrap_or_default());
}

// ── DB data loaders ──────────────────────────────────────────

fn load_co_edit_pairs(db: &MemoryDb) -> Vec<CoEditPair> {
    db.list_co_edit_pairs(20)
        .unwrap_or_default()
        .into_iter()
        .map(|(a, b, count)| CoEditPair {
            file_a: a,
            file_b: b,
            co_edit_count: count,
        })
        .collect()
}

fn load_error_hotspots(db: &MemoryDb) -> Vec<ErrorHotspot> {
    db.list_error_hotspots(10)
        .unwrap_or_default()
        .into_iter()
        .map(|(tool, cat, preview, count, has_res)| ErrorHotspot {
            tool_name: tool,
            category: cat,
            error_preview: preview,
            occurrence_count: count,
            has_resolution: has_res,
        })
        .collect()
}

fn load_test_co_occurrences(db: &MemoryDb) -> Vec<TestCoOccurrence> {
    db.list_test_co_occurrences(20)
        .unwrap_or_default()
        .into_iter()
        .map(|(edited, test, count)| TestCoOccurrence {
            edited_file: edited,
            test_file: test,
            occurrence_count: count,
        })
        .collect()
}
