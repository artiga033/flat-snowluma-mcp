//! Input schemas are copied from official documentation without token edits.

use crate::catalog::{ActionDoc, Param};
use serde_json::{Map, Value, json};

pub fn input_schema(action: &ActionDoc) -> Value {
    if let Some(Value::Object(schema)) = &action.input_schema {
        return Value::Object(schema.clone());
    }
    let mut properties = Map::new();
    let mut required = Vec::new();
    for param in &action.params {
        properties.insert(param.name.clone(), param_schema(param));
        if param.required {
            required.push(Value::String(param.name.clone()));
        }
    }
    let mut schema =
        json!({"type": "object", "properties": properties, "additionalProperties": true});
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

fn param_schema(param: &Param) -> Value {
    let mut schema = param
        .schema
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if !schema.contains_key("type") {
        let ty = match param.ty.as_deref() {
            Some("uint" | "int" | "messageId") => Some(json!("integer")),
            Some("bool") => Some(json!("boolean")),
            Some("string" | "enum") => Some(json!("string")),
            // Preserve unspecified types instead of guessing message/array shapes.
            _ => None,
        };
        if let Some(ty) = ty {
            schema.insert("type".into(), ty);
        }
    }
    if let Some(default) = &param.default {
        schema.entry("default").or_insert(default.clone());
    }
    if let Some(desc) = &param.desc {
        schema.entry("description").or_insert(json!(desc));
    }
    if let Some(values) = &param.values {
        schema.entry("enum").or_insert(json!(values));
    }
    if let Some(role) = &param.role {
        schema.entry("x-role").or_insert(json!(role));
    }
    Value::Object(schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    #[test]
    fn all_documented_schemas_and_parameters_are_preserved_exactly() {
        let catalog = Catalog::load().unwrap();
        for action in &catalog.actions {
            let schema = input_schema(action);
            assert_eq!(
                Some(&schema),
                action.input_schema.as_ref(),
                "{}",
                action.name
            );
            let props = schema["properties"].as_object().unwrap();
            assert_eq!(props.len(), action.params.len(), "{}", action.name);
            for param in &action.params {
                assert!(
                    props.contains_key(&param.name),
                    "{} {}",
                    action.name,
                    param.name
                );
                if let Some(desc) = &param.desc {
                    assert_eq!(
                        props[&param.name]["description"],
                        json!(desc),
                        "{} {}",
                        action.name,
                        param.name
                    );
                }
                let required = schema["required"]
                    .as_array()
                    .is_some_and(|fields| fields.contains(&json!(param.name)));
                assert_eq!(required, param.required, "{} {}", action.name, param.name);
            }
        }
    }
}
