use flowforge_core::FlowForgeConfig;
use flowforge_memory::MemoryDb;
use serde_json::{json, Value};

use crate::params::ParamExt;

pub fn get(db: &MemoryDb, params: &Value) -> flowforge_core::Result<Value> {
    let section_key = params.opt_str("section");

    if let Some(key) = section_key {
        match db.get_intelligence_section(key)? {
            Some(s) => Ok(json!({
                "status": "ok",
                "section": {
                    "key": s.section_key,
                    "title": s.section_title,
                    "content": s.content,
                    "auto_generated": s.auto_generated,
                    "confidence": s.confidence,
                    "project_type": s.project_type,
                    "updated_at": s.updated_at.to_rfc3339(),
                }
            })),
            None => Ok(json!({
                "status": "error",
                "message": format!("Section '{}' not found", key)
            })),
        }
    } else {
        let sections = db.list_intelligence_sections()?;
        let arr: Vec<Value> = sections
            .iter()
            .map(|s| {
                json!({
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
        Ok(json!({
            "status": "ok",
            "count": arr.len(),
            "sections": arr,
        }))
    }
}

pub fn update(
    db: &MemoryDb,
    config: &FlowForgeConfig,
    params: &Value,
) -> flowforge_core::Result<Value> {
    let key = params.require_str("section")?;
    let content = params.require_str("content")?;
    let confidence = params
        .get("confidence")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.9);

    // Re-embed the updated content
    let embedder = flowforge_memory::default_embedder(&config.patterns);
    let title = db
        .get_intelligence_section(key)?
        .map(|s| s.section_title)
        .unwrap_or_else(|| key.to_string());

    let vec = embedder.embed(&format!("{title}: {content}"));
    let embedding_id = db.store_vector("project_intel", key, &vec)?;

    let section = flowforge_memory::IntelligenceSection {
        section_key: key.to_string(),
        section_title: title.clone(),
        content: content.to_string(),
        auto_generated: false,
        confidence,
        embedding_id: Some(embedding_id),
        project_type: None,
        updated_at: chrono::Utc::now(),
    };
    db.upsert_intelligence_section(&section)?;

    // Re-export markdown file
    if let Ok(md) = db.get_intelligence_markdown() {
        let export_path = FlowForgeConfig::project_dir().join("PROJECT_INTELLIGENCE.md");
        let _ = std::fs::write(&export_path, &md);
    }

    Ok(json!({
        "status": "ok",
        "message": format!("Updated section '{}' with confidence {:.0}%", key, confidence * 100.0),
        "section": key,
        "confidence": confidence,
    }))
}

pub fn refine(db: &MemoryDb) -> flowforge_core::Result<Value> {
    let sections = db.list_intelligence_sections()?;

    if sections.is_empty() {
        return Ok(json!({
            "status": "ok",
            "message": "No intelligence sections found. Run `flowforge intelligence generate` first.",
            "sections": [],
        }));
    }

    let low_confidence: Vec<Value> = sections
        .iter()
        .filter(|s| s.confidence < 0.6)
        .map(|s| {
            json!({
                "key": s.section_key,
                "title": s.section_title,
                "content": s.content,
                "confidence": s.confidence,
                "auto_generated": s.auto_generated,
                "instructions": format!(
                    "This section has low confidence ({:.0}%). Please read it, improve the content, \
                     fill in any TODO markers, and call `intelligence_update` with the refined content \
                     and a confidence of 0.8-0.95.",
                    s.confidence * 100.0
                ),
            })
        })
        .collect();

    if low_confidence.is_empty() {
        return Ok(json!({
            "status": "ok",
            "message": "All sections have confidence >= 60%. No refinement needed.",
            "sections": [],
        }));
    }

    Ok(json!({
        "status": "ok",
        "message": format!("{} section(s) need refinement", low_confidence.len()),
        "sections": low_confidence,
    }))
}

pub fn status(db: &MemoryDb) -> flowforge_core::Result<Value> {
    let sections = db.list_intelligence_sections()?;

    if sections.is_empty() {
        return Ok(json!({
            "status": "ok",
            "has_intelligence": false,
            "total_sections": 0,
            "message": "No intelligence generated yet. Run `flowforge intelligence generate`.",
        }));
    }

    let total = sections.len();
    let avg_confidence = sections.iter().map(|s| s.confidence).sum::<f64>() / total as f64;
    let needs_refinement = sections.iter().filter(|s| s.confidence < 0.6).count();
    let auto_generated = sections.iter().filter(|s| s.auto_generated).count();

    Ok(json!({
        "status": "ok",
        "has_intelligence": true,
        "total_sections": total,
        "auto_generated": auto_generated,
        "refined": total - auto_generated,
        "average_confidence": format!("{:.0}%", avg_confidence * 100.0),
        "needs_refinement": needs_refinement,
        "sections": sections.iter().map(|s| json!({
            "key": s.section_key,
            "confidence": s.confidence,
            "auto_generated": s.auto_generated,
        })).collect::<Vec<_>>(),
    }))
}
