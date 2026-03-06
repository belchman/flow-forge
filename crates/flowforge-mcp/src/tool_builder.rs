use serde_json::{json, Value};
use std::collections::HashMap;

use crate::tools::ToolDef;

/// Builder for constructing `ToolDef` registrations with minimal boilerplate.
pub struct ToolBuilder<'a> {
    registry: &'a mut HashMap<String, ToolDef>,
    name: String,
    description: String,
    properties: Vec<(String, Value)>,
    required: Vec<String>,
}

impl<'a> ToolBuilder<'a> {
    pub fn new(registry: &'a mut HashMap<String, ToolDef>, name: &str, description: &str) -> Self {
        Self {
            registry,
            name: name.to_string(),
            description: description.to_string(),
            properties: Vec::new(),
            required: Vec::new(),
        }
    }

    pub fn required_str(mut self, name: &str, desc: &str) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "string", "description": desc}),
        ));
        self.required.push(name.to_string());
        self
    }

    pub fn optional_str(mut self, name: &str, desc: &str) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "string", "description": desc}),
        ));
        self
    }

    pub fn required_bool(mut self, name: &str, desc: &str) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "boolean", "description": desc}),
        ));
        self.required.push(name.to_string());
        self
    }

    pub fn optional_bool(mut self, name: &str, desc: &str) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "boolean", "description": desc}),
        ));
        self
    }

    pub fn optional_int(mut self, name: &str, desc: &str) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "integer", "description": desc}),
        ));
        self
    }

    pub fn optional_int_default(mut self, name: &str, desc: &str, default: u64) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "integer", "description": desc, "default": default}),
        ));
        self
    }

    pub fn optional_str_default(mut self, name: &str, desc: &str, default: &str) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "string", "description": desc, "default": default}),
        ));
        self
    }

    pub fn optional_num(mut self, name: &str, desc: &str) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "number", "description": desc}),
        ));
        self
    }

    pub fn optional_num_default(mut self, name: &str, desc: &str, default: f64) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "number", "description": desc, "default": default}),
        ));
        self
    }

    pub fn required_array(mut self, name: &str, desc: &str, items_schema: Value) -> Self {
        self.properties.push((
            name.to_string(),
            json!({"type": "array", "items": items_schema, "description": desc}),
        ));
        self.required.push(name.to_string());
        self
    }

    pub fn build(self) {
        let mut props = serde_json::Map::new();
        for (k, v) in self.properties {
            props.insert(k, v);
        }
        let mut schema = json!({
            "type": "object",
            "properties": Value::Object(props)
        });
        if !self.required.is_empty() {
            schema["required"] = json!(self.required);
        }
        self.registry.insert(
            self.name.clone(),
            ToolDef {
                name: self.name,
                description: self.description,
                input_schema: schema,
            },
        );
    }
}

/// Extension trait on the tool HashMap to create builders fluently.
pub trait ToolBuilderExt {
    fn tool(&mut self, name: &str, description: &str) -> ToolBuilder<'_>;
}

impl ToolBuilderExt for HashMap<String, ToolDef> {
    fn tool(&mut self, name: &str, description: &str) -> ToolBuilder<'_> {
        ToolBuilder::new(self, name, description)
    }
}
