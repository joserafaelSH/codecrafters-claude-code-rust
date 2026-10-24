[![progress-banner](https://backend.codecrafters.io/progress/claude-code/32c9d44e-30d2-47a2-88e1-62fd4cf18caf)](https://app.codecrafters.io/users/joserafaelSH?r=2qF)

# Building a tiny Claude Code in Rust

This is my solution to the CodeCrafters challenge ["Build Your own Claude Code"](https://codecrafters.io/challenges/claude-code): a small AI coding assistant that runs in the terminal. You give it a prompt, it talks to a language model, and the model can read files, write files, run shell commands, and load "skills" until it has an answer.

I also used this project to learn Rust. So besides explaining how the agent works, this README walks through the Rust features I ran into along the way, with pointers to where each one shows up in the code. If you're learning Rust too, I hope it helps.

## What it does

```bash
./your_program.sh -p "Create a hello.txt file with a greeting, then show me its contents"
```

Behind the scenes, the program:

1. Sends your prompt to the model, along with a list of tools it can use.
2. If the model asks to use a tool (for example, `Write` with a file path and content), the program runs it and sends the result back.
3. Repeats until the model replies with plain text instead of a tool call.
4. Prints that final answer.

That loop is the whole idea behind coding agents. Everything else is details.

```
           ┌──────────────────────────────────────────┐
           │                                          │
 prompt ──►│  model ──► tool calls? ──yes──► run tools ┘
           │              │
           │              no
           │              ▼
           └──────► print the answer
```

## Running it

You need Rust (see `rust-version` in `Cargo.toml`) and an [OpenRouter](https://openrouter.ai) API key.

```bash
export OPENROUTER_API_KEY="your-key"
# optional, defaults to https://openrouter.ai/api/v1
export OPENROUTER_BASE_URL="https://openrouter.ai/api/v1"

./your_program.sh -p "What files are in this folder?"
```

The final answer goes to **stdout**. Everything else (which tools ran, errors) goes to **stderr**, so you can pipe the answer somewhere without the noise:

```bash
./your_program.sh -p "Summarize README.md" 2>/dev/null
```

> ⚠️ The `Bash` and `Write` tools do exactly what the model asks, with your permissions. Run it in a folder you don't mind it touching.

## Tools

The model can call four tools. Their definitions (the JSON schemas the model sees) live in `src/tools/*.json`.

| Tool    | Arguments                  | What it does |
|---------|----------------------------|--------------|
| `Read`  | `file_path`                | Returns a file's contents. Given a folder, reads its `SKILL.md`. |
| `Write` | `file_path`, `content`     | Writes a file, creating parent folders if needed. |
| `Bash`  | `command`                  | Runs a shell command and returns stdout, stderr, and the exit code. |
| `Skill` | `name`, `args` (optional)  | Loads a skill (see below). |

If a tool fails, for example a missing file or bad arguments, the error is sent back to the model as text instead of crashing the program. The model can then read the error and try something else, which it's surprisingly good at.

## Skills

Skills are reusable instructions stored as markdown files:

```
.claude/skills/
└── review/
    └── SKILL.md
```

```markdown
---
name: review
description: Review a file and suggest improvements
context: fork
---
Read $0 and list the three most important improvements.
Focus on: $ARGUMENTS
```

The part between the `---` lines is metadata. The model sees every skill's name and description in its system prompt, so it knows what's available.

You can run a skill yourself by starting the prompt with a slash:

```bash
./your_program.sh -p "/review src/main.rs readability"
```

Placeholders are filled in from the arguments:

| Placeholder        | Becomes (for `src/main.rs readability`) |
|--------------------|------------------------------------------|
| `$ARGUMENTS`       | `src/main.rs readability`               |
| `$0`, `$1`, ...    | `src/main.rs`, `readability`, ...        |
| `$ARGUMENTS[0]`    | `src/main.rs`                            |

Leading `/names` that match known skills are all loaded, and parsing stops at the first word that isn't one:

| Prompt                   | Skills loaded   | Arguments       |
|--------------------------|-----------------|-----------------|
| `/apple /grape 4127`     | apple, grape    | `4127`          |
| `/apple 4127 /grape`     | apple           | `4127 /grape`   |
| `/apple /pear 4127`      | apple (no pear) | `/pear 4127`    |

The model can also load a skill on its own with the `Skill` tool. With `context: fork`, the skill runs as a **separate agent** with its own conversation, and only its final answer comes back. That keeps the main conversation short. Forks can call other skills, up to 3 levels deep, so a skill can't call itself forever.

## Project structure

```
src/
├── main.rs       # reads the prompt, expands /skills, starts the agent
├── tools.rs      # the agent loop and the four tools
├── skills.rs     # finds and parses skills in .claude/skills/
└── tools/        # JSON definitions sent to the model
    ├── read_tool.json
    ├── write_tool.json
    ├── bash_tool.json
    └── skill_tool.json
```

---

## The Rust I learned building this

This section is a tour of the Rust features this project uses, roughly in the order you'd meet them. Each one has a short explanation and a real example from the code.

### Modules: `mod` and `pub`

Rust doesn't treat a file as a module just because it exists. `main.rs` has to declare it:

```rust
// main.rs
mod skills;
mod tools;
```

Everything is private by default. `pub` makes something visible outside its module, and struct fields need their own `pub`. In `tools.rs`, `Tools` is public but its fields (`client`, `skills_map`, `definitions`) aren't, so other code has to go through its methods.

### Structs and `impl`

A `struct` holds data, and an `impl` block gives it behavior:

```rust
pub struct Tools {
    client: Client<OpenAIConfig>,
    skills_map: skills::SkillsMap,
    definitions: Vec<Value>,
}

impl Tools {
    pub fn new(client: Client<OpenAIConfig>, skills_map: skills::SkillsMap) -> Self { ... }
    pub async fn run_agent(&self, messages: &mut Vec<Value>) -> Result<String, BoxError> { ... }
}
```

`new` has no `self`, so it's called as `Tools::new(...)`. Methods take `&self` to read the struct or `&mut self` to change it. `Self` is shorthand for "this type".

### Ownership and borrowing

This was the hardest part to get used to, and it shows up everywhere.

- **`String` vs `&str`.** A `String` owns its text, and a `&str` borrows it. Functions that only read text take `&str` (`required_str(args, "file_path")`), and functions that build new text return `String`.
- **Moving vs borrowing.** `Tools::new(client, skills)` *moves* `skills` into the struct, so `main` can't use it afterward. Passing `&mut messages` to `run_agent` only *borrows* it, so `main` still owns the history.
- **Cloning when you need two owners.** When inserting a skill into a `HashMap`, the name is needed both as the key and inside the struct:

  ```rust
  skills_map.insert(skill.name.clone(), skill);
  ```

  Without `.clone()`, the first argument moves `skill.name` out of `skill`, and the second argument can't move the half-empty struct. The compiler calls this a "partial move".

- **You can't return a reference to something you created.** `get_skill_body` reads a file into a local `String`, so it returns an owned `String`, not a `&str`. A `&str` would point into memory that's freed when the function ends.

### Lifetimes

Most of the time Rust figures out lifetimes on its own. When a function takes several references and returns one, you sometimes have to say which input the output borrows from:

```rust
fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args[key]
        .as_str()
        .ok_or_else(|| format!("missing required string argument '{}'", key))
}
```

`'a` says the returned `&str` lives inside `args`, not inside `key`. So the caller knows the result is valid as long as `args` is.

### `Option` and `Result` instead of null and exceptions

Rust has no `null` and no exceptions. A value that might be missing is an `Option<T>` (`Some(value)` or `None`), and an operation that might fail returns a `Result<T, E>` (`Ok(value)` or `Err(error)`). The compiler won't let you use the value until you've handled both cases.

`get_skill_metadata` returns `Option<Skill>` instead of a `Skill` with empty strings. That way a broken skill can't be mistaken for a real one.

Every tool returns `Result<String, String>`:

```rust
type ToolResult = Result<String, String>;
```

The agent then turns errors into text for the model:

```rust
result.unwrap_or_else(|e| format!("Error: {}", e))
```

Some conversions I used a lot:

| Method               | Turns                     | Into              |
|----------------------|---------------------------|-------------------|
| `.ok()`              | `Result<T, E>`            | `Option<T>`       |
| `.ok_or_else(...)`   | `Option<T>`               | `Result<T, E>`    |
| `.map_err(...)`      | `Result<T, E1>`           | `Result<T, E2>`   |
| `.unwrap_or_default()` | `Option<T>` / `Result<T, E>` | `T` (or its default) |

### The `?` operator

`?` means "if this failed, return the error now; otherwise give me the value". It keeps the happy path flat:

```rust
fn write_tool(args: &Value) -> ToolResult {
    let file_path = required_str(args, "file_path")?;
    let content = required_str(args, "content")?;
    // ...
    fs::write(path, content).map_err(|e| format!("could not write {}: {}", file_path, e))?;
    Ok(format!("Wrote {} bytes to {}", content.len(), file_path))
}
```

It works on `Option` too, returning `None` early.

### `unwrap` and `expect`: only when failure is a bug

`unwrap()` and `expect()` crash the program on failure. They're used in exactly one place:

```rust
serde_json::from_str(text).expect("tool definition is not valid JSON")
```

Those JSON files are part of the program itself. If one is broken, that's my bug, and failing loudly at startup is the right call. Anything coming from the model or the file system uses `Result` instead.

### Pattern matching: `match`, `if let`, `let ... else`

`match` dispatches each tool by name, and must cover every case (the `other` arm catches the rest):

```rust
match name {
    "Read" => read_tool(&args),
    "Write" => write_tool(&args),
    "Bash" => bash_tool(&args),
    "Skill" => Box::pin(self.skill_tool(&args, depth)).await,
    other => Err(format!("unknown tool '{}'", other)),
}
```

`let ... else` keeps going only if a pattern matches, and otherwise leaves the block. It's perfect for parsing the `/skill` prefixes:

```rust
while let Some(token) = tokens.peek() {
    let Some(name) = token.strip_prefix('/') else { break };
    let Some(skill) = skills.map.get(name) else { break };
    matched_skills.push(skill);
    tokens.next();
}
```

### Iterators and closures

Rust iterators are lazy chains of small steps. Some I used:

- **`peekable()`** lets you look at the next item without consuming it. Above, the loop stops at the first non-skill word and leaves it for the arguments.
- **`filter_map`** transforms each item and drops the ones that become `None`. `parse_tool_calls` uses it to skip malformed tool calls (and log them).
- **`enumerate().rev()`** walks the arguments from last to first, so `$1` is replaced *after* `$10`, and `$10` doesn't turn into "first argument + 0".
- **`collect()`** turns an iterator into a collection. The target type decides what you get: `Vec<&str>`, `Vec<Value>`, a `String`...

Closures (`|e| format!(...)`) are small inline functions, used with methods like `map_err` and `unwrap_or_else`.

### `HashMap`

Skills are stored in a `HashMap<String, Skill>`, so looking one up is a single `skills.map.get(name)` instead of searching a list. `get` returns an `Option`, which fits nicely with `let ... else`. One detail: a `HashMap` has no order, so the "unknown skill" error sorts the names before listing them.

### Working with JSON: `serde` and `serde_json`

Rust's standard library has no JSON support, so the project uses the de facto standard crates.

For data with a known shape, `#[derive(Deserialize)]` generates the parsing code:

```rust
#[derive(Debug, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    #[serde(default)]
    pub arguments: String,
}
```

Fields without attributes are required. `#[serde(default)]` uses an empty value when the field is missing.

For data with a loose shape, `serde_json::Value` can be indexed like JavaScript. A missing key gives `Null` instead of a crash:

```rust
let message = response["choices"][0]["message"].clone();
```

The `json!` macro builds JSON with almost literal syntax:

```rust
json!({
    "role": "tool",
    "tool_call_id": tool_call.id,
    "content": content,
})
```

One API quirk worth knowing: tool `arguments` arrive as a JSON object *inside a string*, so they're parsed a second time with `serde_json::from_str`.

### Compile-time file embedding: `include_str!`

The tool definitions are compiled into the binary:

```rust
const TOOL_DEFINITIONS: [&str; 4] = [
    include_str!("tools/read_tool.json"),
    include_str!("tools/write_tool.json"),
    include_str!("tools/bash_tool.json"),
    include_str!("tools/skill_tool.json"),
];
```

Before this, the program read them from a folder at run time, which broke depending on where you ran it from. `include_str!` needs a literal path, because the compiler reads the file *before* the program exists. That's also why it can't take a `String` variable.

### Constants and type aliases

```rust
const MODEL: &str = "anthropic/claude-haiku-4.5";
const MAX_TURNS: usize = 50;
const MAX_FORK_DEPTH: usize = 3;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
```

`const` values are inlined at compile time. A `type` alias is just a shorter name. `BoxError` is long to write, and it's used in several signatures.

### Trait objects: `Box<dyn Error>`

`dyn Error` means "any type that implements the `Error` trait". Different libraries have different error types (network errors, JSON errors, I/O errors), and boxing them as `Box<dyn Error>` lets one function return any of them. `?` converts them automatically. Even a plain `String` converts with `.into()`:

```rust
return Err(format!("unexpected API response: {}", response).into());
```

The `+ Send + Sync` part means the error can be moved between threads, which async code often needs.

### Async and `await` with Tokio

Talking to the API is a network call, so it's `async`:

```rust
#[tokio::main]
async fn main() -> Result<(), tools::BoxError> {
    // ...
    let answer = tools.run_agent(&mut messages).await?;
    println!("{}", answer);
    Ok(())
}
```

A few things I learned the hard way:

- **Calling an `async fn` doesn't run it.** It returns a *future*, and nothing happens until you `.await` it.
- **`.await` doesn't work inside regular closures**, so `tool_calls.into_iter().for_each(|c| self.run(c))` silently does nothing. A plain `for` loop with `.await` inside is the fix.
- **`#[tokio::main]`** sets up the async runtime so `main` itself can be `async`.
- **Tools run one at a time on purpose.** Running them all at once would need several mutable borrows of the message history at the same time, which Rust doesn't allow, and the model often expects order anyway (write a file, *then* read it).

### Async recursion and `Box::pin`

A forked skill runs a whole new agent, so there's a cycle:

```
run_agent → execute_tool_call → skill_tool → run_agent → ...
```

An `async fn` compiles into a state machine that stores the futures it awaits *inside itself*. A cycle would make it infinitely large, so the compiler rejects it. Boxing one call in the cycle puts that future on the heap, and the outer future only stores a pointer:

```rust
"Skill" => Box::pin(self.skill_tool(&args, depth)).await,
```

### Running processes: `std::process::Command`

The `Bash` tool uses the standard library to run commands and capture their output:

```rust
let output = Command::new("bash")
    .arg("-c")
    .arg(command)
    .output()
    .map_err(|e| format!("could not start bash: {}", e))?;
```

`String::from_utf8_lossy` turns the raw bytes into text, replacing invalid UTF-8 instead of failing. This call blocks the thread while the command runs. That's fine while tools run one at a time, and `tokio::process::Command` is the async replacement if that ever changes.

### Command-line arguments with `clap`

The `-p` flag comes from a derive macro, the same idea as `serde`:

```rust
#[derive(Parser, Debug)]
struct Args {
    #[arg(short = 'p', long)]
    prompt: String,
}
```

`Args::parse()` reads the command line, prints help with `--help`, and shows an error if `-p` is missing.

### stdout vs stderr

Small but important: `println!` writes to stdout and `eprintln!` writes to stderr. The CodeCrafters tests read stdout, and early on my debug prints were breaking them. Now only the final answer uses `println!`.

---

## Things I'd like to try next

- Run independent tools concurrently with `futures::join_all`, using `tokio::process` and `tokio::fs`.
- Restrict `Read` and `Write` to the working folder, so the model can't touch files elsewhere.
- Stream the model's answer as it's generated instead of waiting for the whole reply.
- Write tests for the skill parser and the placeholder expansion.

## Credits

Built for the [CodeCrafters "Build Your own Claude Code" challenge](https://codecrafters.io/challenges/claude-code). The API calls use [`async-openai`](https://crates.io/crates/async-openai) against an OpenAI-compatible endpoint ([OpenRouter](https://openrouter.ai)).****
