use async_openai::{Client, config::OpenAIConfig};
use clap::Parser;
use serde_json::{Value, json};
use std::{env, process};

mod skills;
mod tools;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short = 'p', long)]
    prompt: String,
}

#[tokio::main]
async fn main() -> Result<(), tools::BoxError> {
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
    let prompt = args.prompt;

    let mut messages: Vec<Value> = vec![json!({
        "role": "system",
        "content": skills.get_skills_resume()
    })];

    if prompt.starts_with('/') {
        let mut tokens = prompt.split_whitespace().peekable();
        let mut matched_skills: Vec<&skills::Skill> = Vec::new();

        // Leading "/name" tokens that match a known skill; the rest are arguments.
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
        let skill_args = tokens.collect::<Vec<&str>>().join(" ");

        for skill in &matched_skills {
            if let Some(content) = skill.expand(&skill_args) {
                messages.push(json!({
                    "role": "user",
                    "content": format!(
                        "Skill: {name} (located at .claude/skills/{name})\nPaths in the instructions below are relative to that folder.\n\n{content}",
                        name = skill.name
                    )
                }));
            }
        }

        // No known skill matched: send the prompt as a normal message.
        if matched_skills.is_empty() {
            messages.push(json!({ "role": "user", "content": prompt }));
        }
    } else {
        messages.push(json!({ "role": "user", "content": prompt }));
    }

    let tools = tools::Tools::new(client, skills);
    let answer = tools.run_agent(&mut messages).await?;
    println!("{}", answer);

    Ok(())
}
