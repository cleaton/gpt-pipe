use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;
use std::sync::Mutex;

use deno_core::error::AnyError;
use deno_core::op;
use deno_core::Extension;
use deno_core::RuntimeOptions;
use deno_core::{FastString, JsRuntime};
use std::rc::Rc;

use deno_core::FsModuleLoader;
use peekmore::PeekMore;

use once_cell::sync::OnceCell;

use std::sync::mpsc::{self, Receiver};
use std::thread;

const CHUNK_SIZE: usize = 1000;

fn create_stdin_producer_and_sample() -> Vec<String> {
    let (tx, rx) = mpsc::sync_channel::<Vec<String>>(10); // 10 chunks in flight

    // Spawn the producer thread
    let _producer = thread::spawn(move || {
        let stdin = io::stdin();
        let stdin = stdin.lock();
        let mut lines_iter = stdin.lines().peekmore();
        let mut sample: Vec<String> = Vec::new();

        for a in lines_iter.peek_amount(20) {
            if let Some(Ok(line)) = a {
                sample.push(line.to_string())
            }
        }

        let _ = tx.send(sample);

        let mut chunk = Vec::with_capacity(CHUNK_SIZE);
        while let Some(line) = lines_iter.next() {
            if let Ok(line) = line {
                chunk.push(line);
                if chunk.len() == CHUNK_SIZE {
                    let r = tx.send(chunk);
                    chunk = Vec::with_capacity(CHUNK_SIZE);
                    if r.is_err() {
                        break; // Consumer is done, so we're done
                    }
                }
            }
        }
        // Send any remaining lines
        if !chunk.is_empty() {
            let _ = tx.send(chunk);
        }
    });
    let sample = rx.recv().unwrap();
    LINES_RX.set(Mutex::new(rx)).unwrap();
    return sample;
}

static LINES_RX: OnceCell<Mutex<Receiver<Vec<String>>>> = OnceCell::new();

// deno_core

#[op]
async fn op_read_stdin_next() -> Result<Option<Vec<String>>, AnyError> {
    let lines_rx = LINES_RX.get().unwrap().lock().unwrap().recv();
    Ok(match lines_rx {
        Ok(data) => Some(data),
        _ => None,
    })
}

async fn run_js(file_path: &str) -> Result<(), AnyError> {
    let path = std::env::current_dir().expect("no current directory");
    let main_module = deno_core::resolve_path(file_path, path.as_path())?;
    let runtime_extension = Extension::builder("gpt-pipe")
        .ops(vec![op_read_stdin_next::decl()])
        .build();

    let mut runtime = JsRuntime::new(RuntimeOptions {
        module_loader: Some(Rc::new(FsModuleLoader)),
        extensions: vec![runtime_extension],
        ..Default::default()
    });

    const RUNTIME_JAVASCRIPT_CORE: &str = include_str!("./runtime.js");
    runtime
        .execute_script("<anon>", FastString::from_static(RUNTIME_JAVASCRIPT_CORE))
        .unwrap();
    let mod_id = runtime.load_main_module(&main_module, None).await?;
    let result = runtime.mod_evaluate(mod_id);
    runtime.run_event_loop(false).await?;
    result.await?
}

// end deno_core

#[derive(Deserialize)]
struct Gpt3Response {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    content: String,
}

async fn call_gpt3(
    sample_lines_str: &str,
    prompt: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // Read the API key from the .token file
    let api_key = fs::read_to_string(".token")?.trim().to_string();

    const SYSTEM_PROMT: &str = include_str!("./system_promt.txt");
    let user_prompt_sample = format!("S:\n{}", sample_lines_str);
    let user_prompt_question = format!("Q:\n{}", prompt);
    let client = Client::new();
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "model": "gpt-3.5-turbo",
            "messages": [
                { "role": "system", "content": SYSTEM_PROMT },
                { "role": "user", "content": user_prompt_sample },
                { "role": "user", "content": user_prompt_question },
            ],
            "temperature": 0.2
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        eprintln!("HTTP Error: {}", response.status());
        if let Ok(body) = response.text().await {
            eprintln!("Body: {}", body);
        }
        return Err("HTTP request failed".into());
    }

    let gpt3_response = response.json::<Gpt3Response>().await?;

    // Extract the JavaScript code from the assistant's message
    let js_code = gpt3_response
        .choices
        .get(0)
        .ok_or("No response from GPT-3")?
        .message
        .content
        .trim()
        .to_string();

    // Remove enclosing triple backticks
    let js_code = js_code.strip_prefix("```").unwrap_or(&js_code);
    let js_code = js_code.strip_suffix("```").unwrap_or(js_code);

    Ok(js_code.to_string())
}

fn sanitize_filename(filename: &str) -> String {
    let bad_chars = ['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'];
    let mut safe_filename = String::new();

    for c in filename.chars() {
        if bad_chars.contains(&c) {
            safe_filename.push('_');
        } else {
            safe_filename.push(c);
        }
    }

    safe_filename
}

async fn load_script(sample_lines: &str, query: &str) -> String {
    let sanitized = sanitize_filename(query);
    let file = format!("./scripts/{}.js", sanitized);
    if !Path::new("./scripts").exists() {
        fs::create_dir_all("./scripts").unwrap();
    }
    let path = Path::new(&file);
    if !path.exists() {
        let javascript_code = call_gpt3(&sample_lines, &query)
            .await
            .expect("Failed to call GPT-3");
        fs::write(path, &javascript_code).unwrap();
    }
    file
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: gpt-pipe \"<prompt>\"");
        std::process::exit(1);
    }

    let prompt = &args[1];
    let sample_lines = create_stdin_producer_and_sample();
    let sample_lines_str = sample_lines.join("\n");
    let script_file = load_script(&sample_lines_str, &prompt).await;

    if let Err(error) = run_js(script_file.as_str()).await {
        eprintln!("error: {error}");
    }
}
