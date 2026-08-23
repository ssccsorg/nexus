// ── ProcessManager — child process lifecycle ──────────────────────────
//
// Manages child processes spawned by nexd. Each child is tracked by PID
// and monitored for exit. On daemon shutdown, all children are gracefully
// terminated.

use std::collections::HashMap;
use std::time::Duration;
use tokio::process::{Child, Command};
use tracing::info;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// Handle representing a managed child process.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentHandle {
    pub pid: u32,
    pub command: String,
}

/// Internal state for a tracked child process.
struct ChildEntry {
    handle: AgentHandle,
    child: Option<Child>,
    args: Vec<String>,
    respawn: bool,
}

/// Manages lifecycle of child agent processes.
pub struct ProcessManager {
    children: HashMap<u32, ChildEntry>,
    shutting_down: bool,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            children: HashMap::new(),
            shutting_down: false,
        }
    }

    /// Spawn a new child process with the given command and arguments.
    pub fn spawn(&mut self, command: &str, args: &[String]) -> Result<AgentHandle, String> {
        self.spawn_managed(command, args, false)
    }

    /// Spawn a child process, optionally respawning it when it exits.
    /// Respawnable children (the nex-server it supervises) are brought
    /// back by [`try_reap`] with their original command; agent processes
    /// spawned on demand are not respawned.
    pub fn spawn_managed(
        &mut self,
        command: &str,
        args: &[String],
        respawn: bool,
    ) -> Result<AgentHandle, String> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.kill_on_drop(true);

        let child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        let pid = child.id().unwrap_or(0);
        let handle = AgentHandle {
            pid,
            command: command.to_string(),
        };

        info!(pid, command = %command, respawn, "spawned child process");

        self.children.insert(
            pid,
            ChildEntry {
                handle: handle.clone(),
                child: Some(child),
                args: args.to_vec(),
                respawn,
            },
        );

        Ok(handle)
    }

    /// Try to reap any exited children. Respawnable children (the
    /// supervised nex-server) are restarted with their original command;
    /// other children are removed. While shutting down, no child is
    /// respawned.
    pub fn try_reap(&mut self) {
        let mut dead: Vec<u32> = Vec::new();
        let mut respawns: Vec<(String, Vec<String>)> = Vec::new();
        for (&pid, entry) in self.children.iter_mut() {
            if let Some(ref mut child) = entry.child
                && let Ok(Some(status)) = child.try_wait()
            {
                info!(pid, exit = %status, command = %entry.handle.command, "child process exited");
                if entry.respawn && !self.shutting_down {
                    respawns.push((entry.handle.command.clone(), entry.args.clone()));
                }
                dead.push(pid);
            }
        }

        for pid in dead {
            self.children.remove(&pid);
        }
        for (command, args) in respawns {
            if let Ok(handle) = self.spawn_managed(&command, &args, true) {
                info!(command = %command, pid = handle.pid, "respawned crashed child");
            }
        }
    }

    /// Gracefully stop all children: send SIGTERM, wait up to `timeout`
    /// for exit, then force-kill the remainder. Marks the manager as
    /// shutting down so [`try_reap`] does not respawn.
    pub fn shutdown_graceful(&mut self, timeout: Duration) {
        self.shutting_down = true;

        let pids: Vec<i32> = self.children.keys().map(|pid| *pid as i32).collect();
        for pid in &pids {
            let _ = kill(Pid::from_raw(*pid), Signal::SIGTERM);
        }
        info!(children = pids.len(), timeout = ?timeout, "sending SIGTERM to children");

        let deadline = std::time::Instant::now() + timeout;
        loop {
            let mut all_exited = true;
            for entry in self.children.values_mut() {
                if let Some(ref mut child) = entry.child
                    && child.try_wait().ok().flatten().is_none()
                {
                    all_exited = false;
                }
            }
            if all_exited || std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        for (_pid, entry) in self.children.drain() {
            if let Some(mut child) = entry.child {
                let _ = child.start_kill();
            }
        }
    }

    /// List all managed agents.
    pub fn list_agents(&self) -> Vec<AgentHandle> {
        self.children.values().map(|e| e.handle.clone()).collect()
    }

    /// Kill a specific agent by PID.
    pub fn kill(&mut self, pid: u32) -> Result<(), String> {
        let entry = self
            .children
            .remove(&pid)
            .ok_or_else(|| format!("no such agent pid={pid}"))?;
        if let Some(mut child) = entry.child {
            child
                .start_kill()
                .map_err(|e| format!("kill failed for pid={pid}: {e}"))?;
        }
        info!(pid, "killed child process");
        Ok(())
    }
}
