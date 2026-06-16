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

    /// Run a host command and capture stdout (best-effort; None on failure).
    async fn run_capture(&self, args: Vec<String>) -> Option<String> {
        let via_nsenter = self.via_nsenter;
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
            cmd.output().ok().and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).to_string())
                } else {
                    None
                }
            })
        })
        .await
        .ok()
        .flatten()
    }

    /// Count upgradable apt packages on the host (best-effort, uses cached
    /// package lists — does not run `apt-get update`).
    pub async fn upgradable_packages(&self) -> Option<u32> {
        let out = self
            .run_capture(vec![
                "apt-get".into(),
                "-s".into(),
                "-o".into(),
                "Debug::NoLocking=true".into(),
                "upgrade".into(),
            ])
            .await?;
        // Lines like "Inst <pkg> ..." indicate a package to be installed/upgraded.
        let count = out.lines().filter(|l| l.starts_with("Inst ")).count();
        Some(count as u32)
    }

    /// True when the unit state means the updater is still doing work. The
    /// updater unit is Type=oneshot without RemainAfterExit, so systemd
    /// reports "activating" (not "active") for its entire run.
    fn is_running_state(state: &str) -> bool {
        matches!(state, "active" | "activating" | "reloading" | "deactivating")
    }

    pub async fn is_update_service_active(&self) -> bool {
        // `systemctl is-active` exits non-zero for every state except
        // "active" (run_capture discards output on non-zero exit), and the
        // oneshot updater unit never reports "active" while running — use
        // `show`, which always exits 0 and prints the raw ActiveState.
        self.run_capture(vec![
            "systemctl".into(),
            "show".into(),
            "-p".into(),
            "ActiveState".into(),
            "--value".into(),
            "airwaves-update.service".into(),
        ])
        .await
        .map(|out| Self::is_running_state(out.trim()))
        .unwrap_or(false)
    }

    /// Refresh host-side bootstrap files from the repo before running an update,
    /// so already-deployed devices pick up out-of-band updater fixes without a
    /// manual bootstrap. Best-effort; the signed manifest performs the
    /// integrity-checked sync once the host updater starts.
    pub async fn refresh_updater_files(&self) -> Result<(), AppError> {
        const RAW: &str = "https://raw.githubusercontent.com/airframesio/airwaves-os/main/armbian/userpatches/extensions/airwaves-os";
        for (src, dest, executable) in [
            (
                "scripts/airwaves-update",
                "/opt/airwaves/scripts/airwaves-update",
                true,
            ),
            (
                "scripts/airwaves-growfs",
                "/opt/airwaves/scripts/airwaves-growfs",
                true,
            ),
            (
                "scripts/airwaves-init",
                "/opt/airwaves/scripts/airwaves-init",
                true,
            ),
            (
                "config/templates/systemd-airwaves-update.service",
                "/etc/systemd/system/airwaves-update.service",
                false,
            ),
            (
                "config/templates/systemd-airwaves-growfs.service",
                "/etc/systemd/system/airwaves-growfs.service",
                false,
            ),
            (
                "config/templates/systemd-airwaves-init.service",
                "/etc/systemd/system/airwaves-init.service",
                false,
            ),
            (
                "config/templates/systemd-airwaves-containers.service",
                "/etc/systemd/system/airwaves-containers.service",
                false,
            ),
        ] {
            let _ = self
                .run(vec![
                    "curl".into(),
                    "-fsSL".into(),
                    format!("{RAW}/{src}"),
                    "-o".into(),
                    dest.into(),
                ])
                .await;
            if executable {
                let _ = self.run(vec!["chmod".into(), "+x".into(), dest.into()]).await;
            }
        }
        let _ = self.run(vec!["systemctl".into(), "daemon-reload".into()]).await;
        Ok(())
    }

    /// Start the host-side updater oneshot service.
    pub async fn start_update_service(&self) -> Result<(), AppError> {
        self.run(vec![
            "systemctl".into(),
            "start".into(),
            "--no-block".into(),
            "airwaves-update.service".into(),
        ])
        .await
    }

    /// List candidate internal install-target disks (JSON array string) as seen
    /// from the HOST, so virtio/NVMe/host disks are enumerated (not the
    /// container's view). Runs airwaves-install --list-json via nsenter.
    pub async fn list_install_disks(&self) -> Result<String, AppError> {
        self.run_capture(vec![
            "/opt/airwaves/scripts/airwaves-install".into(),
            "--list-json".into(),
        ])
        .await
        .ok_or_else(|| AppError::Internal("Failed to list install disks".into()))
    }

    /// Whether `device` is a safe whole-disk path (/dev/<alnum>), guarding
    /// against shell injection before interpolation into a command.
    fn valid_block_device(device: &str) -> bool {
        device
            .strip_prefix("/dev/")
            .map(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or(false)
    }

    /// Launch airwaves-install on the HOST against `device`, fully detached
    /// (setsid + background) so this returns immediately; the installer writes
    /// /etc/airwaves/install/status.json which the manager polls for progress.
    pub async fn start_install(&self, device: &str) -> Result<(), AppError> {
        if !Self::valid_block_device(device) {
            return Err(AppError::BadRequest(format!(
                "Invalid target device: {device}"
            )));
        }
        let inner = format!(
            "setsid sh -c 'AIRWAVES_INSTALL_APPLY=1 /opt/airwaves/scripts/airwaves-install --target {device} >/var/log/airwaves-install.log 2>&1' &"
        );
        self.run(vec!["sh".into(), "-c".into(), inner]).await
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

    #[test]
    fn install_device_validation_blocks_injection() {
        assert!(HostAdapter::valid_block_device("/dev/sda"));
        assert!(HostAdapter::valid_block_device("/dev/nvme0n1"));
        assert!(HostAdapter::valid_block_device("/dev/vda"));
        // Reject partitions-with-paths, shell metacharacters, traversal, empty.
        assert!(!HostAdapter::valid_block_device("/dev/sda1; rm -rf /"));
        assert!(!HostAdapter::valid_block_device("/dev/sda/../../etc"));
        assert!(!HostAdapter::valid_block_device("/dev/"));
        assert!(!HostAdapter::valid_block_device("sda"));
        assert!(!HostAdapter::valid_block_device("/dev/sd a"));
        assert!(!HostAdapter::valid_block_device("/dev/$(reboot)"));
    }

    #[test]
    fn update_service_running_states() {
        // A Type=oneshot unit spends its whole run in "activating" — treating
        // that as not-running made the progress watchdog fail every update
        // mid-run ("last recorded phase was syncing-host-files").
        assert!(HostAdapter::is_running_state("activating"));
        assert!(HostAdapter::is_running_state("active"));
        assert!(HostAdapter::is_running_state("reloading"));
        assert!(HostAdapter::is_running_state("deactivating"));
        assert!(!HostAdapter::is_running_state("inactive"));
        assert!(!HostAdapter::is_running_state("failed"));
        assert!(!HostAdapter::is_running_state(""));
    }
}
