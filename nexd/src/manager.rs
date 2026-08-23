// ── ProcessManager — child process lifecycle ──────────────────────────
//
// Manages child processes spawned by nexd. Each child is tracked by PID
// and monitored for exit. On daemon shutdown, all children are gracefully
// terminated.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tracing::{error, info};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

/// Handle representing a managed child process.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentHandle {
    pub pid: u32,
    pub command: String,
}

/// Children that exit within this window of their spawn count as
/// crash-looping and advance the respawn circuit breaker.
const RAPID_EXIT_WINDOW: Duration = Duration::from_secs(10);
/// Consecutive rapid exits that stop respawning a command.
const MAX_RAPID_EXITS: u32 = 5;

/// Internal state for a tracked child process.
struct ChildEntry {
    handle: AgentHandle,
    child: Option<Child>,
    args: Vec<String>,
    respawn: bool,
    spawned_at: Instant,
}

/// Manages lifecycle of child agent processes.
pub struct ProcessManager {
    children: HashMap<u32, ChildEntry>,
    shutting_down: bool,
    /// Consecutive rapid-exit count per command (respawn circuit
    /// breaker). Reset when a child survives the rapid-exit window.
    respawn_exits: HashMap<String, u32>,
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
            respawn_exits: HashMap::new(),
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
                spawned_at: Instant::now(),
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
                    let rapid = entry.spawned_at.elapsed() < RAPID_EXIT_WINDOW;
                    let failures = self
                        .respawn_exits
                        .entry(entry.handle.command.clone())
                        .or_insert(0);
                    if rapid {
                        *failures += 1;
                    } else {
                        *failures = 0;
                    }
                    if *failures >= MAX_RAPID_EXITS {
                        error!(
                            command = %entry.handle.command,
                            failures,
                            "stopping respawn after repeated rapid crashes"
                        );
                    } else {
                        respawns.push((entry.handle.command.clone(), entry.args.clone()));
                    }
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

    /// Send SIGTERM to all children and mark the manager as shutting
    /// down so [`try_reap`] does not respawn. Non-blocking.
    pub fn request_shutdown_children(&mut self) {
        self.shutting_down = true;

        let pids: Vec<i32> = self.children.keys().map(|pid| *pid as i32).collect();
        for pid in &pids {
            let _ = kill(Pid::from_raw(*pid), Signal::SIGTERM);
        }
        info!(children = pids.len(), "sending SIGTERM to children");
    }

    /// Whether every tracked child has exited.
    pub fn all_children_exited(&mut self) -> bool {
        self.children.values_mut().all(|entry| {
            entry
                .child
                .as_mut()
                .map(|c| c.try_wait().ok().flatten().is_some())
                .unwrap_or(true)
        })
    }

    /// Force-kill all remaining children and clear the tracking map.
    pub fn force_kill_children(&mut self) {
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
