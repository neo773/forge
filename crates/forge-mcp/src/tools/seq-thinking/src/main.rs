use std::collections::HashMap;

use anyhow::Result;
use colored::Colorize;
use forge_mcp::server::Server;
use forge_mcp::transport::ServerStdioTransport;
use forge_mcp::types::{
    CallToolRequest, CallToolResponse, ListRequest, ResourcesListResponse, ServerCapabilities,
    ToolResponseContent, ToolsListResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ThoughtData {
    thought: String,
    thought_number: i32,
    total_thoughts: i32,
    next_thought_needed: bool,
    is_revision: Option<bool>,
    revises_thought: Option<i32>,
    branch_from_thought: Option<i32>,
    branch_id: Option<String>,
    needs_more_thoughts: Option<bool>,
}

#[derive(Clone)]
struct SequentialThinkingServer {
    thought_history: Vec<ThoughtData>,
    branches: HashMap<String, Vec<ThoughtData>>,
}

impl SequentialThinkingServer {
    fn new() -> Self {
        Self {
            thought_history: Vec::new(),
            branches: HashMap::new(),
        }
    }

    fn validate_thought_data(&self, input: serde_json::Value) -> Result<ThoughtData> {
        let thought_data: ThoughtData = serde_json::from_value(input)?;

        if thought_data.thought_number <= 0 {
            return Err(anyhow::anyhow!("Invalid thoughtNumber: must be positive"));
        }
        if thought_data.total_thoughts <= 0 {
            return Err(anyhow::anyhow!("Invalid totalThoughts: must be positive"));
        }

        Ok(thought_data)
    }

    fn format_thought(&self, thought_data: &ThoughtData) -> String {
        let (prefix, context) = match (thought_data.is_revision, &thought_data.branch_from_thought)
        {
            (Some(true), _) => (
                "🔄 Revision".yellow().to_string(),
                format!(
                    " (revising thought {})",
                    thought_data.revises_thought.unwrap_or(0)
                ),
            ),
            (_, Some(branch)) => (
                "🌿 Branch".green().to_string(),
                format!(
                    " (from thought {}, ID: {})",
                    branch,
                    thought_data.branch_id.as_ref().unwrap_or(&String::new())
                ),
            ),
            _ => ("💭 Thought".blue().to_string(), String::new()),
        };

        let header = format!(
            "{} {}/{}{}",
            prefix, thought_data.thought_number, thought_data.total_thoughts, context
        );
        let border_len = header.len().max(thought_data.thought.len()) + 4;
        let border = "─".repeat(border_len);

        format!(
            "\n┌{}┐\n│ {} │\n├{}┤\n│ {} │\n└{}┘",
            border,
            header,
            border,
            format!("{:width$}", thought_data.thought, width = border_len - 2),
            border
        )
    }

    fn process_thought(&mut self, input: serde_json::Value) -> Result<serde_json::Value> {
        let mut thought_data = self.validate_thought_data(input)?;

        if thought_data.thought_number > thought_data.total_thoughts {
            thought_data.total_thoughts = thought_data.thought_number;
        }

        self.thought_history.push(thought_data.clone());

        if let (Some(branch_from), Some(branch_id)) =
            (thought_data.branch_from_thought, &thought_data.branch_id)
        {
            self.branches
                .entry(branch_id.clone())
                .or_default()
                .push(thought_data.clone());
        }

        eprintln!("{}", self.format_thought(&thought_data));

        let result = serde_json::json!({
            "thoughtNumber": thought_data.thought_number,
            "totalThoughts": thought_data.total_thoughts,
            "nextThoughtNeeded": thought_data.next_thought_needed,
            "branches": self.branches.keys().collect::<Vec<_>>(),
            "thoughtHistoryLength": self.thought_history.len()
        });

        Ok(result)
    }
}

fn call_tool(
    req: CallToolRequest,
    thinking_server: &mut SequentialThinkingServer,
) -> Result<CallToolResponse> {
    let name = req.name.as_str();
    let args = req.arguments.unwrap_or_default();
    let args = serde_json::to_value(args)?;
    let result = match name {
        "sequentialthinking" => {
            let thought_result = thinking_server.process_thought(args)?;
            ToolResponseContent::Text {
                text: serde_json::to_string(&thought_result)?,
            }
        }
        _ => return Err(anyhow::anyhow!("Unknown tool: {}", req.name)),
    };
    Ok(CallToolResponse {
        content: vec![result],
        is_error: None,
        meta: None,
    })
}

fn list_tools(_req: ListRequest) -> Result<ToolsListResponse> {
    let response = json!({
      "tools": [
        {
          "name": "sequentialthinking",
          "description": "A detailed tool for dynamic and reflective problem-solving through thoughts.",
          "inputSchema": {
            "type": "object",
            "properties": {
              "thought": {
                "type": "string",
                "description": "Your current thinking step"
              },
              "nextThoughtNeeded": {
                "type": "boolean",
                "description": "Whether another thought step is needed"
              },
              "thoughtNumber": {
                "type": "integer",
                "description": "Current thought number",
                "minimum": 1
              },
              "totalThoughts": {
                "type": "integer",
                "description": "Estimated total thoughts needed",
                "minimum": 1
              },
              "isRevision": {
                "type": "boolean",
                "description": "Whether this revises previous thinking"
              },
              "revisesThought": {
                "type": "integer",
                "description": "Which thought is being reconsidered",
                "minimum": 1
              },
              "branchFromThought": {
                "type": "integer",
                "description": "Branching point thought number",
                "minimum": 1
              },
              "branchId": {
                "type": "string",
                "description": "Branch identifier"
              },
              "needsMoreThoughts": {
                "type": "boolean",
                "description": "If more thoughts are needed"
              }
            },
            "required": ["thought", "nextThoughtNeeded", "thoughtNumber", "totalThoughts"]
          },
        }
      ],
    });
    Ok(serde_json::from_value(response)?)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(std::io::stderr)
        .init();

    let thinking_server = SequentialThinkingServer::new();

    let t_server = thinking_server.clone();
    let server = Server::builder(ServerStdioTransport)
        .capabilities(ServerCapabilities {
            tools: Some(json!({})),
            ..Default::default()
        })
        .request_handler("tools/list", list_tools)
        .request_handler("tools/call", move |req| -> Result<CallToolResponse> {
            call_tools(req, t_server.clone())
        })
        .request_handler("resources/list", |_req: ListRequest| {
            Ok(ResourcesListResponse {
                resources: vec![],
                next_cursor: None,
                meta: None,
            })
        })
        .build();

    eprintln!("Sequential Thinking MCP Server running on stdio");

    server.listen().await?;

    Ok(())
}

fn call_tools(
    req: CallToolRequest,
    mut thinking_server: SequentialThinkingServer,
) -> Result<CallToolResponse> {
    call_tool(req, &mut thinking_server)
}
