use std::collections::HashMap;

use flowforge_core::config::RoutingConfig;
use flowforge_core::{AgentDef, RoutingBreakdown, RoutingCategory, RoutingContext, RoutingResult};
use regex::Regex;
use tracing::warn;

/// Routes tasks to the best-matching agent based on a weighted scoring algorithm.
///
/// Score = pattern_weight * pattern_score
///       + capability_weight * capability_score
///       + learned_weight * learned_score
///       + priority_weight * priority_score
///       + context_weight * context_score
///       + semantic_weight * semantic_score
pub struct AgentRouter {
    pattern_weight: f64,
    capability_weight: f64,
    learned_weight: f64,
    priority_weight: f64,
    context_weight: f64,
    semantic_weight: f64,
    confidence_sharpening: f64,
}

/// Apply sigmoid sharpening to spread raw confidence scores apart.
/// k=8.0: raw 0.30→~0.15, raw 0.50→~0.50, raw 0.65→~0.80
fn sharpen_confidence(raw: f64, k: f64) -> f64 {
    if k <= 0.0 {
        return raw;
    }
    1.0 / (1.0 + (-k * (raw - 0.50)).exp())
}

/// Check if task text mentions swarm/team/coordination keywords.
fn mentions_swarm(task: &str) -> bool {
    let t = task.to_lowercase();
    t.contains("swarm")
        || t.contains("team")
        || t.contains("coordinat")
        || t.contains("consensus")
        || t.contains("multi-agent")
}

/// Check if task text mentions workflow/pipeline keywords.
fn mentions_workflow(task: &str) -> bool {
    let t = task.to_lowercase();
    t.contains("workflow")
        || t.contains("pipeline")
        || t.contains("automat")
        || t.contains("sparc")
        || t.contains("goal plan")
}

impl AgentRouter {
    /// Create a new router with the given weight configuration.
    /// Non-finite weights are replaced with 0.0 to prevent NaN propagation.
    pub fn new(config: &RoutingConfig) -> Self {
        let sanitize = |w: f64| if w.is_finite() { w } else { 0.0 };
        Self {
            pattern_weight: sanitize(config.pattern_weight),
            capability_weight: sanitize(config.capability_weight),
            learned_weight: sanitize(config.learned_weight),
            priority_weight: sanitize(config.priority_weight),
            context_weight: sanitize(config.context_weight),
            semantic_weight: sanitize(config.semantic_weight),
            confidence_sharpening: if config.confidence_sharpening.is_finite() {
                config.confidence_sharpening
            } else {
                0.0
            },
        }
    }

    /// Route a task to the best-matching agents.
    ///
    /// Returns a list of `RoutingResult` sorted by confidence (highest first).
    ///
    /// - `task`: the task description text
    /// - `agents`: available agent definitions to score
    /// - `learned_weights`: mapping of (task_pattern, agent_name) -> weight from learning system
    /// - `context`: optional session context for context-aware scoring
    /// - `semantic_scores`: optional pre-computed semantic similarity scores per agent
    pub fn route(
        &self,
        task: &str,
        agents: &[&AgentDef],
        learned_weights: &HashMap<(String, String), f64>,
        context: Option<&RoutingContext>,
        semantic_scores: Option<&HashMap<String, f64>>,
    ) -> Vec<RoutingResult> {
        // Pre-filter agents by routing category
        let want_swarm = mentions_swarm(task);
        let want_workflow = mentions_workflow(task);

        let filtered: Vec<&&AgentDef> = agents
            .iter()
            .filter(|agent| match agent.routing_category {
                RoutingCategory::Core | RoutingCategory::Specialist => true,
                RoutingCategory::SwarmOnly => want_swarm,
                RoutingCategory::WorkflowOnly => want_workflow,
            })
            .collect();

        let mut results: Vec<RoutingResult> = filtered
            .iter()
            .map(|agent| {
                let pattern_score = self.compute_pattern_score(task, agent);
                let capability_score = self.compute_capability_score(task, agent);
                let learned_score = self.compute_learned_score(task, agent, learned_weights);
                let priority_score = agent.priority.boost();
                let context_score = context
                    .map(|ctx| self.compute_context_score(agent, ctx))
                    .unwrap_or(0.5);
                let semantic_score = semantic_scores
                    .and_then(|scores| scores.get(&agent.name).copied())
                    .unwrap_or(0.0);

                let raw_confidence = self.pattern_weight * pattern_score
                    + self.capability_weight * capability_score
                    + self.learned_weight * learned_score
                    + self.priority_weight * priority_score
                    + self.context_weight * context_score
                    + self.semantic_weight * semantic_score;

                // Apply sigmoid sharpening, then clamp
                let sharpened = sharpen_confidence(raw_confidence, self.confidence_sharpening);
                let confidence = if sharpened.is_finite() {
                    sharpened.clamp(0.0, 1.0)
                } else {
                    0.0
                };

                RoutingResult {
                    agent_name: agent.name.clone(),
                    confidence,
                    breakdown: RoutingBreakdown {
                        pattern_score,
                        capability_score,
                        learned_score,
                        priority_score,
                        context_score,
                        semantic_score,
                    },
                }
            })
            .collect();

        results.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.agent_name.cmp(&b.agent_name))
        });
        results
    }

    /// Check agent's regex patterns against the task text.
    /// Returns the fraction of patterns that match (0.0 to 1.0).
    /// Patterns are automatically made case-insensitive. A leading word boundary
    /// is added to each alternative to prevent mid-word matches (e.g. "sync"
    /// won't match inside "async"), while allowing suffix variations (e.g.
    /// "document" matches "documentation").
    fn compute_pattern_score(&self, task: &str, agent: &AgentDef) -> f64 {
        if agent.patterns.is_empty() {
            return 0.0;
        }

        let mut matches = 0usize;
        for pattern_str in &agent.patterns {
            // Add leading word boundary if pattern doesn't already use anchors/boundaries
            let wrapped = if pattern_str.contains("\\b")
                || pattern_str.starts_with('^')
                || pattern_str.ends_with('$')
            {
                format!("(?i){pattern_str}")
            } else {
                // Wrap each alternative with a leading \b to prevent mid-word matches
                let bounded = pattern_str
                    .split('|')
                    .map(|alt| format!("\\b(?:{alt})"))
                    .collect::<Vec<_>>()
                    .join("|");
                format!("(?i){bounded}")
            };

            match Regex::new(&wrapped) {
                Ok(re) => {
                    if re.is_match(task) {
                        matches += 1;
                    }
                }
                Err(e) => {
                    warn!(
                        "Invalid regex pattern '{}' for agent '{}': {e}",
                        pattern_str, agent.name
                    );
                }
            }
        }

        matches as f64 / agent.patterns.len() as f64
    }

    /// Count keyword overlap between task words and agent capabilities.
    /// Normalized by the number of capabilities (max possible matches).
    fn compute_capability_score(&self, task: &str, agent: &AgentDef) -> f64 {
        if agent.capabilities.is_empty() {
            return 0.0;
        }

        let task_lower = task.to_lowercase();
        let task_words: Vec<&str> = task_lower.split_whitespace().collect();

        let matches = agent
            .capabilities
            .iter()
            .filter(|cap| {
                let cap_lower = cap.to_lowercase();
                // Check if the capability appears as a word or substring in the task
                task_words
                    .iter()
                    .any(|word| word.contains(&cap_lower) || cap_lower.contains(word))
            })
            .count();

        matches as f64 / agent.capabilities.len() as f64
    }

    /// Look up learned routing weights for this task/agent pair.
    /// Returns 0.5 as default if no learned weight is found.
    fn compute_learned_score(
        &self,
        task: &str,
        agent: &AgentDef,
        learned_weights: &HashMap<(String, String), f64>,
    ) -> f64 {
        // Check for exact or pattern-based matches in the learned weights
        for ((task_pattern, agent_name), weight) in learned_weights {
            if agent_name == &agent.name {
                // Prefer substring match (fast path, no regex compilation)
                if task.contains(task_pattern.as_str()) {
                    return *weight;
                }
                // Only try regex if pattern contains metacharacters
                if task_pattern.contains(|c: char| ".*+?()[]{}|\\^$".contains(c)) {
                    if let Ok(re) = Regex::new(task_pattern) {
                        if re.is_match(task) {
                            return *weight;
                        }
                    }
                }
            }
        }
        0.5 // default when no learned weight found
    }

    /// Compute a context score based on session state.
    /// Uses 3 signals: file extension affinity, tool usage affinity, and continuity bonus.
    fn compute_context_score(&self, agent: &AgentDef, context: &RoutingContext) -> f64 {
        let mut score = 0.0;
        let mut signals = 0;

        // Signal 1: File extension affinity
        if !context.active_file_extensions.is_empty() {
            signals += 1;
            let lang_matches = context
                .active_file_extensions
                .iter()
                .filter(|ext| {
                    let lang = ext_to_language(ext);
                    agent
                        .capabilities
                        .iter()
                        .any(|cap| cap.eq_ignore_ascii_case(lang))
                })
                .count();
            if lang_matches > 0 {
                score +=
                    (lang_matches as f64 / context.active_file_extensions.len() as f64).min(1.0);
            }
        }

        // Signal 2: Tool usage affinity
        if !context.recent_tools.is_empty() {
            signals += 1;
            let coding_tools = context
                .recent_tools
                .iter()
                .filter(|t| matches!(t.as_str(), "Write" | "Edit" | "NotebookEdit"))
                .count();
            let research_tools = context
                .recent_tools
                .iter()
                .filter(|t| {
                    matches!(
                        t.as_str(),
                        "Read" | "Bash" | "Grep" | "Glob" | "WebSearch" | "WebFetch"
                    )
                })
                .count();

            let is_coding_agent = agent.capabilities.iter().any(|c| {
                let cl = c.to_lowercase();
                cl.contains("code") || cl.contains("implement") || cl.contains("refactor")
            });
            let is_research_agent = agent.capabilities.iter().any(|c| {
                let cl = c.to_lowercase();
                cl.contains("research") || cl.contains("explore") || cl.contains("review")
            });

            let tool_match = (is_coding_agent && coding_tools > research_tools)
                || (is_research_agent && research_tools > coding_tools);
            if tool_match {
                score += 0.7;
            } else {
                score += 0.3;
            }
        }

        // Signal 3: Continuity bonus — avoid unnecessary agent switching
        if let Some(ref active) = context.active_agent {
            signals += 1;
            if agent.name.eq_ignore_ascii_case(active)
                || agent
                    .capabilities
                    .iter()
                    .any(|c| c.eq_ignore_ascii_case(active))
            {
                score += 0.3;
            }
        }

        if signals == 0 {
            0.5 // neutral when no context available
        } else {
            (score / signals as f64).min(1.0)
        }
    }
}

/// Map file extension to a language name for capability matching.
fn ext_to_language(ext: &str) -> &str {
    match ext {
        "rs" => "rust",
        "py" => "python",
        "js" => "javascript",
        "ts" | "tsx" => "typescript",
        "go" => "go",
        "java" => "java",
        "rb" => "ruby",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "sh" | "bash" | "zsh" => "shell",
        "sql" => "sql",
        "md" | "mdx" => "markdown",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "json" => "json",
        "html" | "htm" => "html",
        "css" | "scss" | "sass" => "css",
        _ => ext,
    }
}

impl Default for AgentRouter {
    fn default() -> Self {
        Self::new(&RoutingConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowforge_core::{AgentSource, Priority};

    fn make_agent(name: &str, caps: &[&str], patterns: &[&str], priority: Priority) -> AgentDef {
        AgentDef {
            name: name.to_string(),
            description: String::new(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
            priority,
            color: None,
            routing_category: RoutingCategory::Core,
            body: String::new(),
            source: AgentSource::BuiltIn,
        }
    }

    #[test]
    fn test_route_pattern_match() {
        let router = AgentRouter::default();
        // Single pattern — should score 1.0 when it matches
        let agent = make_agent("tester", &["test"], &["test"], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&agent];
        let learned = HashMap::new();

        let results = router.route("test the login flow", &agents, &learned, None, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].breakdown.pattern_score, 1.0);
    }

    #[test]
    fn test_route_partial_pattern_match() {
        let router = AgentRouter::default();
        // Two patterns — only "test" matches, "spec" doesn't → 0.5
        let agent = make_agent("tester", &["test"], &["test", "spec"], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&agent];
        let learned = HashMap::new();

        let results = router.route("test the login flow", &agents, &learned, None, None);
        assert!((results[0].breakdown.pattern_score - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_route_no_pattern_match() {
        let router = AgentRouter::default();
        let agent = make_agent("tester", &["test"], &["^deploy.*"], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&agent];
        let learned = HashMap::new();

        let results = router.route("test the login flow", &agents, &learned, None, None);
        assert_eq!(results[0].breakdown.pattern_score, 0.0);
    }

    #[test]
    fn test_route_word_boundary() {
        let router = AgentRouter::default();
        // "sync" should NOT match "async" — leading \b prevents mid-word matches
        let sync_agent = make_agent("syncer", &["sync"], &["sync"], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&sync_agent];
        let learned = HashMap::new();

        let results = router.route("fix async handler", &agents, &learned, None, None);
        assert_eq!(results[0].breakdown.pattern_score, 0.0);

        // But should match when "sync" is a standalone word
        let results = router.route("sync the database", &agents, &learned, None, None);
        assert_eq!(results[0].breakdown.pattern_score, 1.0);

        // "document" should match "documentation" (leading boundary, suffix allowed)
        let doc_agent = make_agent("doc", &[], &["document"], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&doc_agent];
        let results = router.route("write documentation", &agents, &learned, None, None);
        assert_eq!(results[0].breakdown.pattern_score, 1.0);
    }

    #[test]
    fn test_route_capability_score() {
        let router = AgentRouter::default();
        let agent = make_agent(
            "reviewer",
            &["rust", "review", "lint"],
            &[],
            Priority::Normal,
        );
        let agents: Vec<&AgentDef> = vec![&agent];
        let learned = HashMap::new();

        let results = router.route("review this rust code", &agents, &learned, None, None);
        let cap_score = results[0].breakdown.capability_score;
        // "rust" and "review" should match out of 3 capabilities
        assert!((cap_score - 2.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_route_sorting() {
        let router = AgentRouter::default();
        let low = make_agent("low", &[], &[], Priority::Low);
        let high = make_agent("high", &["test"], &["test.*"], Priority::High);
        let agents: Vec<&AgentDef> = vec![&low, &high];
        let learned = HashMap::new();

        let results = router.route("test something", &agents, &learned, None, None);
        assert_eq!(results[0].agent_name, "high");
        assert_eq!(results[1].agent_name, "low");
    }

    #[test]
    fn test_route_learned_weights() {
        let router = AgentRouter::default();
        let agent_a = make_agent("agent-a", &[], &[], Priority::Normal);
        let agent_b = make_agent("agent-b", &[], &[], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&agent_a, &agent_b];

        let mut learned = HashMap::new();
        learned.insert(("deploy".to_string(), "agent-b".to_string()), 0.9);

        let results = router.route("deploy the service", &agents, &learned, None, None);
        // agent-b should rank higher due to the learned weight
        assert_eq!(results[0].agent_name, "agent-b");
    }

    #[test]
    fn test_route_empty_agents() {
        let router = AgentRouter::default();
        let agents: Vec<&AgentDef> = vec![];
        let learned = HashMap::new();

        let results = router.route("anything", &agents, &learned, None, None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_route_context_file_extension_affinity() {
        let router = AgentRouter::default();
        let rust_agent = make_agent("rust-dev", &["rust", "code"], &[], Priority::Normal);
        let py_agent = make_agent("py-dev", &["python", "code"], &[], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&rust_agent, &py_agent];
        let learned = HashMap::new();

        let context = RoutingContext {
            active_file_extensions: vec!["rs".to_string()],
            ..Default::default()
        };

        let results = router.route("fix a bug", &agents, &learned, Some(&context), None);
        // rust-dev should rank higher because we're editing .rs files
        assert_eq!(results[0].agent_name, "rust-dev");
    }

    #[test]
    fn test_route_context_continuity_bonus() {
        let router = AgentRouter::default();
        let agent_a = make_agent("coder", &["code"], &[], Priority::Normal);
        let agent_b = make_agent("reviewer", &["code"], &[], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&agent_a, &agent_b];
        let learned = HashMap::new();

        let context = RoutingContext {
            active_agent: Some("coder".to_string()),
            ..Default::default()
        };

        let results = router.route("write some code", &agents, &learned, Some(&context), None);
        // "coder" should get a continuity bonus
        assert_eq!(results[0].agent_name, "coder");
        assert!(results[0].breakdown.context_score > results[1].breakdown.context_score);
    }

    #[test]
    fn test_route_deterministic_tiebreak() {
        let router = AgentRouter::default();
        let agent_a = make_agent("alpha", &[], &[], Priority::Normal);
        let agent_b = make_agent("beta", &[], &[], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&agent_b, &agent_a];
        let learned = HashMap::new();

        let results = router.route("anything", &agents, &learned, None, None);
        // Same confidence → sorted alphabetically
        assert_eq!(results[0].agent_name, "alpha");
        assert_eq!(results[1].agent_name, "beta");
    }

    #[test]
    fn test_route_nan_weight_protection() {
        // Router should handle NaN/Inf weights gracefully
        let config = RoutingConfig {
            pattern_weight: f64::NAN,
            capability_weight: f64::INFINITY,
            learned_weight: f64::NEG_INFINITY,
            priority_weight: 0.5,
            context_weight: 0.5,
            semantic_weight: 0.0,
            confidence_sharpening: 0.0,
        };
        let router = AgentRouter::new(&config);
        // NaN/Inf weights should be sanitized to 0.0
        assert_eq!(router.pattern_weight, 0.0);
        assert_eq!(router.capability_weight, 0.0);
        assert_eq!(router.learned_weight, 0.0);

        let agent = make_agent("test", &["test"], &["test"], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&agent];
        let learned = HashMap::new();

        let results = router.route("test task", &agents, &learned, None, None);
        assert!(!results.is_empty());
        let confidence = results[0].confidence;
        assert!(
            confidence.is_finite(),
            "Confidence {confidence} is not finite"
        );
        assert!(
            (0.0..=1.0).contains(&confidence),
            "Confidence {confidence} out of [0.0, 1.0]"
        );
    }

    #[test]
    fn test_route_confidence_clamped() {
        // With default weights summing to 1.0 and all scores at 1.0,
        // confidence should be clamped to 1.0
        let router = AgentRouter::default();
        let agent = make_agent(
            "perfect",
            &["test", "code", "review"],
            &["test"],
            Priority::Critical,
        );
        let agents: Vec<&AgentDef> = vec![&agent];
        let mut learned = HashMap::new();
        learned.insert(("test".to_string(), "perfect".to_string()), 1.0);

        let results = router.route("test", &agents, &learned, None, None);
        assert!(results[0].confidence <= 1.0);
        assert!(results[0].confidence >= 0.0);
    }

    #[test]
    fn test_ext_to_language() {
        assert_eq!(ext_to_language("rs"), "rust");
        assert_eq!(ext_to_language("py"), "python");
        assert_eq!(ext_to_language("ts"), "typescript");
        assert_eq!(ext_to_language("tsx"), "typescript");
        assert_eq!(ext_to_language("unknown"), "unknown");
    }

    #[test]
    fn test_sharpen_confidence() {
        // Midpoint stays at 0.5
        assert!((sharpen_confidence(0.5, 8.0) - 0.5).abs() < 0.01);
        // Low raw → pushed lower
        assert!(sharpen_confidence(0.3, 8.0) < 0.2);
        // High raw → pushed higher
        assert!(sharpen_confidence(0.65, 8.0) > 0.75);
        // k=0 disables sharpening
        assert!((sharpen_confidence(0.3, 0.0) - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_semantic_scores_boost_confidence() {
        let router = AgentRouter::default();
        let agent_a = make_agent("agent-a", &[], &[], Priority::Normal);
        let agent_b = make_agent("agent-b", &[], &[], Priority::Normal);
        let agents: Vec<&AgentDef> = vec![&agent_a, &agent_b];
        let learned = HashMap::new();

        let mut semantic = HashMap::new();
        semantic.insert("agent-b".to_string(), 0.9);
        semantic.insert("agent-a".to_string(), 0.1);

        let results = router.route("anything", &agents, &learned, None, Some(&semantic));
        // agent-b should rank higher due to semantic score
        assert_eq!(results[0].agent_name, "agent-b");
        assert!(results[0].breakdown.semantic_score > results[1].breakdown.semantic_score);
    }

    #[test]
    fn test_agent_filtering_swarm_only() {
        let router = AgentRouter::default();
        let core_agent = make_agent("coder", &["code"], &[], Priority::Normal);
        let mut swarm_agent = make_agent("swarm-lead", &["coordinate"], &[], Priority::Normal);
        swarm_agent.routing_category = RoutingCategory::SwarmOnly;
        let agents: Vec<&AgentDef> = vec![&core_agent, &swarm_agent];
        let learned = HashMap::new();

        // Normal task: swarm agent should be filtered out
        let results = router.route("fix a bug in the parser", &agents, &learned, None, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_name, "coder");

        // Swarm task: swarm agent should be included
        let results = router.route("coordinate team review", &agents, &learned, None, None);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_agent_filtering_workflow_only() {
        let router = AgentRouter::default();
        let core_agent = make_agent("coder", &["code"], &[], Priority::Normal);
        let mut workflow_agent = make_agent("sparc", &["workflow"], &[], Priority::Normal);
        workflow_agent.routing_category = RoutingCategory::WorkflowOnly;
        let agents: Vec<&AgentDef> = vec![&core_agent, &workflow_agent];
        let learned = HashMap::new();

        // Normal task: workflow agent filtered out
        let results = router.route("fix a bug", &agents, &learned, None, None);
        assert_eq!(results.len(), 1);

        // Workflow task: workflow agent included
        let results = router.route("automate the deployment pipeline", &agents, &learned, None, None);
        assert_eq!(results.len(), 2);
    }
}
