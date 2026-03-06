use std::collections::HashMap;
use std::path::PathBuf;

use flowforge_core::config::AgentsConfig;
use flowforge_core::{AgentDef, AgentSource, FlowForgeConfig, Result};
use tracing::debug;

use crate::loader;

/// Registry that holds all loaded agent definitions, keyed by name.
pub struct AgentRegistry {
    agents: HashMap<String, AgentDef>,
}

impl AgentRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    /// Load agents from all configured sources.
    ///
    /// Loading order (later sources override earlier ones by agent name):
    /// 1. Built-in agents
    /// 2. Global agents from `~/.flowforge/agents/`
    /// 3. Project agents from `.flowforge/agents/`
    /// 4. Project agents from `.claude/agents/`
    pub fn load(config: &AgentsConfig) -> Result<Self> {
        let mut registry = Self::new();

        // 1. Built-in agents
        if config.load_builtin {
            let builtins = loader::load_builtin();
            debug!("Loaded {} built-in agents", builtins.len());
            for agent in builtins {
                registry.agents.insert(agent.name.clone(), agent);
            }
        }

        // 2. Global agents from ~/.flowforge/agents/
        if config.load_global {
            let global_dir = FlowForgeConfig::global_dir().join("agents");
            let globals = loader::load_from_dir(&global_dir, AgentSource::Global)?;
            debug!(
                "Loaded {} global agents from {}",
                globals.len(),
                global_dir.display()
            );
            for agent in globals {
                if let Some(existing) = registry.agents.get(&agent.name) {
                    if matches!(existing.source, AgentSource::BuiltIn) {
                        eprintln!(
                            "[FlowForge] Agent '{}' overrides built-in (source: global)",
                            agent.name
                        );
                    }
                }
                registry.agents.insert(agent.name.clone(), agent);
            }
        }

        // 3. Project agents from .flowforge/agents/ and .claude/agents/
        if config.load_project {
            let project_dirs: Vec<PathBuf> = vec![
                FlowForgeConfig::project_dir().join("agents"),
                PathBuf::from(".claude/agents"),
            ];

            for dir in &project_dirs {
                let project_agents = loader::load_from_dir(dir, AgentSource::Project)?;
                debug!(
                    "Loaded {} project agents from {}",
                    project_agents.len(),
                    dir.display()
                );
                for agent in project_agents {
                    if let Some(existing) = registry.agents.get(&agent.name) {
                        if matches!(existing.source, AgentSource::BuiltIn) {
                            eprintln!(
                                "[FlowForge] Agent '{}' overrides built-in (source: project)",
                                agent.name
                            );
                        }
                    }
                    registry.agents.insert(agent.name.clone(), agent);
                }
            }
        }

        // 4. Plugin agents
        let plugins_config = flowforge_core::config::PluginsConfig::default();
        if let Ok(plugins) = flowforge_core::plugin::load_all_plugins(&plugins_config) {
            for plugin in &plugins {
                for agent_ref in &plugin.manifest.agents {
                    let agent_path = plugin.dir.join(&agent_ref.path);
                    if agent_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&agent_path) {
                            if let Ok(mut agent) = loader::parse_agent_def(&content) {
                                agent.source =
                                    AgentSource::Plugin(plugin.manifest.plugin.name.clone());
                                if let Some(existing) = registry.agents.get(&agent.name) {
                                    if matches!(existing.source, AgentSource::BuiltIn) {
                                        eprintln!(
                                            "[FlowForge] Agent '{}' overrides built-in (source: plugin:{})",
                                            agent.name, plugin.manifest.plugin.name
                                        );
                                    }
                                }
                                registry.agents.insert(agent.name.clone(), agent);
                            }
                        }
                    }
                }
            }
        }

        Ok(registry)
    }

    /// Get an agent by name.
    pub fn get(&self, name: &str) -> Option<&AgentDef> {
        self.agents.get(name)
    }

    /// List all registered agents, sorted by name.
    pub fn list(&self) -> Vec<&AgentDef> {
        let mut agents: Vec<&AgentDef> = self.agents.values().collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        agents
    }

    /// Search for agents matching a text query.
    /// Checks name, description, and capabilities for substring matches.
    pub fn search(&self, query: &str) -> Vec<&AgentDef> {
        let query_lower = query.to_lowercase();
        self.agents
            .values()
            .filter(|agent| {
                agent.name.to_lowercase().contains(&query_lower)
                    || agent.description.to_lowercase().contains(&query_lower)
                    || agent
                        .capabilities
                        .iter()
                        .any(|cap| cap.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Number of registered agents.
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// Insert an agent definition directly (useful for testing).
    pub fn insert(&mut self, agent: AgentDef) {
        self.agents.insert(agent.name.clone(), agent);
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowforge_core::Priority;

    fn make_agent(name: &str, desc: &str, caps: &[&str]) -> AgentDef {
        AgentDef {
            name: name.to_string(),
            description: desc.to_string(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            patterns: Vec::new(),
            priority: Priority::Normal,
            color: None,
            routing_category: flowforge_core::RoutingCategory::Core,
            body: String::new(),
            source: AgentSource::BuiltIn,
        }
    }

    #[test]
    fn test_registry_insert_and_get() {
        let mut reg = AgentRegistry::new();
        reg.insert(make_agent("test", "Test agent", &["rust"]));
        assert_eq!(reg.len(), 1);
        assert!(reg.get("test").is_some());
        assert!(reg.get("missing").is_none());
    }

    #[test]
    fn test_registry_override() {
        let mut reg = AgentRegistry::new();
        reg.insert(make_agent("test", "Original", &[]));
        reg.insert(make_agent("test", "Override", &[]));
        assert_eq!(reg.get("test").unwrap().description, "Override");
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn test_registry_search() {
        let mut reg = AgentRegistry::new();
        reg.insert(make_agent(
            "code-review",
            "Reviews code",
            &["rust", "review"],
        ));
        reg.insert(make_agent("test-runner", "Runs tests", &["testing"]));
        reg.insert(make_agent("deployer", "Deploys services", &["deploy"]));

        let results = reg.search("rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "code-review");

        let results = reg.search("test");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "test-runner");

        let results = reg.search("deploy");
        assert_eq!(results.len(), 1); // matches both name and capability of deployer
    }
}
