use std::{env, fs, path};

use async_openai::{Client, config::OpenAIConfig};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{skills, tools};

#[allow(unused_variables)]
#[derive(Debug, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
    pub id: String,
}

#[allow(unused_variables)]
#[derive(Debug, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub function: ToolCallFunction,
}

pub struct Tools {
    pub skills_map: skills::SkillsMap,
    pub tools: Vec<Value>,
}

impl Tools {
    pub fn new(skills_map: skills::SkillsMap) -> Self {
        let mut tools: Vec<Value> = vec![];
        let tools_path = path::Path::new("./tools ");

        if let Ok(dir_entries) = fs::read_dir(tools_path) {
            for dir_entry in dir_entries {
                if let Ok(entry) = dir_entry {
                    let path = entry.path();
                    if path.is_file() {
                        if path.ends_with(".json") {
                            if let Ok(contents) = std::fs::read_to_string(&path) {
                                if let Ok(json) = serde_json::from_str::<Value>(&contents) {
                                    tools.push(json);
                                }
                            }
                        }
                    }
                }
            }
        }
        Self { skills_map, tools }
    }

    pub async fn execute_tool_call(&self, tool_call: ToolCall, message_history: &mut Vec<Value>) {
        println!("CHEGUEI AQUI {:#?}", tool_call);
        let tool_role = "tool";
        match tool_call.function.name.as_str() {
            "Read" => {
                let result = self.execute_read_tool(tool_call.function);
                let response = json!({
                    "role": tool_role,
                    "content": result,
                    "tool_call_id": tool_call.id
                });
                message_history.push(response);
            }
            "Write" => {
                let result = self.execute_write_tool(tool_call.function);
                let response = json!({
                    "role": tool_role,
                    "content": result,
                    "tool_call_id": tool_call.id
                });
                message_history.push(response);
            }
            "Bash" => {
                let result = self.execute_bash_tool(tool_call.function);
                let response = json!({
                    "role": tool_role,
                    "content": result,
                    "tool_call_id": tool_call.id
                });
                message_history.push(response);
            }
            "Skill" => match Box::pin(self.execute_skill_tool(tool_call.function)).await {
                Ok(Some(response)) => message_history.push(response),
                Ok(None) => println!("Done, no output"),
                Err(e) => eprintln!("Error: {}", e),
            },
            _ => eprintln!("Unknown tool call: {}", tool_call.function.name),
        }
    }

    fn execute_read_tool(&self, tool_call_function: ToolCallFunction) -> String {
        let data: Value = serde_json::from_str(&tool_call_function.arguments).unwrap();
        let file_path = data["file_path"].as_str().unwrap();
        if file_path.ends_with("/") {
            let aux_path = format!("{}/SKILL.md", file_path);
            let contents = match fs::read_to_string(file_path) {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("Could not read {}: {}", aux_path, e);
                    String::new()
                }
            };
            return contents;
        }
        let contents = match fs::read_to_string(file_path) {
            Ok(text) => text,
            Err(e) => {
                eprintln!("Could not read {}: {}", file_path, e);
                String::new()
            }
        };
        return contents;
    }

    fn execute_write_tool(&self, tool_call_function: ToolCallFunction) -> String {
        let data: Value = serde_json::from_str(&tool_call_function.arguments).unwrap();
        let file_path = data["file_path"].as_str().unwrap();
        let contents = data["content"].as_str().unwrap();
        fs::write(file_path, contents).expect("Should have been able to write to the file");
        return "".to_string();
    }

    fn execute_bash_tool(&self, tool_call_function: ToolCallFunction) -> String {
        let data: Value = serde_json::from_str(&tool_call_function.arguments).unwrap();
        let command = data["command"].as_str().unwrap();
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(command)
            .output()
            .expect("Should have been able to execute the command");
        return String::from_utf8_lossy(&output.stdout).into_owned();
    }

    async fn execute_skill_tool(
        &self,
        tool_call_function: ToolCallFunction,
    ) -> Result<Option<Value>, Box<dyn std::error::Error>> {
        let data: Value = serde_json::from_str(&tool_call_function.arguments).unwrap();
        let skill_name = data["name"].as_str().unwrap();
        let args = data["args"].as_str().unwrap_or_default();
        let tokens = args.split_whitespace().peekable();
        let args: Vec<&str> = tokens.collect();
        let full_args = args.join(" ");
        if let Some(skill) = self.skills_map.map.get(skill_name) {
            if skill.context == "fork" {
                println!("TEM FORK CARAI {:?}", skill);
                if let Some(mut content) = skill.get_skill_body() {
                    let mut message: Vec<Value> = vec![];
                    message.push(json!({
                        "role": "system",
                        "content": content
                    }));
                    let base_url = env::var("OPENROUTER_BASE_URL")
                        .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());

                    let api_key = env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| {
                        eprintln!("OPENROUTER_API_KEY is not set");
                        panic!()
                    });

                    let config = OpenAIConfig::new()
                        .with_api_base(base_url)
                        .with_api_key(api_key);

                    let json_tools = &self.tools;
                    let client = Client::with_config(config);
                    loop {
                        #[allow(unused_variables)]
                        let response: Value = client
                            .chat()
                            .create_byot(json!({
                              "messages": message,
                              "model": "anthropic/claude-haiku-4.5",
                              "tools": json_tools
                            }))
                            .await?;

                        // You can use print statements as follows for debugging, they'll be visible when running tests.
                        eprintln!("Logs from your program will appear here!");

                        message.push(response["choices"][0]["message"].clone());
                        self.read_response(response.clone(), &mut message).await;
                        let finish_reason = response["choices"][0]["finish_reason"]
                            .as_str()
                            .unwrap_or("");
                        if finish_reason == "stop" {
                            let message = message.last().unwrap()["content"].as_str().unwrap_or("");
                            let id = &tool_call_function.id;
                            return Ok(Some(json!({
                                "role": "tool",
                                "content": message,
                                "tool_call_id": id
                            })));
                        }
                    }
                    return Ok(None);
                }
            } else if let Some(mut content) = skill.get_skill_body() {
                println!("NAO TEM FORK CARAI {:?}", skill);
                for (i, arg) in args.iter().enumerate().rev() {
                    content = content.replace(&format!("$ARGUMENTS[{}]", i), arg);
                    content = content.replace(&format!("${}", i), arg);
                }
                content = content.replace("$ARGUMENTS", &full_args);
                let id = &tool_call_function.id;
                let response = json!({
                    "role": "tool",
                    "content": content,
                    "tool_call_id": id
                });
                return Ok(Some(response));
            }
        }
        Ok(None)
        // println!("skill {:#?}", skill);
    }

    async fn read_response(&self, response: Value, message_history: &mut Vec<Value>) {
        let message = &response["choices"][0]["message"];

        if let Some(tool_calls) = message["tool_calls"].as_array() {
            let tool_calls: Vec<tools::ToolCall> = tool_calls
                .iter()
                .filter_map(|call| serde_json::from_value::<ToolCall>(call.clone()).ok())
                .collect();

            for tool_call in tool_calls {
                self.execute_tool_call(tool_call, message_history).await;
            }
        }
    }
}
