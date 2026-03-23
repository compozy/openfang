//! Integration tests for registry execution and concurrency behavior.

use std::sync::Arc;

use arky_tools::{
    Tool, ToolCall, ToolContent, ToolDescriptor, ToolError, ToolOrigin, ToolRegistry, ToolResult,
};
use async_trait::async_trait;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::{sync::Barrier, task::JoinSet};
use tokio_util::sync::CancellationToken;

struct EchoTool {
    descriptor: ToolDescriptor,
}

impl EchoTool {
    fn new(canonical_name: impl Into<String>) -> Self {
        let canonical_name = canonical_name.into();
        Self {
            descriptor: ToolDescriptor::new(
                canonical_name,
                "Echo Tool",
                "Echoes JSON input back to the caller.",
                json!({
                    "type": "object",
                    "properties": {
                        "value": {
                            "type": "string",
                        },
                    },
                    "required": ["value"],
                }),
                ToolOrigin::Local,
            )
            .expect("descriptor should be valid"),
        }
    }
}

#[async_trait]
impl Tool for EchoTool {
    fn descriptor(&self) -> ToolDescriptor {
        self.descriptor.clone()
    }

    async fn execute(
        &self,
        call: ToolCall,
        _cancel: CancellationToken,
    ) -> Result<ToolResult, ToolError> {
        let value = call
            .input
            .get("value")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                ToolError::invalid_args(
                    "missing value",
                    Some(json!({
                        "input": call.input,
                    })),
                )
            })?;

        Ok(ToolResult::success(
            call.id,
            call.name,
            vec![ToolContent::text(value)],
        ))
    }
}

const fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn tool_trait_objects_should_be_send_and_sync() {
    assert_send_sync::<Arc<dyn Tool>>();
}

#[tokio::test]
async fn mock_tool_should_execute_through_the_registry() {
    let registry = ToolRegistry::new();
    registry
        .register(EchoTool::new("mcp/local/echo"))
        .expect("tool should register");

    let result = registry
        .execute(
            ToolCall::new(
                "call-1",
                "mcp/local/echo",
                json!({
                    "value": "hello",
                }),
            ),
            CancellationToken::new(),
        )
        .await
        .expect("tool should execute");

    let expected =
        ToolResult::success("call-1", "mcp/local/echo", vec![ToolContent::text("hello")]);

    assert_eq!(result, expected);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_registry_access_should_be_thread_safe() {
    let registry = Arc::new(ToolRegistry::new());
    let barrier = Arc::new(Barrier::new(9));
    let mut tasks = JoinSet::new();

    for index in 0..8 {
        let registry = Arc::clone(&registry);
        let barrier = Arc::clone(&barrier);

        tasks.spawn(async move {
            let canonical_name = format!("mcp/concurrent/echo-{index}");
            let call_id = format!("call-{index}");
            let expected_value = format!("value-{index}");
            barrier.wait().await;

            let handle = registry
                .register_call_scoped(EchoTool::new(&canonical_name))
                .expect("call-scoped tool should register");

            assert!(registry.contains(&canonical_name));
            let has_descriptor = registry
                .list()
                .into_iter()
                .map(|descriptor| descriptor.canonical_name)
                .any(|value| value == canonical_name);
            assert!(has_descriptor);

            let result = registry
                .execute(
                    ToolCall::new(
                        &call_id,
                        &canonical_name,
                        json!({
                            "value": expected_value,
                        }),
                    ),
                    CancellationToken::new(),
                )
                .await
                .expect("tool should execute");

            assert_eq!(result.name, canonical_name);
            assert_eq!(handle.cleanup(), 1);
        });
    }

    barrier.wait().await;

    while let Some(joined) = tasks.join_next().await {
        joined.expect("task should join successfully");
    }

    assert!(registry.list().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_duplicate_registration_should_surface_a_name_collision() {
    let registry = Arc::new(ToolRegistry::new());
    let barrier = Arc::new(Barrier::new(3));
    let canonical_name = "mcp/concurrent/shared".to_owned();
    let mut tasks = JoinSet::new();

    for _ in 0..2 {
        let registry = Arc::clone(&registry);
        let barrier = Arc::clone(&barrier);
        let canonical_name = canonical_name.clone();

        tasks.spawn(async move {
            barrier.wait().await;
            registry.register(EchoTool::new(canonical_name))
        });
    }

    barrier.wait().await;

    let mut successes = 0usize;
    let mut collisions = 0usize;
    while let Some(joined) = tasks.join_next().await {
        match joined.expect("task should join successfully") {
            Ok(()) => successes += 1,
            Err(ToolError::NameCollision { canonical_name }) => {
                collisions += 1;
                assert_eq!(canonical_name, "mcp/concurrent/shared");
            }
            Err(error) => panic!("unexpected registration outcome: {error}"),
        }
    }

    assert_eq!(successes, 1);
    assert_eq!(collisions, 1);
}
