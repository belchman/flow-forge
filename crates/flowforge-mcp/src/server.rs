use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info};

use crate::tools::ToolRegistry;

const SERVER_NAME: &str = "flowforge";
const SERVER_VERSION: &str = "0.1.0";
const PROTOCOL_VERSION: &str = "2024-11-05";

pub struct McpServer {
    tools: ToolRegistry,
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            tools: ToolRegistry::new(),
        }
    }

    pub async fn run(&self) -> flowforge_core::Result<()> {
        info!("FlowForge MCP server starting");

        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader
                .read_line(&mut line)
                .await
                .map_err(|e| flowforge_core::Error::Mcp(format!("stdin read error: {}", e)))?;

            if bytes_read == 0 {
                info!("stdin closed, shutting down");
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            debug!("received: {}", trimmed);

            let request: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    let err_response = json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {
                            "code": -32700,
                            "message": format!("Parse error: {}", e)
                        }
                    });
                    let mut out = serde_json::to_string(&err_response).unwrap_or_default();
                    out.push('\n');
                    stdout.write_all(out.as_bytes()).await.map_err(|e| {
                        flowforge_core::Error::Mcp(format!("stdout write error: {}", e))
                    })?;
                    stdout.flush().await.ok();
                    continue;
                }
            };

            let response = self.handle_request(&request);

            // Notifications (no id) don't require a response
            if request.get("id").is_none() {
                continue;
            }

            let mut out = serde_json::to_string(&response)
                .map_err(|e| flowforge_core::Error::Mcp(format!("serialize error: {}", e)))?;
            out.push('\n');

            debug!("sending: {}", out.trim());

            stdout
                .write_all(out.as_bytes())
                .await
                .map_err(|e| flowforge_core::Error::Mcp(format!("stdout write error: {}", e)))?;
            stdout
                .flush()
                .await
                .map_err(|e| flowforge_core::Error::Mcp(format!("stdout flush error: {}", e)))?;
        }

        Ok(())
    }

    fn handle_request(&self, request: &Value) -> Value {
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        match method {
            "initialize" => self.handle_initialize(id),
            "notifications/initialized" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            }),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(id, request),
            "ping" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            }),
            _ => {
                error!("unknown method: {}", method);
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: {}", method)
                    }
                })
            }
        }
    }

    fn handle_initialize(&self, id: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                }
            }
        })
    }

    fn handle_tools_list(&self, id: Value) -> Value {
        let tools: Vec<Value> = self
            .tools
            .list()
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema
                })
            })
            .collect();

        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": tools
            }
        })
    }

    fn handle_tools_call(&self, id: Value, request: &Value) -> Value {
        let params = request.get("params").cloned().unwrap_or(json!({}));
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        if self.tools.get(tool_name).is_none() {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32602,
                    "message": format!("Unknown tool: {}", tool_name)
                }
            });
        }

        let result = self.tools.call(tool_name, &arguments);

        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [
                    {
                        "type": "text",
                        "text": serde_json::to_string_pretty(&result).unwrap_or_default()
                    }
                ]
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_initialize() {
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let resp = server.handle_request(&req);
        assert_eq!(resp["id"], 1);
        assert!(resp["result"]["protocolVersion"].is_string());
        assert_eq!(resp["result"]["serverInfo"]["name"], "flowforge");
    }

    #[test]
    fn test_handle_tools_list() {
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        });
        let resp = server.handle_request(&req);
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 36);
    }

    #[test]
    fn test_handle_tools_call() {
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "memory_get",
                "arguments": { "key": "test-key" }
            }
        });
        let resp = server.handle_request(&req);
        assert!(resp["result"]["content"].is_array());
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("test-key"));
    }

    #[test]
    fn test_handle_unknown_tool() {
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "nonexistent",
                "arguments": {}
            }
        });
        let resp = server.handle_request(&req);
        assert!(resp["error"].is_object());
    }

    #[test]
    fn test_handle_unknown_method() {
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "bogus/method",
            "params": {}
        });
        let resp = server.handle_request(&req);
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Method not found"));
    }

    #[test]
    fn test_handle_ping() {
        let server = McpServer::new();
        let req = json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "ping",
            "params": {}
        });
        let resp = server.handle_request(&req);
        assert!(resp["result"].is_object());
    }
}
