//! Ref-counted shared Codex app-server registry.

use std::{collections::HashMap, sync::Arc};

use arky_provider::ProviderError;
use tokio::{runtime::Handle, sync::Mutex, task::JoinHandle, time::sleep};

use crate::{CodexAppServer, CodexProviderConfig};

#[derive(Debug)]
struct RegistrySlot {
    server: Arc<CodexAppServer>,
    config: CodexProviderConfig,
    ref_count: usize,
    idle_task: Option<JoinHandle<()>>,
}

#[derive(Debug, Default)]
struct RegistryState {
    slots: HashMap<String, RegistrySlot>,
}

/// Shared Codex app-server registry keyed by compatible runtime configuration.
#[derive(Debug, Clone, Default)]
pub struct CodexServerRegistry {
    state: Arc<Mutex<RegistryState>>,
}

impl CodexServerRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquires a shared app-server lease for the given configuration.
    pub async fn acquire(
        &self,
        config: CodexProviderConfig,
    ) -> Result<CodexServerLease, ProviderError> {
        let key = config.registry_key();
        let existing_server = {
            let state = self.state.lock().await;
            state.slots.get(&key).map(|slot| slot.server.clone())
        };

        if let Some(existing_server) = existing_server {
            if existing_server.is_alive().await {
                let mut state = self.state.lock().await;
                if let Some(slot) = state.slots.get_mut(&key) {
                    if Arc::ptr_eq(&slot.server, &existing_server) {
                        if let Some(idle_task) = slot.idle_task.take() {
                            idle_task.abort();
                        }
                        slot.ref_count = slot.ref_count.saturating_add(1);
                        return Ok(CodexServerLease::new(
                            self.clone(),
                            key,
                            slot.server.clone(),
                        ));
                    }
                }
            }
        }

        let stale_server = {
            let mut state = self.state.lock().await;
            state.slots.remove(&key).map(|slot| slot.server)
        };

        if let Some(stale_server) = stale_server {
            let _ = stale_server.shutdown().await;
        }

        let server = Arc::new(CodexAppServer::spawn(config.clone()).await?);
        {
            let mut state = self.state.lock().await;
            state.slots.insert(
                key.clone(),
                RegistrySlot {
                    server: server.clone(),
                    config,
                    ref_count: 1,
                    idle_task: None,
                },
            );
        }

        Ok(CodexServerLease::new(self.clone(), key, server))
    }

    /// Forces a reconfigure by replacing the shared server for this key.
    pub async fn reconfigure(
        &self,
        config: CodexProviderConfig,
    ) -> Result<CodexServerLease, ProviderError> {
        let key = config.registry_key();
        let previous = {
            let mut state = self.state.lock().await;
            state.slots.remove(&key).map(|slot| slot.server)
        };
        if let Some(previous) = previous {
            let _ = previous.shutdown().await;
        }

        self.acquire(config).await
    }

    /// Shuts down and clears every tracked server.
    pub async fn dispose_all(&self) -> Result<(), ProviderError> {
        let servers = {
            let mut state = self.state.lock().await;
            state
                .slots
                .drain()
                .map(|(_, mut slot)| {
                    if let Some(idle_task) = slot.idle_task.take() {
                        idle_task.abort();
                    }
                    slot.server
                })
                .collect::<Vec<_>>()
        };

        for server in servers {
            server.shutdown().await?;
        }

        Ok(())
    }

    async fn release_key(&self, key: String) {
        let maybe_shutdown = {
            let mut state = self.state.lock().await;
            let Some(slot) = state.slots.get_mut(&key) else {
                return;
            };

            if slot.ref_count > 1 {
                slot.ref_count -= 1;
                return;
            }

            if slot.config.idle_shutdown_timeout.is_zero() {
                state.slots.remove(&key).map(|slot| slot.server)
            } else {
                let registry = self.clone();
                let delay = slot.config.idle_shutdown_timeout;
                let task_key = key.clone();
                let idle_task = tokio::spawn(async move {
                    sleep(delay).await;
                    registry.finalize_idle_shutdown(task_key).await;
                });
                slot.ref_count = 0;
                slot.idle_task = Some(idle_task);
                None
            }
        };

        if let Some(server) = maybe_shutdown {
            let _ = server.shutdown().await;
        }
    }

    async fn finalize_idle_shutdown(&self, key: String) {
        let server = {
            let mut state = self.state.lock().await;
            match state.slots.get(&key) {
                Some(slot) if slot.ref_count == 0 => {}
                _ => return,
            }

            state.slots.remove(&key).map(|slot| slot.server)
        };

        if let Some(server) = server {
            let _ = server.shutdown().await;
        }
    }
}

/// Active reference to a shared Codex app-server.
#[derive(Debug)]
pub struct CodexServerLease {
    registry: CodexServerRegistry,
    key: String,
    server: Arc<CodexAppServer>,
}

impl CodexServerLease {
    const fn new(registry: CodexServerRegistry, key: String, server: Arc<CodexAppServer>) -> Self {
        Self {
            registry,
            key,
            server,
        }
    }

    /// Returns the shared app-server.
    #[must_use]
    pub fn server(&self) -> Arc<CodexAppServer> {
        self.server.clone()
    }
}

impl Drop for CodexServerLease {
    fn drop(&mut self) {
        let registry = self.registry.clone();
        let key = self.key.clone();
        if let Ok(handle) = Handle::try_current() {
            handle.spawn(async move {
                registry.release_key(key).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use pretty_assertions::{assert_eq, assert_ne};
    use tempfile::TempDir;

    use super::CodexServerRegistry;
    use crate::{ApprovalMode, CodexProcessConfig, CodexProviderConfig};

    fn fixture_config(tempdir: &TempDir) -> CodexProviderConfig {
        let mut config = CodexProviderConfig {
            binary: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/fake_codex_app_server.js")
                .display()
                .to_string(),
            process: CodexProcessConfig {
                allow_npx: false,
                ..CodexProcessConfig::default()
            },
            approval_mode: ApprovalMode::AutoApprove,
            idle_shutdown_timeout: Duration::from_millis(25),
            request_timeout: Duration::from_secs(5),
            scheduler_timeout: Duration::from_secs(5),
            startup_timeout: Duration::from_secs(5),
            ..CodexProviderConfig::default()
        };
        config
            .env
            .insert("ARKY_CODEX_FIXTURE".to_owned(), "1".to_owned());
        config.env.insert(
            "ARKY_CODEX_FIXTURE_STATE".to_owned(),
            tempdir
                .path()
                .join("fixture-state.json")
                .display()
                .to_string(),
        );
        config
    }

    #[tokio::test]
    async fn registry_should_reuse_and_idle_shutdown_servers() {
        let tempdir = TempDir::new().expect("tempdir should create");
        let registry = CodexServerRegistry::new();
        let config = fixture_config(&tempdir);

        let first = registry
            .acquire(config.clone())
            .await
            .expect("first lease should acquire");
        let second = registry
            .acquire(config)
            .await
            .expect("second lease should acquire");

        let first_pid = first.server().process_id().await;
        let second_pid = second.server().process_id().await;
        assert_eq!(first_pid, second_pid);

        drop(first);
        drop(second);
        tokio::time::sleep(Duration::from_millis(80)).await;

        let third = registry
            .acquire(fixture_config(&tempdir))
            .await
            .expect("server should restart after idle shutdown");
        let third_pid = third.server().process_id().await;
        assert!(third_pid.is_some());
        assert_ne!(third_pid, second_pid);
    }

    #[tokio::test]
    async fn registry_should_reconfigure_shared_keys() {
        let tempdir = TempDir::new().expect("tempdir should create");
        let registry = CodexServerRegistry::new();
        let mut initial = fixture_config(&tempdir);
        initial.shared_app_server_key = Some("shared".to_owned());

        let first = registry
            .acquire(initial.clone())
            .await
            .expect("first lease should acquire");
        let first_pid = first.server().process_id().await;
        drop(first);

        let mut updated = initial;
        updated.process.experimental_api = true;
        let second = registry
            .reconfigure(updated)
            .await
            .expect("reconfigure should replace the server");
        let second_pid = second.server().process_id().await;

        assert_ne!(first_pid, second_pid);
    }
}
