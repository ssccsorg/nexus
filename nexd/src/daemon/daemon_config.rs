// ── Daemon configuration
use std::time::Duration;
use crate::daemon::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel { Trace, Debug, Info, Warn, Error }
impl Default for LogLevel { fn default() -> Self { Self::Info } }

#[derive(Debug, Clone)]
pub struct Config {
    pub name: String,
    pub log_level: LogLevel,
    pub shutdown_timeout: Duration,
    pub force_shutdown_timeout: Duration,
    pub kill_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: String::from("nexd"),
            log_level: LogLevel::Info,
            shutdown_timeout: Duration::from_secs(10),
            force_shutdown_timeout: Duration::from_secs(15),
            kill_timeout: Duration::from_secs(20),
        }
    }
}

impl Config {
    pub fn builder() -> ConfigBuilder { ConfigBuilder::new() }
    pub fn new() -> Result<Self> { Ok(Self::default()) }
}

#[derive(Debug)]
pub struct ConfigBuilder { config: Config }
impl ConfigBuilder {
    fn new() -> Self { Self { config: Config::default() } }
    pub fn name(mut self, name: impl Into<String>) -> Self { self.config.name = name.into(); self }
    pub fn log_level(mut self, level: LogLevel) -> Self { self.config.log_level = level; self }
    pub fn shutdown_timeout(mut self, t: Duration) -> Result<Self> { self.config.shutdown_timeout = t; Ok(self) }
    pub fn force_shutdown_timeout(mut self, t: Duration) -> Result<Self> { self.config.force_shutdown_timeout = t; Ok(self) }
    pub fn kill_timeout(mut self, t: Duration) -> Result<Self> { self.config.kill_timeout = t; Ok(self) }
    pub fn build(self) -> Result<Config> {
        if self.config.shutdown_timeout >= self.config.force_shutdown_timeout {
            return Err(Error::invalid_config("shutdown_timeout must be < force_shutdown_timeout"));
        }
        if self.config.force_shutdown_timeout >= self.config.kill_timeout {
            return Err(Error::invalid_config("force_shutdown_timeout must be < kill_timeout"));
        }
        Ok(self.config)
    }
}
