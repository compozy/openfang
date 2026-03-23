//! Model discovery for the Codex app-server.

use std::sync::Arc;

use arky_provider::ProviderError;
use serde_json::Value;

use crate::thread::RpcClient;

/// One model descriptor returned by `model/list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexModelDescriptor {
    /// Stable model identifier.
    pub id: String,
    /// Optional provider-facing name.
    pub name: Option<String>,
    /// Optional human-friendly display name.
    pub display_name: Option<String>,
    /// Optional canonical model string.
    pub model: Option<String>,
    /// Optional creation timestamp.
    pub created: Option<u64>,
}

/// One parsed `model/list` page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelListPage {
    /// Models contained in this page.
    pub models: Vec<CodexModelDescriptor>,
    /// Cursor for the next page. `Some("")` is normalized away.
    pub next_cursor: Option<String>,
}

/// Service wrapper around the `model/list` RPC.
#[derive(Debug, Clone)]
pub struct CodexModelService<C> {
    rpc: Arc<C>,
}

impl<C> CodexModelService<C>
where
    C: RpcClient + 'static,
{
    /// Creates a model service from an RPC client.
    #[must_use]
    pub const fn new(rpc: Arc<C>) -> Self {
        Self { rpc }
    }

    /// Fetches one page of models.
    pub async fn list_page(&self, cursor: Option<&str>) -> Result<ModelListPage, ProviderError> {
        let params = cursor
            .filter(|cursor| !cursor.is_empty())
            .map(|cursor| serde_json::json!({ "cursor": cursor }));
        let value = self.rpc.request_value("model/list", params).await?;
        parse_model_list_page(&value)
    }

    /// Lists every unique model across all pages.
    pub async fn list_all_models(&self) -> Result<Vec<CodexModelDescriptor>, ProviderError> {
        let mut seen_ids = std::collections::BTreeSet::new();
        let mut seen_cursors = std::collections::BTreeSet::new();
        let mut cursor: Option<String> = None;
        let mut models = Vec::new();

        loop {
            if let Some(current_cursor) = cursor.as_deref()
                && !seen_cursors.insert(current_cursor.to_owned())
            {
                return Err(ProviderError::protocol_violation(
                    format!("model/list returned a repeated cursor: {current_cursor}"),
                    None,
                ));
            }

            let page = self.list_page(cursor.as_deref()).await?;
            for model in page.models {
                if seen_ids.insert(model.id.clone()) {
                    models.push(model);
                }
            }

            match page.next_cursor {
                Some(next_cursor) if !next_cursor.is_empty() => {
                    cursor = Some(next_cursor);
                }
                _ => break,
            }
        }

        Ok(models)
    }
}

fn parse_model_list_page(value: &Value) -> Result<ModelListPage, ProviderError> {
    let object = value.as_object().ok_or_else(|| {
        ProviderError::protocol_violation("model/list returned a non-object result", None)
    })?;
    let models_source = object
        .get("models")
        .and_then(Value::as_array)
        .or_else(|| object.get("data").and_then(Value::as_array))
        .cloned()
        .unwrap_or_default();

    let mut models = Vec::new();
    for entry in models_source {
        let Some(record) = entry.as_object() else {
            continue;
        };

        let id = record
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| record.get("model").and_then(Value::as_str))
            .or_else(|| record.get("name").and_then(Value::as_str))
            .or_else(|| record.get("displayName").and_then(Value::as_str))
            .filter(|value| !value.is_empty());
        let Some(id) = id else {
            continue;
        };

        models.push(CodexModelDescriptor {
            id: id.to_owned(),
            name: record
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            display_name: record
                .get("displayName")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            model: record
                .get("model")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            created: record.get("created").and_then(Value::as_u64),
        });
    }

    let next_cursor = object
        .get("nextCursor")
        .or_else(|| object.get("next_cursor"))
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned);

    Ok(ModelListPage {
        models,
        next_cursor,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arky_provider::ProviderError;
    use async_trait::async_trait;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use tokio::sync::Mutex;

    use super::{CodexModelService, parse_model_list_page};
    use crate::thread::RpcClient;

    #[derive(Debug, Default)]
    struct MockRpcClient {
        calls: Mutex<Vec<(String, Value)>>,
        responses: Mutex<Vec<Value>>,
    }

    #[async_trait]
    impl RpcClient for MockRpcClient {
        async fn request_value(
            &self,
            method: &str,
            params: Option<Value>,
        ) -> Result<Value, ProviderError> {
            self.calls
                .lock()
                .await
                .push((method.to_owned(), params.clone().unwrap_or(Value::Null)));
            Ok(self.responses.lock().await.remove(0))
        }
    }

    #[test]
    fn parse_model_list_page_should_accept_models_and_data_shapes() {
        let page = parse_model_list_page(&json!({
            "data": [
                {
                    "model": "gpt-5",
                    "displayName": "GPT-5",
                    "created": 123,
                },
                {
                    "name": "o4-mini",
                },
                {
                    "ignored": true,
                }
            ],
            "next_cursor": "cursor-2",
        }))
        .expect("page should parse");

        assert_eq!(page.models.len(), 2);
        assert_eq!(page.models[0].id, "gpt-5");
        assert_eq!(page.models[0].display_name.as_deref(), Some("GPT-5"));
        assert_eq!(page.models[1].id, "o4-mini");
        assert_eq!(page.next_cursor.as_deref(), Some("cursor-2"));
    }

    #[tokio::test]
    async fn model_service_should_paginate_and_deduplicate_models() {
        let rpc = Arc::new(MockRpcClient {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(vec![
                json!({
                    "models": [
                        { "id": "gpt-5", "name": "gpt-5" },
                        { "id": "o4-mini", "name": "o4-mini" }
                    ],
                    "nextCursor": "cursor-2",
                }),
                json!({
                    "data": [
                        { "id": "o4-mini", "name": "o4-mini" },
                        { "id": "gpt-4o", "displayName": "GPT-4o" }
                    ],
                    "nextCursor": null,
                }),
            ]),
        });
        let service = CodexModelService::new(rpc.clone());

        let models = service.list_all_models().await.expect("models should list");
        let calls = rpc.calls.lock().await.clone();

        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id, "gpt-5");
        assert_eq!(models[1].id, "o4-mini");
        assert_eq!(models[2].id, "gpt-4o");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "model/list");
        assert_eq!(calls[0].1, Value::Null);
        assert_eq!(calls[1].1["cursor"], "cursor-2");
    }

    #[tokio::test]
    async fn model_service_should_reject_repeated_cursors() {
        let rpc = Arc::new(MockRpcClient {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(vec![
                json!({
                    "models": [],
                    "nextCursor": "cursor-1",
                }),
                json!({
                    "models": [],
                    "nextCursor": "cursor-1",
                }),
            ]),
        });
        let service = CodexModelService::new(rpc);

        let error = service
            .list_all_models()
            .await
            .expect_err("repeated cursors should fail");

        assert!(matches!(error, ProviderError::ProtocolViolation { .. }));
    }
}
