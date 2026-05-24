use glob::Pattern;
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::task;
use walkdir::WalkDir;

#[derive(Debug, Error)]
enum CapabilityError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("tool not found: {0}")]
    ToolNotFound(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("regex error: {0}")]
    Regex(String),
    #[error("serde error: {0}")]
    Serde(String),
    #[error("bash execution failed: {0}")]
    Bash(String),
}

impl From<io::Error> for CapabilityError {
    fn from(value: io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl From<serde_json::Error> for CapabilityError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

#[derive(Clone)]
struct ServerState {
    workspace_root: PathBuf,
    task_dir: PathBuf,
    task_index: Arc<Mutex<HashMap<String, PathBuf>>>,
}

#[derive(Debug, Clone, Serialize)]
struct ToolOutcome {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    structured_content: Option<Value>,
    is_error: bool,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: Option<String>,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct GlobInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrepInput {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    output_mode: Option<String>,
    #[serde(default)]
    before: Option<usize>,
    #[serde(default)]
    after: Option<usize>,
    #[serde(default)]
    context: Option<usize>,
    #[serde(default)]
    line_number: Option<bool>,
    #[serde(default)]
    ignore_case: Option<bool>,
    #[serde(default)]
    file_type: Option<String>,
    #[serde(default)]
    head_limit: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    multiline: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReadInput {
    file_path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    pages: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EditInput {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WriteInput {
    file_path: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BashInput {
    command: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default)]
    run_in_background: Option<bool>,
    #[serde(default, rename = "dangerouslyDisableSandbox")]
    dangerously_disable_sandbox: Option<bool>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let workspace_root = arg_value(&args, "--workspace").unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).to_string_lossy().to_string()
    });

    let root = PathBuf::from(workspace_root);
    let task_dir = root.join(".agentkernel-capabilities").join("tasks");
    fs::create_dir_all(&task_dir)?;

    let state = Arc::new(ServerState {
        workspace_root: root,
        task_dir,
        task_index: Arc::new(Mutex::new(HashMap::new())),
    });

    serve_stdio(state).await?;

    Ok(())
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|a| a == key)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

async fn serve_stdio(state: Arc<ServerState>) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = stdout;

    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(response) = handle_rpc_line(&state, line).await {
            let response_text = serde_json::to_string(&response)?;
            writer.write_all(response_text.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
    }

    Ok(())
}

async fn handle_rpc_line(state: &Arc<ServerState>, line: &str) -> Option<JsonRpcResponse> {
    let request: Result<JsonRpcRequest, _> = serde_json::from_str(line);
    let req = match request {
        Ok(req) => req,
        Err(err) => {
            return Some(error_response(Some(Value::Null), -32700, "parse error", Some(json!({"detail": err.to_string()}))));
        }
    };

    if req.id.is_none() {
        let _ = dispatch_notification(state, req).await;
        return None;
    }

    let id = req.id.clone();
    let response = match dispatch_request(state, req).await {
        Ok(value) => jsonrpc_ok(id, value),
        Err(err) => error_to_response(id, err),
    };
    Some(response)
}

async fn dispatch_notification(_state: &Arc<ServerState>, req: JsonRpcRequest) -> Result<(), CapabilityError> {
    match req.method.as_str() {
        "notifications/initialized" | "$/cancelRequest" | "notifications/cancelled" => Ok(()),
        _ => Ok(()),
    }
}

async fn dispatch_request(state: &Arc<ServerState>, req: JsonRpcRequest) -> Result<Value, CapabilityError> {
    match req.method.as_str() {
        "initialize" => {
            let protocol_version = req
                .params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("2024-11-05");

            Ok(json!({
                "protocolVersion": protocol_version,
                "serverInfo": {
                    "name": "agentkernel-capabilities",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    },
                    "resources": {},
                    "prompts": {}
                },
                "instructions": "MCP server exposing local glob, grep, read, edit, write, and bash tools"
            }))
        }
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let params: ToolCallParams = serde_json::from_value(req.params)?;
            let name = params.name.ok_or_else(|| CapabilityError::InvalidRequest("tools/call params.name is required".into()))?;
            let outcome = call_tool(state, &name, params.arguments).await?;
            Ok(json!({
                "content": [{"type": "text", "text": outcome.text}],
                "isError": outcome.is_error,
                "structuredContent": outcome.structured_content
            }))
        }
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        "logging/setLevel" => Ok(json!({})),
        "ping" => Ok(json!({})),
        _ => Err(CapabilityError::ToolNotFound(req.method)),
    }
}

fn jsonrpc_ok(id: Option<Value>, result: Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

fn error_to_response(id: Option<Value>, err: CapabilityError) -> JsonRpcResponse {
    let (code, message, data) = match err {
        CapabilityError::InvalidRequest(msg) => (-32600, msg, None),
        CapabilityError::ToolNotFound(msg) => (-32601, msg, None),
        CapabilityError::Io(msg) => (-32000, msg, None),
        CapabilityError::Regex(msg) => (-32001, msg, None),
        CapabilityError::Serde(msg) => (-32002, msg, None),
        CapabilityError::Bash(msg) => (-32003, msg, None),
    };
    error_response(id, code, &message, data)
}

fn error_response(id: Option<Value>, code: i64, message: &str, data: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
            data,
        }),
    }
}

async fn call_tool(state: &Arc<ServerState>, name: &str, arguments: Value) -> Result<ToolOutcome, CapabilityError> {
    match name {
        "glob" => {
            let input: GlobInput = serde_json::from_value(arguments)?;
            glob_tool(state, input).await
        }
        "grep" => {
            let input: GrepInput = serde_json::from_value(arguments)?;
            grep_tool(state, input).await
        }
        "read" => {
            let input: ReadInput = serde_json::from_value(arguments)?;
            read_tool(state, input).await
        }
        "edit" => {
            let input: EditInput = serde_json::from_value(arguments)?;
            edit_tool(state, input).await
        }
        "write" => {
            let input: WriteInput = serde_json::from_value(arguments)?;
            write_tool(state, input).await
        }
        "bash" => {
            let input: BashInput = serde_json::from_value(arguments)?;
            bash_tool(state, input).await
        }
        other => Err(CapabilityError::ToolNotFound(other.to_string())),
    }
}

fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "glob".into(),
            description: "按文件路径模式查找文件".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "grep".into(),
            description: "按内容搜索文件".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string" },
                    "glob": { "type": "string" },
                    "outputMode": { "type": "string", "enum": ["content", "files_with_matches", "count"] },
                    "before": { "type": "integer", "minimum": 0 },
                    "after": { "type": "integer", "minimum": 0 },
                    "context": { "type": "integer", "minimum": 0 },
                    "lineNumber": { "type": "boolean" },
                    "ignoreCase": { "type": "boolean" },
                    "fileType": { "type": "string" },
                    "headLimit": { "type": "integer", "minimum": 0 },
                    "offset": { "type": "integer", "minimum": 0 },
                    "multiline": { "type": "boolean" }
                },
                "required": ["pattern"]
            }),
        },
        ToolDefinition {
            name: "read".into(),
            description: "读取文件内容，支持分页读取".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0 },
                    "limit": { "type": "integer", "minimum": 1 },
                    "pages": { "type": "string" }
                },
                "required": ["file_path"]
            }),
        },
        ToolDefinition {
            name: "edit".into(),
            description: "精确替换文件中的旧字符串".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean" }
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
        },
        ToolDefinition {
            name: "write".into(),
            description: "创建或整体重写文件".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["file_path", "content"]
            }),
        },
        ToolDefinition {
            name: "bash".into(),
            description: "执行 shell 命令，可同步或后台运行".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "description": { "type": "string" },
                    "timeout": { "type": "integer", "minimum": 1 },
                    "run_in_background": { "type": "boolean" },
                    "dangerouslyDisableSandbox": { "type": "boolean" }
                },
                "required": ["command"]
            }),
        },
    ]
}

async fn glob_tool(state: &Arc<ServerState>, input: GlobInput) -> Result<ToolOutcome, CapabilityError> {
    let base = input
        .path
        .map(PathBuf::from)
        .unwrap_or_else(|| state.workspace_root.clone());
    let pattern_str = base.join(&input.pattern).to_string_lossy().to_string();
    let mut filenames = Vec::new();
    for entry in glob::glob(&pattern_str).map_err(|e| CapabilityError::InvalidRequest(e.to_string()))? {
        match entry {
            Ok(path) => {
                let display = path
                    .strip_prefix(&state.workspace_root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                filenames.push(display);
                if filenames.len() >= 200 {
                    break;
                }
            }
            Err(err) => return Err(CapabilityError::Io(err.to_string())),
        }
    }
    let truncated = filenames.len() >= 200;
    Ok(ToolOutcome {
        text: serde_json::to_string_pretty(&json!({
            "num_files": filenames.len(),
            "truncated": truncated,
            "filenames": filenames,
        }))?,
        structured_content: Some(json!({
            "numFiles": filenames.len(),
            "truncated": truncated,
            "filenames": filenames,
        })),
        is_error: false,
    })
}

async fn grep_tool(state: &Arc<ServerState>, input: GrepInput) -> Result<ToolOutcome, CapabilityError> {
    let base = input
        .path
        .map(PathBuf::from)
        .unwrap_or_else(|| state.workspace_root.clone());
    let file_filter = input
        .glob
        .and_then(|g| Pattern::new(&g).ok());
    let type_ext = input.file_type.map(|ft| ft.trim_start_matches('.').to_string());
    let mut builder = RegexBuilder::new(&input.pattern);
    builder.case_insensitive(input.ignore_case.unwrap_or(false));
    builder.dot_matches_new_line(input.multiline.unwrap_or(false));
    let regex = builder.build().map_err(|e| CapabilityError::Regex(e.to_string()))?;

    let output_mode = input.output_mode.unwrap_or_else(|| "files_with_matches".to_string());
    let before = input.before.unwrap_or(0);
    let after = input.after.unwrap_or(0);
    let context = input.context.unwrap_or(0);
    let before = before.max(context);
    let after = after.max(context);
    let include_line = input.line_number.unwrap_or(true);
    let offset = input.offset.unwrap_or(0);
    let head_limit = input.head_limit.unwrap_or(250);

    let mut matches = Vec::new();
    let mut matched_files = HashSet::new();
    let mut total = 0usize;

    for entry in WalkDir::new(&base).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if let Some(filter) = &file_filter {
            let rel = path.strip_prefix(&base).unwrap_or(path);
            if !filter.matches_path(rel) {
                continue;
            }
        }
        if let Some(ext) = &type_ext {
            let current_ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if current_ext != ext {
                continue;
            }
        }

        let content = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(_) => continue,
        };

        if input.multiline.unwrap_or(false) {
            for mat in regex.find_iter(&content) {
                if total < offset {
                    total += 1;
                    continue;
                }
                if matches.len() >= head_limit {
                    break;
                }
                let start = mat.start().saturating_sub(80);
                let end = (mat.end() + 80).min(content.len());
                let snippet = content[start..end].to_string();
                let rel = path
                    .strip_prefix(&base)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                matches.push(json!({
                    "file_path": rel,
                    "match": snippet,
                    "start": mat.start(),
                    "end": mat.end(),
                }));
                matched_files.insert(rel);
                total += 1;
            }
            continue;
        }

        let lines: Vec<&str> = content.lines().collect();
        let mut i = 0usize;
        while i < lines.len() {
            if regex.is_match(lines[i]) {
                if total < offset {
                    total += 1;
                    i += 1;
                    continue;
                }
                if matches.len() >= head_limit {
                    break;
                }
                let start = i.saturating_sub(before);
                let end = (i + after + 1).min(lines.len());
                let rel = path
                    .strip_prefix(&base)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                let before_lines: Vec<String> = lines[start..i].iter().map(|s| (*s).to_string()).collect();
                let after_lines: Vec<String> = lines[i + 1..end].iter().map(|s| (*s).to_string()).collect();
                matches.push(json!({
                    "file_path": rel,
                    "line_number": if include_line { Some((i + 1) as u64) } else { None::<u64> },
                    "text": lines[i],
                    "before": before_lines,
                    "after": after_lines,
                }));
                matched_files.insert(rel);
                total += 1;
            }
            i += 1;
        }
    }

    let structured = match output_mode.as_str() {
        "count" => json!({ "count": total }),
        "files_with_matches" => json!({
            "files": matched_files.into_iter().collect::<Vec<_>>(),
            "count": total,
        }),
        _ => json!({
            "matches": matches,
            "count": total,
            "truncated": total.saturating_sub(offset) > head_limit,
        }),
    };

    Ok(ToolOutcome {
        text: serde_json::to_string_pretty(&structured)?,
        structured_content: Some(structured),
        is_error: false,
    })
}

async fn read_tool(_state: &Arc<ServerState>, input: ReadInput) -> Result<ToolOutcome, CapabilityError> {
    if input.pages.is_some() {
        return Err(CapabilityError::InvalidRequest("pages is reserved for PDF support in a future extension".into()));
    }
    let path = resolve_path(&input.file_path)?;
    let content = fs::read_to_string(&path)?;
    let lines: Vec<&str> = content.lines().collect();
    let offset = input.offset.unwrap_or(0);
    let limit = input.limit.unwrap_or(200);
    let selected = lines.iter().skip(offset).take(limit).enumerate().map(|(idx, line)| {
        format!("{:>6}\t{}", offset + idx + 1, line)
    }).collect::<Vec<_>>().join("\n");
    let truncated = offset + limit < lines.len();
    Ok(ToolOutcome {
        text: selected.clone(),
        structured_content: Some(json!({
            "file_path": path.to_string_lossy(),
            "start_line": offset + 1,
            "line_count": selected.lines().count(),
            "truncated": truncated,
            "content": selected,
        })),
        is_error: false,
    })
}

async fn edit_tool(_state: &Arc<ServerState>, input: EditInput) -> Result<ToolOutcome, CapabilityError> {
    let path = resolve_path(&input.file_path)?;
    let existing = fs::read_to_string(&path)?;
    let replace_all = input.replace_all.unwrap_or(false);
    let matches = existing.matches(&input.old_string).count();
    if matches == 0 {
        return Err(CapabilityError::InvalidRequest("old_string not found".into()));
    }
    if matches > 1 && !replace_all {
        return Err(CapabilityError::InvalidRequest(format!("old_string matched {matches} times; set replace_all=true or provide a more specific snippet")));
    }
    let updated = if replace_all {
        existing.replace(&input.old_string, &input.new_string)
    } else {
        existing.replacen(&input.old_string, &input.new_string, 1)
    };
    fs::write(&path, &updated)?;
    let diff = simple_diff(&existing, &updated);
    Ok(ToolOutcome {
        text: format!("updated: {}", path.to_string_lossy()),
        structured_content: Some(json!({
            "file_path": path.to_string_lossy(),
            "status": "updated",
            "diff": diff,
            "content": updated,
        })),
        is_error: false,
    })
}

async fn write_tool(_state: &Arc<ServerState>, input: WriteInput) -> Result<ToolOutcome, CapabilityError> {
    let path = resolve_path(&input.file_path)?;
    let existed = path.exists();
    let before = if existed { fs::read_to_string(&path).ok() } else { None };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, &input.content)?;
    let diff = before.as_deref().map(|old| simple_diff(old, &input.content)).unwrap_or_else(|| format!("+ {} bytes", input.content.len()));
    Ok(ToolOutcome {
        text: format!("{}: {}", if existed { "updated" } else { "created" }, path.to_string_lossy()),
        structured_content: Some(json!({
            "file_path": path.to_string_lossy(),
            "status": if existed { "updated" } else { "created" },
            "diff": diff,
            "content": input.content,
        })),
        is_error: false,
    })
}

async fn bash_tool(state: &Arc<ServerState>, input: BashInput) -> Result<ToolOutcome, CapabilityError> {
    let _sandbox_override = input.dangerously_disable_sandbox.unwrap_or(false);
    let timeout_ms = input.timeout.unwrap_or(120_000);
    if input.run_in_background.unwrap_or(false) {
        let task_id = format!("task_{}", uuid::Uuid::new_v4());
        let output_file = state.task_dir.join(format!("{task_id}.log"));
        state.task_index.lock().unwrap().insert(task_id.clone(), output_file.clone());
        let command = input.command.clone();
        task::spawn(run_background_bash(task_id.clone(), command, output_file.clone(), timeout_ms));
        return Ok(ToolOutcome {
            text: format!("background task launched: {task_id}"),
            structured_content: Some(json!({
                "task_id": task_id,
                "status": "running",
                "output_file": output_file.to_string_lossy(),
            })),
            is_error: false,
        });
    }

    let start = Instant::now();
    let output = Command::new("sh")
        .arg("-lc")
        .arg(&input.command)
        .current_dir(&state.workspace_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| CapabilityError::Bash(e.to_string()))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);
    let is_error = !output.status.success();
    let text = format!(
        "exit_code: {exit_code}\nduration_ms: {duration_ms}\n\nstdout:\n{stdout}\n\nstderr:\n{stderr}"
    );

    Ok(ToolOutcome {
        text,
        structured_content: Some(json!({
            "exit_code": exit_code,
            "duration_ms": duration_ms,
            "stdout": stdout,
            "stderr": stderr,
            "is_error": is_error,
        })),
        is_error,
    })
}

async fn run_background_bash(task_id: String, command: String, output_file: PathBuf, timeout_ms: u64) {
    let start = Instant::now();
    let output = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        Command::new("sh")
            .arg("-lc")
            .arg(&command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await;

    let content = match output {
        Ok(Ok(result)) => {
            let stdout = String::from_utf8_lossy(&result.stdout).to_string();
            let stderr = String::from_utf8_lossy(&result.stderr).to_string();
            format!(
                "task_id: {task_id}\nstatus: {}\nexit_code: {}\nduration_ms: {}\n\nstdout:\n{}\n\nstderr:\n{}\n",
                if result.status.success() { "completed" } else { "failed" },
                result.status.code().unwrap_or(-1),
                start.elapsed().as_millis(),
                stdout,
                stderr
            )
        }
        Ok(Err(err)) => format!("task_id: {task_id}\nstatus: failed\nerror: {}\n", err),
        Err(_) => format!("task_id: {task_id}\nstatus: timeout\n"),
    };

    let _ = fs::write(&output_file, content);
}

fn resolve_path(input: &str) -> Result<PathBuf, CapabilityError> {
    let path = PathBuf::from(input);
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn simple_diff(old: &str, new: &str) -> String {
    let mut diff = String::new();
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let max = old_lines.len().max(new_lines.len());
    for i in 0..max {
        match (old_lines.get(i), new_lines.get(i)) {
            (Some(a), Some(b)) if a == b => {}
            (Some(a), Some(b)) => {
                diff.push_str(&format!("- {}\n+ {}\n", a, b));
            }
            (Some(a), None) => diff.push_str(&format!("- {}\n", a)),
            (None, Some(b)) => diff.push_str(&format!("+ {}\n", b)),
            (None, None) => {}
        }
    }
    if diff.is_empty() {
        diff.push_str("(no textual diff detected)");
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_smoke() {
        let diff = simple_diff("a\nb", "a\nc");
        assert!(diff.contains("- b"));
        assert!(diff.contains("+ c"));
    }

    #[test]
    fn tool_list_has_six_tools() {
        assert_eq!(tool_definitions().len(), 6);
    }
}
