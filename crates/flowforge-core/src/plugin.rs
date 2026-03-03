//! Plugin SDK: TOML-based extensions for custom tools, hooks, and agents.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::config::{FlowForgeConfig, PluginsConfig};
use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub tools: Vec<PluginToolDef>,
    #[serde(default)]
    pub hooks: Vec<PluginHookDef>,
    #[serde(default)]
    pub agents: Vec<PluginAgentRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolDef {
    pub name: String,
    pub description: String,
    pub command: String,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub input_schema: Option<String>,
}

fn default_timeout() -> u64 {
    5000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHookDef {
    pub event: String,
    pub command: String,
    #[serde(default = "default_priority")]
    pub priority: i32,
}

fn default_priority() -> i32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAgentRef {
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
}

/// Check if a plugin is enabled given the config.
pub fn is_plugin_enabled(name: &str, config: &PluginsConfig) -> bool {
    // Disabled list takes precedence
    if config.disabled.iter().any(|d| d == name) {
        return false;
    }
    // If enabled list is empty, all are enabled
    if config.enabled.is_empty() {
        return true;
    }
    config.enabled.iter().any(|e| e == name)
}

/// Load a single plugin from a directory.
pub fn load_plugin(dir: &Path) -> Result<LoadedPlugin> {
    let manifest_path = dir.join("plugin.toml");
    if !manifest_path.exists() {
        return Err(crate::Error::Plugin(format!(
            "No plugin.toml found in {}",
            dir.display()
        )));
    }

    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: PluginManifest = toml::from_str(&content)
        .map_err(|e| crate::Error::Plugin(format!("Invalid plugin.toml: {e}")))?;

    Ok(LoadedPlugin {
        manifest,
        dir: dir.to_path_buf(),
    })
}

/// Load all plugins from the plugins directory.
pub fn load_all_plugins(config: &PluginsConfig) -> Result<Vec<LoadedPlugin>> {
    let plugins_dir = FlowForgeConfig::plugins_dir();
    if !plugins_dir.exists() {
        return Ok(vec![]);
    }

    let mut plugins = Vec::new();
    let entries = std::fs::read_dir(&plugins_dir)?;

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        match load_plugin(&path) {
            Ok(plugin) => {
                if is_plugin_enabled(&plugin.manifest.plugin.name, config) {
                    plugins.push(plugin);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load plugin from {}: {e}", path.display());
            }
        }
    }

    Ok(plugins)
}
