// ── ProcessManager — child process lifecycle ──────────────────────────
//
// Manages child processes spawned by nexd. Each child is tracked by PID
// and monitored for exit. On daemon shutdown, all children are gracefully
// terminated.

use std::collections::HashMap;
use tokio::process::{Child, Command};
use tracing::info;

/// Handle representing a managed child process.
#[derive(Debug, Clone)]
pub struct AgentHandle {
    pub pid: u32,
    pub command: String,
}

/// Internal state for a tracked child process.
struct ChildEntry {
    handle: AgentHandle,
    child: Option<Child>,
}

/// Manages lifecycle of child agent processes.
pub struct ProcessManager {
    children: HashMap<u32, ChildEntry>,
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
        }
    }

    /// Spawn a new child process with the given command and arguments.
    pub fn spawn(&mut self, command: &str, args: &[String]) -> Result<AgentHandle, String> {
        let mut cmd = Command::new(command);
        cmd.args(args);
        cmd.kill_on_drop(true);

        let child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        let pid = child.id().unwrap_or(0);
        let handle = AgentHandle {
            pid,
            command: command.to_string(),
        };

        info!(pid, command = %command, "spawned child process");

        self.children.insert(
            pid,
            ChildEntry {
                handle: handle.clone(),
                child: Some(child),
            },
        );

        Ok(handle)
    }

    /// Try to reap any exited children. Synchronous (non-blocking try_wait).
    pub fn try_reap(&mut self) {
        let dead: Vec<u32> = self
            .children
            .iter_mut()
            .filter_map(|(&pid, entry)| {
                if let Some(ref mut child) = entry.child {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            info!(pid, exit = %status, "child process exited");
                            Some(pid)
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .collect();

        for pid in dead {
            self.children.remove(&pid);
        }
    }

    /// Synchronously initiate kill for all children without awaiting.
    /// This avoids holding a MutexGuard across an await point.
    pub fn shutdown_sync(&mut self) {
        if self.children.is_empty() {
            return;
        }

        info!(
            "initiating kill for {} child processes",
            self.children.len()
        );

        for (_pid, entry) in self.children.drain() {
            if let Some(mut child) = entry.child {
                let _ = child.start_kill();
            }
        }

        info!("kill initiated for all child processes");
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
