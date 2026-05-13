//! Constructs ACP protocol messages for the --prompt workflow.

use std::io::Read;

use serde_json::{Value, json};

pub fn initialize_message() -> Vec<u8> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientInfo": {
                "name": "acp-spawn",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    });
    let mut encoded = serde_json::to_string(&msg).expect("initialize message should serialize");
    encoded.push('\n');
    encoded.into_bytes()
}

pub fn session_new_message(cwd: &str) -> Vec<u8> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/new",
        "params": {
            "cwd": cwd,
            "mcpServers": []
        }
    });
    let mut encoded = serde_json::to_string(&msg).expect("session/new message should serialize");
    encoded.push('\n');
    encoded.into_bytes()
}

pub fn session_prompt_message(session_id: &str, prompt_text: &str) -> Vec<u8> {
    let msg = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{"type": "text", "text": prompt_text}]
        }
    });
    let mut encoded = serde_json::to_string(&msg).expect("session/prompt message should serialize");
    encoded.push('\n');
    encoded.into_bytes()
}

pub struct HandshakeResult {
    pub response: Value,
    pub buffered_lines: Vec<String>,
}

pub fn read_response<R: Read>(
    stdout_reader: &mut std::io::BufReader<R>,
    expected_id: i64,
) -> Result<HandshakeResult, String> {
    let mut buffered_lines = Vec::new();
    loop {
        let mut line = String::new();
        let bytes = std::io::BufRead::read_line(stdout_reader, &mut line)
            .map_err(|e| format!("failed to read agent stdout: {e}"))?;
        if bytes == 0 {
            return Err("agent closed stdout before response".into());
        }
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed.is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(trimmed).map_err(|e| format!("invalid json from agent: {e}"))?;
        if let Some(id) = value.get("id").and_then(|v| v.as_i64())
            && id == expected_id
        {
            return Ok(HandshakeResult {
                response: value,
                buffered_lines,
            });
        }
        buffered_lines.push(trimmed.to_string());
    }
}

pub fn extract_session_id(response: &Value) -> Result<String, String> {
    response
        .get("result")
        .and_then(|r| r.get("sessionId"))
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "response missing result.sessionId".into())
}
