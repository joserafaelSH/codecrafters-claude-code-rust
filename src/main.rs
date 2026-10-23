use async_openai::{Client, config::OpenAIConfig};
use clap::Parser;
use serde_json::{Value, json};
use std::{env, process};

use crate::tools::ToolCall;

mod skills;
mod tools;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short = 'p', long)]
    prompt: String,
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
    let skills = skills::SkillsMap::new(".claude/skills/");

    // constant
    let user_role = "user";
    let system_role = "system";
    // let tool_role = "tool";
    let prompt = args.prompt;

    let mut message: Vec<Value> = vec![];
    message.push(json!({
        "role": system_role,
        "content": skills.get_skills_resume()
    }));

    if prompt.starts_with("/") {
        let mut tokens = prompt.split_whitespace().peekable();
        let mut matched_skills: Vec<&skills::Skill> = Vec::new();

        while let Some(token) = tokens.peek() {
            let Some(name) = token.strip_prefix('/') else {
                break;
            };
            let Some(skill) = skills.map.get(name) else {
                break;
            };
            matched_skills.push(skill);
            tokens.next();
        }
        let args: Vec<&str> = tokens.collect();
        let full_args = args.join(" ");

        for skill in matched_skills {
            if let Some(mut content) = skill.get_skill_body() {
                for (i, arg) in args.iter().enumerate().rev() {
                    content = content.replace(&format!("$ARGUMENTS[{}]", i), arg);
                    content = content.replace(&format!("${}", i), arg);
                }
                content = content.replace("$ARGUMENTS", &full_args);
                //constant?
                message.push(json!({
                    "role": "user",
                    "content": format!("Skill: {} (located at .claude/skills/{})\nPaths in the instructions below are relative to that folder.\n\n{}", skill.name, skill.name, content)
                }));
            }
        }
    } else {
        message.push(json!({
            "role": user_role,
            "content": prompt
        }));
    }

    let tools = tools::Tools::new(skills);
    let json_tools = &tools.tools;
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
        read_response(response.clone(), &tools, &mut message).await;
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

async fn read_response(response: Value, tools: &tools::Tools, message_history: &mut Vec<Value>) {
    let message = &response["choices"][0]["message"];

    if let Some(tool_calls) = message["tool_calls"].as_array() {
        let tool_calls: Vec<tools::ToolCall> = tool_calls
            .iter()
            .filter_map(|call| serde_json::from_value::<ToolCall>(call.clone()).ok())
            .collect();

        for tool_call in tool_calls {
            tools.execute_tool_call(tool_call, message_history).await;
        }
    }
}
