use async_openai::{Client, config::OpenAIConfig};
use clap::Parser;
use serde_json::{Value, json};
use std::path;
use std::{env, fs, process};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short = 'p', long)]
    prompt: String,
}
#[derive(Debug)]
struct Skill {
    name: String,
    description: String,
    path: String,
}

fn get_file_content_as_string(file_path: &str) -> Option<String> {
    match fs::read_to_string(file_path) {
        Ok(content) => Some(content),
        Err(_) => None,
    }
}

fn strip_frontmatter(raw: &str) -> &str {
    if let Some(rest) = raw.strip_prefix("---") {
        if let Some((_, body)) = rest.split_once("---") {
            return body.trim();
        }
    }
    raw.trim()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let base_url = env::var("OPENROUTER_BASE_URL")
        .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());

    let api_key = env::var("OPENROUTER_API_KEY").unwrap_or_else(|_| {
        eprintln!("OPENROUTER_API_KEY is not set");
        process::exit(1);
    });

    let config = OpenAIConfig::new()
        .with_api_base(base_url)
        .with_api_key(api_key);

    let client = Client::with_config(config);
    let mut skills: Vec<Skill> = Vec::new();
    find_skills(&mut skills);
    // println!("{:#?}", skills);
    let role = "user";
    let prompt = args.prompt;
    let mut message: Vec<Value> = vec![];
    message.push(json!({
        "role": "system",
        "content": parse_skills_to_system_message(&skills)
    }));

    if prompt.starts_with("/") {
        let mut tokens = prompt.split_whitespace().peekable();
        let mut matched_skills: Vec<&Skill> = Vec::new();

        while let Some(token) = tokens.peek() {
            let Some(name) = token.strip_prefix('/') else {
                break;
            };
            let Some(skill) = skills.iter().find(|s| s.name == name) else {
                break;
            };
            matched_skills.push(skill);
            tokens.next();
        }
        let args: Vec<&str> = tokens.collect();
        let full_args = args.join(" ");
        // println!("full args {}", full_args);
        for skill in matched_skills {
            // println!("skill {:#?}", skill);
            let raw = get_file_content_as_string(&skill.path).unwrap_or_default();
            let mut content = strip_frontmatter(&raw).to_string();
            for (i, arg) in args.iter().enumerate().rev() {
                content = content.replace(&format!("$ARGUMENTS[{}]", i), arg);
                content = content.replace(&format!("${}", i), arg);
            }
            content = content.replace("$ARGUMENTS", &full_args);
            message.push(json!({
                "role": "user",
                "content": format!("Skill: {} (located at .claude/skills/{})\nPaths in the instructions below are relative to that folder.\n\n{}", skill.name, skill.name, content)
            }));
        }
    } else {
        message.push(json!({
            "role": role,
            "content": prompt
        }));
    }

    let mut tools = vec![
        json!({
            "type": "function",
            "function": {
                "name": "Read",
                "description": "Read and return the contents of a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The path to the file to read"
                        }
                    },
                    "required": ["file_path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "Write",
                "description": "Write content to a file",
                "parameters": {
                    "type": "object",
                    "required": ["file_path", "content"],
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The path of the file to write to"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write to the file"
                        }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "Bash",
                "description": "Execute a shell command",
                "parameters": {
                    "type": "object",
                    "required": ["command"],
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command to execute"
                        }
                    }
                }
            }
        }),
        json!({
          "type": "function",
          "function": {
            "name": "Skill",
            "description": "Load a skill's instructions into the conversation",
            "parameters": {
              "type": "object",
              "required": ["name"],
              "properties": {
                "name": { "type": "string", "description": "The name of the skill to use" },
                "args": { "type": "string", "description": "Optional arguments for the skill" }
              }
            }
          }
        }),
    ];

    loop {
        //println!("{:#?}", message);
        #[allow(unused_variables)]
        let response: Value = client
            .chat()
            .create_byot(json!({
              "messages": message,
              "model": "anthropic/claude-haiku-4.5",
              "tools": tools
            }))
            .await?;

        // println!("{:#?}", response);
        // You can use print statements as follows for debugging, they'll be visible when running tests.
        eprintln!("Logs from your program will appear here!");

        //TODO: Uncomment the lines below to pass the first stage
        // if let Some(content) = response["choices"][0]["message"]["content"].as_str() {
        //     println!("{}", content);
        // }
        message.push(response["choices"][0]["message"].clone());
        read_response(response.clone(), &mut message);
        let finish_reason = response["choices"][0]["finish_reason"]
            .as_str()
            .unwrap_or("");
        if finish_reason == "stop" {
            println!(
                "{}",
                message.last().unwrap()["content"].as_str().unwrap_or("")
            );
            break;
        }
    }

    Ok(())
}

fn parse_skills_to_system_message(skills: &Vec<Skill>) -> String {
    let mut skill_message = String::new();
    skill_message.push_str("skills:\n\n");

    for skill in skills {
        skill_message.push_str(&format!(
            "- name: {}\n  description: {}.\n\n",
            skill.name, skill.description
        ));
    }

    skill_message
}

fn get_skill_name_description(skill_file_path: &path::Path) -> Option<(String, String)> {
    if let Ok(contents) = fs::read_to_string(skill_file_path) {
        let mut lines = contents.lines();
        if let Some(first_line) = lines.next() {
            if first_line.trim() == "---" {
                let mut name = String::new();
                let mut description = String::new();
                for line in lines {
                    if line.trim() == "---" {
                        break;
                    }
                    if line.starts_with("name:") {
                        name = line["name:".len()..].trim().to_string();
                    } else if line.starts_with("description:") {
                        description = line["description:".len()..].trim().to_string();
                    }
                }
                return Some((name, description));
            }
        }
    }
    None
}

fn find_skills(skills: &mut Vec<Skill>) {
    let skills_path = path::Path::new(".claude/skills/");
    // the partern
    //     .claude/
    // └── skills/
    //     ├── apple/
    //     │   └── SKILL.md
    //     └── grape/
    //         └── SKILL.md

    // for each folder in skills_path, read the SKILL.md file and print the contents
    // list all paths in skills_path
    if let Ok(entries) = fs::read_dir(skills_path) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_dir() {
                    let skill_file_path = path.join("SKILL.md");
                    if let Some((name, description)) = get_skill_name_description(&skill_file_path)
                    {
                        skills.push(Skill {
                            name,
                            description,
                            path: skill_file_path.to_string_lossy().to_string(),
                        });
                    }
                }
            }
        }
    }
}
#[allow(unused_variables)]
#[derive(Debug)]
struct ToolCallFunction {
    name: String,
    arguments: String,
}

#[allow(unused_variables)]
#[derive(Debug)]
struct ToolCall {
    id: String,
    type_tool: String,
    function: ToolCallFunction,
}
fn execute_read_tool(tool_call_function: ToolCallFunction) -> String {
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

fn execute_write_tool(tool_call_function: ToolCallFunction) -> String {
    let data: Value = serde_json::from_str(&tool_call_function.arguments).unwrap();
    let file_path = data["file_path"].as_str().unwrap();
    let contents = data["content"].as_str().unwrap();
    fs::write(file_path, contents).expect("Should have been able to write to the file");
    return "".to_string();
}

fn execute_bash_tool(tool_call_function: ToolCallFunction) -> String {
    let data: Value = serde_json::from_str(&tool_call_function.arguments).unwrap();
    let command = data["command"].as_str().unwrap();
    let output = std::process::Command::new("bash")
        .arg("-c")
        .arg(command)
        .output()
        .expect("Should have been able to execute the command");
    return String::from_utf8_lossy(&output.stdout).into_owned();
}

fn execute_skill_tool(tool_call_function: ToolCallFunction) -> String {
    let data: Value = serde_json::from_str(&tool_call_function.arguments).unwrap();
    let skill_name = data["name"].as_str().unwrap();
    let args = data["args"].as_str().unwrap_or_default();
    let tokens = args.split_whitespace().peekable();
    // let mut matched_skills: Vec<&Skill> = Vec::new();

    // while let Some(token) = tokens.peek() {
    //     let Some(name) = token.strip_prefix('/') else {
    //         break;
    //     };
    //     let Some(skill) = skills.iter().find(|s| s.name == name) else {
    //         break;
    //     };
    //     matched_skills.push(skill);
    //     tokens.next();
    // }
    let args: Vec<&str> = tokens.collect();
    let full_args = args.join(" ");

    // println!("skill {:#?}", skill);
    let raw = fs::read_to_string(format!(".claude/skills/{}/SKILL.md", skill_name)).unwrap();
    let mut content = strip_frontmatter(&raw).to_string();
    for (i, arg) in args.iter().enumerate().rev() {
        content = content.replace(&format!("$ARGUMENTS[{}]", i), arg);
        content = content.replace(&format!("${}", i), arg);
    }
    content = content.replace("$ARGUMENTS", &full_args);

    content
}

fn execute_tool_call(tool_call: ToolCall, message_history: &mut Vec<Value>) {
    match tool_call.function.name.as_str() {
        "Read" => {
            let result = execute_read_tool(tool_call.function);
            let response = json!({
                "role": "tool",
                "content": result,
                "tool_call_id": tool_call.id
            });
            message_history.push(response);
        }
        "Write" => {
            let result = execute_write_tool(tool_call.function);
            let response = json!({
                "role": "tool",
                "content": result,
                "tool_call_id": tool_call.id
            });
            message_history.push(response);
        }
        "Bash" => {
            let result = execute_bash_tool(tool_call.function);
            let response = json!({
                "role": "tool",
                "content": result,
                "tool_call_id": tool_call.id
            });
            message_history.push(response);
        }
        "Skill" => {
            let result = execute_skill_tool(tool_call.function);
            let response = json!({
                "role": "tool",
                "content": result,
                "tool_call_id": tool_call.id
            });
            message_history.push(response);
        }
        _ => eprintln!("Unknown tool call: {}", tool_call.function.name),
    }
}

fn read_response(response: Value, message_history: &mut Vec<Value>) {
    // get the message form the first choice
    let message = &response["choices"][0]["message"];

    if let Some(tool_calls) = message["tool_calls"].as_array() {
        let tool_calls: Vec<ToolCall> = tool_calls
            .iter()
            .filter_map(|call| {
                let id = call["id"].as_str().unwrap_or("").to_string();
                let type_tool = call["type"].as_str().unwrap_or("").to_string();
                let function_name = call["function"]["name"].as_str().unwrap_or("").to_string();
                let function_args = call["function"]["arguments"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                Some(ToolCall {
                    id,
                    type_tool,
                    function: ToolCallFunction {
                        name: function_name,
                        arguments: function_args,
                    },
                })
            })
            .collect();
        tool_calls
            .into_iter()
            .for_each(|tool_call| execute_tool_call(tool_call, message_history));

        // find last content message and print it
    }
}
