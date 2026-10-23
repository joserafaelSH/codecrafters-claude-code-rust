use std::{fs, path::Path, process::Command};

use async_openai::{Client, config::OpenAIConfig};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::skills;

/// Error type for the agent. `Send + Sync` keeps it usable with `tokio::spawn` later.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

const MODEL: &str = "anthropic/claude-haiku-4.5";

/// Safety net: stop if the model keeps calling tools without ever answering.
const MAX_TURNS: usize = 50;

/// How many forked skills may run inside each other (a skill can call a skill).
const MAX_FORK_DEPTH: usize = 3;

/// Tool definitions are embedded at compile time, so they don't depend on the
/// directory the program is run from.
const TOOL_DEFINITIONS: [&str; 4] = [
    include_str!("tools/read_tool.json"),
    include_str!("tools/write_tool.json"),
    include_str!("tools/bash_tool.json"),
    include_str!("tools/skill_tool.json"),
];

#[derive(Debug, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    /// A JSON object encoded as a string, e.g. `{"file_path":"a.txt"}`.
    #[serde(default)]
    pub arguments: String,
}

#[derive(Debug, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub function: ToolCallFunction,
}

/// Result of a single tool: the text sent back to the model, or an error message.
type ToolResult = Result<String, String>;

pub struct Tools {
    client: Client<OpenAIConfig>,
    skills_map: skills::SkillsMap,
    definitions: Vec<Value>,
}

impl Tools {
    pub fn new(client: Client<OpenAIConfig>, skills_map: skills::SkillsMap) -> Self {
        let definitions = TOOL_DEFINITIONS
            .iter()
            .map(|text| serde_json::from_str(text).expect("tool definition is not valid JSON"))
            .collect();

        Self {
            client,
            skills_map,
            definitions,
        }
    }

    /// Runs the agent loop until the model answers without calling tools.
    /// Returns the model's final text answer.
    pub async fn run_agent(&self, messages: &mut Vec<Value>) -> Result<String, BoxError> {
        self.run_agent_at_depth(messages, 0).await
    }

    async fn run_agent_at_depth(
        &self,
        messages: &mut Vec<Value>,
        depth: usize,
    ) -> Result<String, BoxError> {
        for _ in 0..MAX_TURNS {
            let response: Value = self
                .client
                .chat()
                .create_byot(json!({
                    "model": MODEL,
                    "messages": messages,
                    "tools": &self.definitions,
                }))
                .await?;

            let message = response["choices"][0]["message"].clone();
            if message.is_null() {
                return Err(format!("unexpected API response: {}", response).into());
            }
            messages.push(message.clone());

            let tool_calls = parse_tool_calls(&message);
            if tool_calls.is_empty() {
                let answer = message["content"].as_str().unwrap_or_default();
                return Ok(answer.to_string());
            }

            // Every tool call must get exactly one "tool" message back,
            // including failures, or the next request is rejected.
            for tool_call in tool_calls {
                let content = self.execute_tool_call(&tool_call, depth).await;
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_call.id,
                    "content": content,
                }));
            }
        }

        Err(format!(
            "agent stopped after {} turns without a final answer",
            MAX_TURNS
        )
        .into())
    }

    /// Runs one tool and always returns text for the model.
    /// Errors are returned as text too, so the model can see them and retry.
    async fn execute_tool_call(&self, tool_call: &ToolCall, depth: usize) -> String {
        let name = tool_call.function.name.as_str();
        eprintln!("[tool] {} {}", name, tool_call.function.arguments);

        let result = match parse_arguments(&tool_call.function.arguments) {
            Err(e) => Err(e),
            Ok(args) => match name {
                "Read" => read_tool(&args),
                "Write" => write_tool(&args),
                "Bash" => bash_tool(&args),
                // Box::pin breaks the async recursion:
                // run_agent -> execute_tool_call -> skill_tool -> run_agent
                "Skill" => Box::pin(self.skill_tool(&args, depth)).await,
                other => Err(format!("unknown tool '{}'", other)),
            },
        };

        result.unwrap_or_else(|e| {
            eprintln!("[tool] {} failed: {}", name, e);
            format!("Error: {}", e)
        })
    }

    async fn skill_tool(&self, args: &Value, depth: usize) -> ToolResult {
        let skill_name = required_str(args, "name")?;
        let skill_args = args["args"].as_str().unwrap_or_default();

        let skill = self.skills_map.map.get(skill_name).ok_or_else(|| {
            let mut available: Vec<&str> = self.skills_map.map.keys().map(String::as_str).collect();
            available.sort();
            format!(
                "unknown skill '{}'. Available skills: {}",
                skill_name,
                available.join(", ")
            )
        })?;

        let content = skill
            .expand(skill_args)
            .ok_or_else(|| format!("could not read the file for skill '{}'", skill_name))?;

        if skill.context != "fork" {
            // Inline skill: its instructions go straight back into this conversation.
            return Ok(content);
        }

        // Forked skill: run it as a separate agent with its own message history.
        if depth >= MAX_FORK_DEPTH {
            return Err(format!(
                "skill '{}' was not run: maximum skill nesting ({}) reached",
                skill_name, MAX_FORK_DEPTH
            ));
        }

        let task = if skill_args.is_empty() {
            "Follow the instructions above.".to_string()
        } else {
            format!("Follow the instructions above. Arguments: {}", skill_args)
        };

        let mut fork_messages = vec![
            json!({ "role": "system", "content": content }),
            json!({ "role": "user", "content": task }),
        ];

        self.run_agent_at_depth(&mut fork_messages, depth + 1)
            .await
            .map_err(|e| format!("skill '{}' failed: {}", skill_name, e))
    }
}

fn parse_tool_calls(message: &Value) -> Vec<ToolCall> {
    let Some(calls) = message["tool_calls"].as_array() else {
        return Vec::new();
    };

    calls
        .iter()
        .filter_map(
            |call| match serde_json::from_value::<ToolCall>(call.clone()) {
                Ok(tool_call) => Some(tool_call),
                Err(e) => {
                    eprintln!("[tool] skipping malformed tool call: {} ({})", e, call);
                    None
                }
            },
        )
        .collect()
}

/// The API sends arguments as a JSON string. An empty string means "no arguments".
fn parse_arguments(arguments: &str) -> Result<Value, String> {
    if arguments.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(arguments).map_err(|e| format!("arguments are not valid JSON: {}", e))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .ok_or_else(|| format!("missing required string argument '{}'", key))
}

fn read_tool(args: &Value) -> ToolResult {
    let file_path = required_str(args, "file_path")?;
    let path = Path::new(file_path);

    // Reading a skill folder means reading its SKILL.md.
    let path = if path.is_dir() {
        path.join("SKILL.md")
    } else {
        path.to_path_buf()
    };

    fs::read_to_string(&path).map_err(|e| format!("could not read {}: {}", path.display(), e))
}

fn write_tool(args: &Value) -> ToolResult {
    let file_path = required_str(args, "file_path")?;
    let content = required_str(args, "content")?;
    let path = Path::new(file_path);

    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {}", parent.display(), e))?;
    }

    fs::write(path, content).map_err(|e| format!("could not write {}: {}", file_path, e))?;
    Ok(format!("Wrote {} bytes to {}", content.len(), file_path))
}

fn bash_tool(args: &Value) -> ToolResult {
    let command = required_str(args, "command")?;

    // std::process blocks this thread while the command runs. That is fine while
    // tools run one at a time; switch to tokio::process for concurrent tools.
    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|e| format!("could not start bash: {}", e))?;

    let mut result = String::from_utf8_lossy(&output.stdout).into_owned();

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        result.push_str("\n[stderr]\n");
        result.push_str(&stderr);
    }

    if !output.status.success() {
        let code = output
            .status
            .code()
            .map_or_else(|| "killed by signal".to_string(), |c| c.to_string());
        result.push_str(&format!("\n[exit code: {}]", code));
    }

    Ok(result)
}
