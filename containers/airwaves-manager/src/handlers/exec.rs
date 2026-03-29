use axum::Json;
use serde::{Deserialize, Serialize};
use std::process::Command;

use crate::AppError;

#[derive(Deserialize)]
pub struct ExecRequest {
    pub command: String,
}

#[derive(Serialize)]
pub struct ExecResponse {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Allowed command prefixes for the web terminal.
/// Only these commands can be executed - everything else is rejected.
const ALLOWED_COMMANDS: &[&str] = &[
    "uname",
    "hostname",
    "uptime",
    "date",
    "whoami",
    "id",
    "free",
    "df",
    "top -bn1",
    "ps aux",
    "ps",
    "cat /etc/airwaves-release",
    "cat /etc/os-release",
    "cat /etc/hostname",
    "cat /etc/airwaves/config.json",
    "ip addr",
    "ip link",
    "ip route",
    "ss -tlnp",
    "docker ps",
    "docker stats --no-stream",
    "docker images",
    "docker network ls",
    "docker version",
    "systemctl status",
    "systemctl list-units --type=service --state=running",
    "journalctl -n",
    "lsusb",
    "lsblk",
    "lscpu",
    "dmesg | tail",
    "ls",
    "pwd",
];

fn is_command_allowed(input: &str) -> bool {
    let trimmed = input.trim();

    // Exact match
    if ALLOWED_COMMANDS.contains(&trimmed) {
        return true;
    }

    // Prefix match (allows flags/args for allowed base commands)
    for allowed in ALLOWED_COMMANDS {
        if trimmed.starts_with(allowed) {
            // Ensure it's a proper prefix (followed by space or end)
            let rest = &trimmed[allowed.len()..];
            if rest.is_empty() || rest.starts_with(' ') {
                return true;
            }
        }
    }

    // Block shell metacharacters that could enable injection
    if trimmed.contains(';')
        || trimmed.contains("&&")
        || trimmed.contains("||")
        || trimmed.contains('`')
        || trimmed.contains("$(")
        || trimmed.contains('>') && !trimmed.starts_with("dmesg")
        || trimmed.contains('<')
    {
        return false;
    }

    false
}

pub async fn exec_command(Json(req): Json<ExecRequest>) -> Result<Json<ExecResponse>, AppError> {
    let input = req.command.trim().to_string();

    // Handle built-in shell commands
    match input.as_str() {
        "help" => {
            return Ok(Json(ExecResponse {
                exit_code: 0,
                stdout: format!(
                    "Airwaves OS Web Terminal\n\
                     Available commands:\n  {}\n\n\
                     Type 'clear' in the UI to clear the screen.",
                    ALLOWED_COMMANDS.join("\n  ")
                ),
                stderr: String::new(),
            }));
        }
        "clear" => {
            return Ok(Json(ExecResponse {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            }));
        }
        _ => {}
    }

    if !is_command_allowed(&input) {
        return Ok(Json(ExecResponse {
            exit_code: 1,
            stdout: String::new(),
            stderr: format!("Permission denied: '{}' is not an allowed command.\nType 'help' for available commands.", input),
        }));
    }

    // Execute via sh -c to support pipes in allowed commands (like dmesg | tail)
    let output = Command::new("sh")
        .args(["-c", &input])
        .output()
        .map_err(|e| AppError::Internal(format!("Failed to execute: {}", e)))?;

    Ok(Json(ExecResponse {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }))
}
