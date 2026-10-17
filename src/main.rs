use async_openai::{Client, config::OpenAIConfig};
use clap::Parser;
use serde_json::{Value, from_str, json};
use std::{env, fs, process};

#[derive(Parser)]
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
    let role = "user";
    let prompt = args.prompt;
    let mut message: Vec<Value> = vec![json!({
        "role": role,
        "content": prompt
    })];
    loop {
        println!("{:?}", message);
        #[allow(unused_variables)]
        let response: Value = client
            .chat()
            .create_byot(json!({
                "messages": message,
                "model": "anthropic/claude-haiku-4.5",
                "tools": [
                    {
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
                    }
                ]
            }))
            .await?;

        // You can use print statements as follows for debugging, they'll be visible when running tests.
        eprintln!("Logs from your program will appear here!");

        //TODO: Uncomment the lines below to pass the first stage
        // if let Some(content) = response["choices"][0]["message"]["content"].as_str() {
        //     println!("{}", content);
        // }
        println!("Response: {:#?}", response);
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
#[allow(unused_variables)]
#[derive(Debug)]
struct ToolCallFunctionParametersPropertiesAux {
    type_aux: String,
    description_aux: String,
}
#[allow(unused_variables)]
#[derive(Debug)]
struct ToolCallFunctionParametersProperties {
    file_path: ToolCallFunctionParametersPropertiesAux,
    contents: ToolCallFunctionParametersPropertiesAux,
}
#[allow(unused_variables)]
#[derive(Debug)]
struct ToolCallFunctionParameters {
    properties: ToolCallFunctionParametersProperties,
}

#[allow(unused_variables)]
#[derive(Debug)]
struct ToolCallFunction {
    name: String,
    arguments: String,
    description: String,
    parameters: ToolCallFunctionParameters,
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
    let contents = fs::read_to_string(file_path).expect("Should have been able to read the file");
    return contents;
}

fn execute_write_tool(tool_call_function: ToolCallFunction) -> String {
    let data: Value = serde_json::from_str(&tool_call_function.arguments).unwrap();
    let file_path = data["parameters"]["properties"]["file_path"]["description_aux"]
        .as_str()
        .unwrap();
    let contents = data["parameters"]["properties"]["contents"]["description_aux"]
        .as_str()
        .unwrap();
    println!("Writing to file: {} in path {}", contents, file_path);
    fs::write(file_path, contents).expect("Should have been able to write to the file");
    return "File written successfully".to_string();
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
                let function_description = call["function"]["description"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let function_parameters_properties_file_path_type = call["function"]["parameters"]
                    ["properties"]["file_path"]["type"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let function_parameters_properties_file_path_description =
                    call["function"]["parameters"]["properties"]["file_path"]["description"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                let function_parameters_properties_contents_type =
                    call["function"]["parameters"]["properties"]["contents"]["type"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                let function_parameters_properties_contents_description =
                    call["function"]["parameters"]["properties"]["contents"]["description"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                Some(ToolCall {
                    id,
                    type_tool,
                    function: ToolCallFunction {
                        name: function_name,
                        arguments: function_args,
                        description: function_description,
                        parameters: ToolCallFunctionParameters {
                            properties: ToolCallFunctionParametersProperties {
                                file_path: ToolCallFunctionParametersPropertiesAux {
                                    type_aux: function_parameters_properties_file_path_type,
                                    description_aux:
                                        function_parameters_properties_file_path_description,
                                },
                                contents: ToolCallFunctionParametersPropertiesAux {
                                    type_aux: function_parameters_properties_contents_type,
                                    description_aux:
                                        function_parameters_properties_contents_description,
                                },
                            },
                        },
                    },
                })
            })
            .collect();
        println!("Tool calls: {:?}", tool_calls);
        tool_calls
            .into_iter()
            .for_each(|tool_call| execute_tool_call(tool_call, message_history));

        // find last content message and print it
    }
}

// Executing the Read Tool
// When you detect a tool_calls array in the response:

// Extract the tool call: Get the message from the first choice, then extract the first tool call from the tool_calls array.
// Parse the function name: Read the function name to determine which tool to execute.
// Parse the arguments: Parse the function arguments as a JSON string to get the parameters.
// Execute the tool: Your program must perform the action requested. For the Read tool, use your language's file system library to read the file at the requested file_path.
// Output the result: Print the tool execution result. For the Read tool, print the raw file contents directly to stdout (just the contents, with no labels or formatting).
