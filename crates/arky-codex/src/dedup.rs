//! Duplicate detection helpers for Codex notifications.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::CodexNotification;

/// Stable fingerprint deduper for one turn stream.
#[derive(Debug, Clone, Default)]
pub struct FingerprintDeduper {
    fingerprints: BTreeSet<String>,
}

impl FingerprintDeduper {
    /// Creates an empty deduper.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when the notification has not been seen before.
    pub fn record(&mut self, notification: &CodexNotification) -> bool {
        self.fingerprints
            .insert(fingerprint_notification(notification))
    }

    /// Returns the number of stored fingerprints.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fingerprints.len()
    }

    /// Returns whether the deduper is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fingerprints.is_empty()
    }
}

/// Builds a stable fingerprint from a raw Codex notification.
#[must_use]
pub fn fingerprint_notification(notification: &CodexNotification) -> String {
    let params = stable_json(&notification.params);
    format!("{}:{params}", notification.method)
}

fn stable_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => {
            serde_json::to_string(value).unwrap_or_else(|_| "\"<invalid>\"".to_owned())
        }
        Value::Array(values) => {
            let items = values.iter().map(stable_json).collect::<Vec<_>>();
            format!("[{}]", items.join(","))
        }
        Value::Object(values) => {
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(right.0));
            let items = entries
                .into_iter()
                .map(|(key, value)| {
                    let encoded_key =
                        serde_json::to_string(key).unwrap_or_else(|_| "\"<invalid>\"".to_owned());
                    format!("{encoded_key}:{}", stable_json(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", items.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{fingerprint_notification, FingerprintDeduper};
    use crate::CodexNotification;

    #[test]
    fn fingerprint_notification_should_be_order_stable() {
        let left = CodexNotification {
            method: "item.started".to_owned(),
            params: json!({
                "b": 2,
                "a": 1,
            }),
        };
        let right = CodexNotification {
            method: "item.started".to_owned(),
            params: json!({
                "a": 1,
                "b": 2,
            }),
        };

        assert_eq!(
            fingerprint_notification(&left),
            fingerprint_notification(&right),
        );
    }

    #[test]
    fn fingerprint_deduper_should_suppress_duplicates() {
        let notification = CodexNotification {
            method: "turn.completed".to_owned(),
            params: json!({ "id": "turn-1" }),
        };
        let mut deduper = FingerprintDeduper::new();

        assert_eq!(deduper.record(&notification), true);
        assert_eq!(deduper.record(&notification), false);
        assert_eq!(deduper.len(), 1);
    }
}
