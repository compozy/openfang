//! Tool trait and thread-safe registry implementation.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, RwLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use arky_protocol::{ToolCall, ToolResult};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::{ToolDescriptor, ToolError};

type ToolMap = BTreeMap<String, Arc<dyn Tool>>;

#[derive(Default)]
struct RegistryInner {
    tools: RwLock<ToolMap>,
}

/// Core tool contract used by registries and providers.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns immutable tool metadata.
    fn descriptor(&self) -> ToolDescriptor;

    /// Executes the tool for a concrete tool call.
    async fn execute(
        &self,
        call: ToolCall,
        cancel: CancellationToken,
    ) -> Result<ToolResult, ToolError>;
}

/// RAII handle that unregisters call-scoped tools when dropped.
#[derive(Debug)]
pub struct ToolRegistrationHandle {
    registry: Weak<RegistryInner>,
    canonical_names: Vec<String>,
    active: AtomicBool,
}

impl ToolRegistrationHandle {
    fn new(registry: &Arc<RegistryInner>, canonical_names: Vec<String>) -> Self {
        Self {
            registry: Arc::downgrade(registry),
            canonical_names,
            active: AtomicBool::new(true),
        }
    }

    /// Returns the canonical names managed by this handle.
    #[must_use]
    pub fn canonical_names(&self) -> &[String] {
        &self.canonical_names
    }

    /// Explicitly unregisters the call-scoped tools once.
    pub fn cleanup(&self) -> usize {
        if !self.active.swap(false, Ordering::AcqRel) {
            return 0;
        }

        let registry = self.registry.upgrade();
        let Some(registry) = registry else {
            return 0;
        };

        let mut removed = 0usize;
        let mut tools = write_tools(&registry.tools);
        for canonical_name in &self.canonical_names {
            if tools.remove(canonical_name).is_some() {
                removed += 1;
            }
        }

        removed
    }
}

impl Drop for ToolRegistrationHandle {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Thread-safe registry for long-lived and call-scoped tools.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    inner: Arc<RegistryInner>,
}

impl ToolRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a long-lived tool.
    pub fn register<T>(&self, tool: T) -> Result<(), ToolError>
    where
        T: Tool + 'static,
    {
        self.register_arc(Arc::new(tool))
    }

    /// Registers a long-lived tool from an existing shared pointer.
    pub fn register_arc(&self, tool: Arc<dyn Tool>) -> Result<(), ToolError> {
        self.insert_batch(vec![tool])?;
        Ok(())
    }

    /// Registers multiple long-lived tools atomically.
    pub fn register_many<T, I>(&self, tools: I) -> Result<(), ToolError>
    where
        T: Tool + 'static,
        I: IntoIterator<Item = T>,
    {
        let tools = tools
            .into_iter()
            .map(|tool| Arc::new(tool) as Arc<dyn Tool>)
            .collect::<Vec<_>>();
        self.insert_batch(tools)?;
        Ok(())
    }

    /// Registers call-scoped tools and returns an RAII cleanup handle.
    pub fn register_call_scoped<T>(&self, tool: T) -> Result<ToolRegistrationHandle, ToolError>
    where
        T: Tool + 'static,
    {
        self.register_many_call_scoped([Arc::new(tool) as Arc<dyn Tool>])
    }

    /// Registers multiple call-scoped tools atomically.
    pub fn register_many_call_scoped<I>(
        &self,
        tools: I,
    ) -> Result<ToolRegistrationHandle, ToolError>
    where
        I: IntoIterator<Item = Arc<dyn Tool>>,
    {
        let canonical_names = self.insert_batch(tools.into_iter().collect())?;

        Ok(ToolRegistrationHandle::new(&self.inner, canonical_names))
    }

    /// Retrieves a registered tool by canonical name.
    #[must_use]
    pub fn get(&self, canonical_name: &str) -> Option<Arc<dyn Tool>> {
        read_tools(&self.inner.tools).get(canonical_name).cloned()
    }

    /// Returns whether a canonical name is registered.
    #[must_use]
    pub fn contains(&self, canonical_name: &str) -> bool {
        read_tools(&self.inner.tools).contains_key(canonical_name)
    }

    /// Lists registered tool descriptors in canonical-name order.
    #[must_use]
    pub fn list(&self) -> Vec<ToolDescriptor> {
        read_tools(&self.inner.tools)
            .values()
            .map(|tool| tool.descriptor())
            .collect()
    }

    /// Removes a tool registration by canonical name.
    pub fn remove(&self, canonical_name: &str) -> Option<Arc<dyn Tool>> {
        write_tools(&self.inner.tools).remove(canonical_name)
    }

    /// Clears all registrations.
    pub fn clear(&self) {
        write_tools(&self.inner.tools).clear();
    }

    /// Executes a registered tool by canonical name.
    pub async fn execute(
        &self,
        call: ToolCall,
        cancel: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        if cancel.is_cancelled() {
            return Err(ToolError::cancelled(
                "tool execution was cancelled before it started",
                Some(call.name),
            ));
        }

        let tool = self.get(&call.name).ok_or_else(|| {
            ToolError::execution_failed("tool is not registered", Some(call.name.clone()))
        })?;

        tool.execute(call, cancel).await
    }

    fn insert_batch(&self, tools: Vec<Arc<dyn Tool>>) -> Result<Vec<String>, ToolError> {
        if tools.is_empty() {
            return Ok(Vec::new());
        }

        let mut pending = Vec::with_capacity(tools.len());
        let mut pending_names = BTreeSet::new();

        for tool in tools {
            let descriptor = tool.descriptor();
            let canonical_name = descriptor.canonical_name.clone();
            let _ = descriptor.canonical_parts()?;

            if !pending_names.insert(canonical_name.clone()) {
                return Err(ToolError::name_collision(canonical_name));
            }

            pending.push((canonical_name, tool));
        }

        let mut registered_names = Vec::with_capacity(pending.len());
        {
            let mut registered_tools = write_tools(&self.inner.tools);

            for (canonical_name, _) in &pending {
                if registered_tools.contains_key(canonical_name) {
                    return Err(ToolError::name_collision(canonical_name.clone()));
                }
            }

            for (canonical_name, tool) in pending {
                registered_names.push(canonical_name.clone());
                registered_tools.insert(canonical_name, tool);
            }
        }

        Ok(registered_names)
    }
}

fn read_tools(lock: &RwLock<ToolMap>) -> std::sync::RwLockReadGuard<'_, ToolMap> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write_tools(lock: &RwLock<ToolMap>) -> std::sync::RwLockWriteGuard<'_, ToolMap> {
    match lock.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use arky_protocol::ToolContent;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{Tool, ToolRegistry};
    use crate::{ToolDescriptor, ToolError, ToolOrigin};
    use async_trait::async_trait;
    use tokio_util::sync::CancellationToken;

    struct TestTool {
        descriptor: ToolDescriptor,
    }

    impl TestTool {
        fn new(canonical_name: &str, display_name: &str) -> Self {
            Self {
                descriptor: ToolDescriptor::new(
                    canonical_name,
                    display_name,
                    format!("{display_name} description"),
                    json!({
                        "type": "object",
                    }),
                    ToolOrigin::Local,
                )
                .expect("descriptor should be valid"),
            }
        }
    }

    #[async_trait]
    impl Tool for TestTool {
        fn descriptor(&self) -> ToolDescriptor {
            self.descriptor.clone()
        }

        async fn execute(
            &self,
            call: ToolCall,
            _cancel: CancellationToken,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::success(
                call.id,
                call.name,
                vec![ToolContent::text("ok")],
            ))
        }
    }

    use arky_protocol::{ToolCall, ToolResult};

    #[test]
    fn tool_registry_should_register_lookup_list_and_remove_tools() {
        let registry = ToolRegistry::new();
        registry
            .register(TestTool::new("mcp/local/read_file", "Read File"))
            .expect("tool should register");

        let listed = registry.list();
        let removed = registry.remove("mcp/local/read_file");

        assert_eq!(listed.len(), 1);
        assert!(removed.is_some());
        assert!(registry.get("mcp/local/read_file").is_none());
    }

    #[test]
    fn tool_registry_should_reject_duplicate_canonical_names() {
        let registry = ToolRegistry::new();
        registry
            .register(TestTool::new("mcp/local/read_file", "Read File"))
            .expect("tool should register");

        let duplicate = registry.register(TestTool::new("mcp/local/read_file", "Read File Again"));

        assert!(matches!(
            duplicate,
            Err(ToolError::NameCollision { canonical_name })
            if canonical_name == "mcp/local/read_file"
        ));
    }

    #[test]
    fn call_scoped_registration_should_unregister_on_cleanup_and_drop() {
        let registry = ToolRegistry::new();
        let handle = registry
            .register_call_scoped(TestTool::new("mcp/local/list_dir", "List Dir"))
            .expect("call-scoped tool should register");

        assert!(registry.contains("mcp/local/list_dir"));
        assert_eq!(handle.cleanup(), 1);
        assert_eq!(handle.cleanup(), 0);
        assert!(!registry.contains("mcp/local/list_dir"));

        let second_handle = registry
            .register_call_scoped(TestTool::new("mcp/local/read_dir", "Read Dir"))
            .expect("call-scoped tool should register");
        drop(second_handle);
        assert!(!registry.contains("mcp/local/read_dir"));
    }
}
