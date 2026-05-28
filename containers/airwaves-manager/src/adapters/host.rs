use std::process::Command;

use crate::error::AppError;
use crate::ports::HostPort;

/// Services the manager is allowed to restart on the host.
const ALLOWED_SERVICES: &[&str] = &[
    "avahi-daemon",
    "systemd-networkd",
    "systemd-resolved",
    "docker",
    "ssh",
    "sshd",
    "airwaves-containers",
];

/// Executes privileged operations against the host. When running inside the
/// management container (`HOST_VIA_NSENTER=1`), commands are run in the host's
/// namespaces via `nsenter -t 1`, so they talk to the host's systemd, hostnamed
/// and timedated. When running natively (dev), commands run directly.
pub struct HostAdapter {
    via_nsenter: bool,
}

impl HostAdapter {
    pub fn new() -> Self {
        let via_nsenter = std::env::var("HOST_VIA_NSENTER")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);
        if via_nsenter {
            tracing::info!("Host control enabled via nsenter (host PID namespace)");
        } else {
            tracing::warn!("Host control running in direct mode (not in host namespaces)");
        }
        Self { via_nsenter }
    }

    /// Run a host command and map a non-zero exit (or spawn failure) to an error.
    async fn run(&self, args: Vec<String>) -> Result<(), AppError> {
        let via_nsenter = self.via_nsenter;
        let description = args.join(" ");
        tokio::task::spawn_blocking(move || {
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let mut cmd = if via_nsenter {
                let mut c = Command::new("nsenter");
                c.args(["-t", "1", "-m", "-u", "-i", "-n", "-p", "--"]);
                c.args(&arg_refs);
                c
            } else {
                let mut c = Command::new(arg_refs[0]);
                c.args(&arg_refs[1..]);
                c
            };
            let output = cmd
                .output()
                .map_err(|e| AppError::Internal(format!("Failed to run '{description}': {e}")))?;
            if output.status.success() {
                Ok(())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                Err(AppError::Internal(format!(
                    "Command '{description}' failed (exit {}): {}",
                    output.status.code().unwrap_or(-1),
                    stderr.trim()
                )))
            }
        })
        .await
        .map_err(|e| AppError::Internal(format!("Task join error: {e}")))?
    }

    /// Run a command after a short delay, detached, so the HTTP response can be
    /// sent before the host action takes effect (used for reboot/shutdown).
    fn run_detached_delayed(&self, args: Vec<String>) {
        let via_nsenter = self.via_nsenter;
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let mut cmd = if via_nsenter {
                let mut c = Command::new("nsenter");
                c.args(["-t", "1", "-m", "-u", "-i", "-n", "-p", "--"]);
                c.args(&arg_refs);
                c
            } else {
                let mut c = Command::new(arg_refs[0]);
                c.args(&arg_refs[1..]);
                c
            };
            match cmd.output() {
                Ok(o) if o.status.success() => {}
                Ok(o) => tracing::error!(
                    "Detached host command {:?} failed: {}",
                    args,
                    String::from_utf8_lossy(&o.stderr).trim()
                ),
                Err(e) => tracing::error!("Detached host command {:?} errored: {e}", args),
            }
        });
    }
}

/// Validate an RFC 1123 hostname label (lowercased, <=63 chars, alnum + hyphen).
fn valid_hostname(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() || n.len() > 63 {
        return false;
    }
    let bytes = n.as_bytes();
    if bytes[0] == b'-' || bytes[bytes.len() - 1] == b'-' {
        return false;
    }
    n.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Validate a timezone identifier (e.g. "America/New_York"). Conservative: no
/// shell metacharacters, must look like a tz path component set.
fn valid_timezone(tz: &str) -> bool {
    !tz.is_empty()
        && tz.len() <= 64
        && tz
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '+' | '-'))
}

impl HostPort for HostAdapter {
    async fn set_hostname(&self, hostname: &str) -> Result<(), AppError> {
        let name = hostname.trim().to_lowercase();
        if !valid_hostname(&name) {
            return Err(AppError::BadRequest(format!(
                "Invalid hostname '{hostname}': must be 1-63 chars, alphanumeric or hyphen, not starting/ending with a hyphen"
            )));
        }
        self.run(vec![
            "hostnamectl".into(),
            "set-hostname".into(),
            name.clone(),
        ])
        .await?;
        // Refresh mDNS advertisement; ignore failure (avahi may be absent in dev).
        let _ = self
            .run(vec![
                "systemctl".into(),
                "restart".into(),
                "avahi-daemon".into(),
            ])
            .await;
        Ok(())
    }

    async fn reboot(&self) -> Result<(), AppError> {
        self.run_detached_delayed(vec!["systemctl".into(), "reboot".into()]);
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), AppError> {
        self.run_detached_delayed(vec!["systemctl".into(), "poweroff".into()]);
        Ok(())
    }

    async fn restart_service(&self, service: &str) -> Result<(), AppError> {
        if !ALLOWED_SERVICES.contains(&service) {
            return Err(AppError::BadRequest(format!(
                "Service '{service}' is not in the allowlist"
            )));
        }
        self.run(vec![
            "systemctl".into(),
            "restart".into(),
            service.to_string(),
        ])
        .await
    }

    async fn set_timezone(&self, timezone: &str) -> Result<(), AppError> {
        if !valid_timezone(timezone) {
            return Err(AppError::BadRequest(format!(
                "Invalid timezone '{timezone}'"
            )));
        }
        self.run(vec![
            "timedatectl".into(),
            "set-timezone".into(),
            timezone.to_string(),
        ])
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_validation() {
        assert!(valid_hostname("airwaves-a1b2c3"));
        assert!(valid_hostname("node1"));
        assert!(!valid_hostname(""));
        assert!(!valid_hostname("-bad"));
        assert!(!valid_hostname("bad-"));
        assert!(!valid_hostname("has space"));
        assert!(!valid_hostname("inject;rm -rf"));
        assert!(!valid_hostname(&"a".repeat(64)));
    }

    #[test]
    fn timezone_validation() {
        assert!(valid_timezone("America/New_York"));
        assert!(valid_timezone("UTC"));
        assert!(valid_timezone("Etc/GMT+5"));
        assert!(!valid_timezone("../etc/passwd"));
        assert!(!valid_timezone("foo;reboot"));
        assert!(!valid_timezone(""));
    }
}
