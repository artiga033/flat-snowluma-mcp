//! Argument validation against the documented contract.
//!
//! Cross-field constraints are checked before transport. Unknown fields are
//! reported with the legal field list so
//! `message_count` is not silently ignored. The checks here run before any
//! request reaches SnowLuma.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::catalog::ActionDoc;
use crate::error::ValidationError;
use crate::patches;

/// Validate `params` for `action`, returning a structured error on failure.
pub fn validate(action: &ActionDoc, params: &Map<String, Value>) -> Result<(), ValidationError> {
    check_unknown_fields(action, params)?;
    check_required(action, params)?;
    check_invariants(action, params)?;
    Ok(())
}

/// Reject fields that are neither documented nor a verified compatibility field.
fn check_unknown_fields(
    action: &ActionDoc,
    params: &Map<String, Value>,
) -> Result<(), ValidationError> {
    let known: BTreeSet<&str> = action.params.iter().map(|p| p.name.as_str()).collect();
    let unknown: Vec<String> = params
        .keys()
        .filter(|key| !known.contains(key.as_str()))
        .filter(|key| !patches::is_compat_field(&action.name, key))
        .cloned()
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    let legal = if known.is_empty() {
        "none (this action takes no parameters)".to_string()
    } else {
        known.into_iter().collect::<Vec<_>>().join(", ")
    };
    crate::error::validation::UnknownFieldsSnafu {
        action: action.name.clone(),
        unknown,
        legal,
    }
    .fail()
}

/// Reject a call that omits a required field.
fn check_required(action: &ActionDoc, params: &Map<String, Value>) -> Result<(), ValidationError> {
    for param in &action.params {
        if !param.required {
            continue;
        }
        let present = params
            .get(&param.name)
            .is_some_and(|value| !value.is_null());
        if !present {
            return crate::error::validation::InvariantSnafu {
                action: action.name.clone(),
                reason: format!("missing required field `{}`", param.name),
            }
            .fail();
        }
    }
    Ok(())
}

/// Evaluate the documented cross-field invariants.
fn check_invariants(
    action: &ActionDoc,
    params: &Map<String, Value>,
) -> Result<(), ValidationError> {
    for invariant in &action.invariants {
        if let Some(reason) = evaluate(action, invariant, params) {
            return crate::error::validation::InvariantSnafu {
                action: action.name.clone(),
                reason,
            }
            .fail();
        }
    }
    Ok(())
}

/// Evaluate a single invariant string, returning a failure reason if violated.
///
/// Only the invariant shapes present in the snapshot are handled; anything
/// unrecognized is treated as satisfied so an unseen constraint never blocks a
/// call the server would accept.
fn evaluate(action: &ActionDoc, invariant: &str, params: &Map<String, Value>) -> Option<String> {
    let _ = action;
    let text = invariant.trim();
    let lower = text.to_lowercase();

    // "exactly one of: a | b" — keywords are matched case-insensitively, but the
    // field names keep their original spelling (they may be camelCase).
    if lower.starts_with("exactly one of:")
        && let Some(fields) = parse_field_list(&text["exactly one of:".len()..])
    {
        let present: Vec<&String> = fields.iter().filter(|f| has(params, f)).collect();
        if present.len() != 1 {
            return Some(format!(
                "exactly one of [{}] must be provided, got {}",
                fields.join(", "),
                present.len()
            ));
        }
        return None;
    }

    // "at least one of: a, b, c"
    if lower.starts_with("at least one of:")
        && let Some(fields) = parse_field_list(&text["at least one of:".len()..])
    {
        if !fields.iter().any(|f| has(params, f)) {
            return Some(format!(
                "at least one of [{}] must be provided",
                fields.join(", ")
            ));
        }
        return None;
    }

    // "X is required when Y is Z or W"
    if let Some(reason) = evaluate_required_when(text, params) {
        return Some(reason);
    }

    // "X must be one of 1, 4, 16, 64, 128"
    if let Some(reason) = evaluate_must_be_one_of(text, params) {
        return Some(reason);
    }

    None
}

/// `field must be one of a, b, c`
fn evaluate_must_be_one_of(text: &str, params: &Map<String, Value>) -> Option<String> {
    let marker = " must be one of ";
    let position = text.to_lowercase().find(marker)?;
    let field = text[..position].trim();
    let rest = &text[position + marker.len()..];
    let allowed: Vec<String> = rest
        .split(',')
        .map(|value| value.trim().trim_matches('(').trim_matches(')').to_string())
        .filter(|value| !value.is_empty())
        .collect();
    let value = params.get(field)?;
    let actual = value_to_scalar(value);
    if allowed.iter().any(|candidate| candidate == &actual) {
        return None;
    }
    Some(format!(
        "`{field}` must be one of [{}], got {actual}",
        allowed.join(", ")
    ))
}

/// `field is required when <other> is <v1> or <v2>`
fn evaluate_required_when(text: &str, params: &Map<String, Value>) -> Option<String> {
    let marker = " is required when ";
    let position = text.to_lowercase().find(marker)?;
    let field = text[..position].trim();
    let condition = &text[position + marker.len()..];
    let condition_lower = condition.to_lowercase();
    let is_position = condition_lower.find(" is ")?;
    let condition_field = condition[..is_position].trim();
    let values: Vec<String> = condition[is_position + 4..]
        .split(" or ")
        .map(|value| value.trim().to_string())
        .collect();
    let trigger = params.get(condition_field)?;
    let actual = value_to_scalar(trigger);
    if !values.iter().any(|value| value == &actual) {
        return None;
    }
    if has(params, field) {
        return None;
    }
    Some(format!(
        "`{field}` is required when `{condition_field}` is {actual} (from: {text})"
    ))
}

/// `type must be 1 (regular) or 20 (new members)` — a value-set constraint.
fn parse_field_list(rest: &str) -> Option<Vec<String>> {
    let fields: Vec<String> = rest
        .split('|')
        .flat_map(|part| part.split(','))
        .map(|value| value.trim().trim_matches('`').to_string())
        .filter(|value| !value.is_empty() && is_identifier(value))
        .collect();
    (!fields.is_empty()).then_some(fields)
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn has(params: &Map<String, Value>, field: &str) -> bool {
    params.get(field).is_some_and(|value| !value.is_null())
}

fn value_to_scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use serde_json::json;

    fn params(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn rejects_unknown_field_with_legal_list() {
        let catalog = Catalog::load().unwrap();
        let action = catalog.get("get_group_msg_history").unwrap();
        let error = validate(
            action,
            &params(json!({ "group_id": 1, "message_count": 3 })),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("message_count"), "{message}");
        assert!(message.contains("count"), "{message}");
    }

    #[test]
    fn accepts_valid_history_call() {
        let catalog = Catalog::load().unwrap();
        let action = catalog.get("get_group_msg_history").unwrap();
        assert!(validate(action, &params(json!({ "group_id": 1, "count": 3 }))).is_ok());
    }

    #[test]
    fn missing_required_field_is_reported() {
        let catalog = Catalog::load().unwrap();
        let action = catalog.get("get_group_msg_history").unwrap();
        let error = validate(action, &params(json!({}))).unwrap_err();
        assert!(error.to_string().contains("group_id"));
    }

    #[test]
    fn exactly_one_of_is_enforced() {
        let catalog = Catalog::load().unwrap();
        let action = catalog.get("set_friends_category").unwrap();
        assert!(validate(action, &params(json!({ "uin": 1, "categoryId": 5 }))).is_ok());
        let both = validate(
            action,
            &params(json!({ "uin": 1, "categoryId": 5, "categoryName": "x" })),
        );
        assert!(both.is_err());
        let neither = validate(action, &params(json!({ "uin": 1 })));
        assert!(neither.is_err());
    }

    #[test]
    fn at_least_one_of_is_enforced() {
        let catalog = Catalog::load().unwrap();
        let action = catalog.get("set_group_member_permissions").unwrap();
        assert!(validate(action, &params(json!({ "group_id": 1 }))).is_err());
        assert!(
            validate(
                action,
                &params(json!({ "group_id": 1, "allow_member_create_group": true }))
            )
            .is_ok()
        );
    }

    #[test]
    fn required_when_and_value_set_are_enforced() {
        let catalog = Catalog::load().unwrap();
        let action = catalog.get("send_qzone_msg").unwrap();
        assert!(validate(action, &params(json!({ "content": "hi" }))).is_ok());
        // ugc_right=16 requires target_uins.
        assert!(validate(action, &params(json!({ "content": "hi", "ugc_right": 16 }))).is_err());
        assert!(
            validate(
                action,
                &params(json!({ "content": "hi", "ugc_right": 16, "target_uins": [1] }))
            )
            .is_ok()
        );
        // A value outside the allowed set is rejected.
        assert!(validate(action, &params(json!({ "content": "hi", "ugc_right": 7 }))).is_err());
    }
}
