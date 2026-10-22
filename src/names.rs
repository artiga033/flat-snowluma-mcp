//! Preserve documented action names, including dots. Normalization is used
//! only to check for collisions in clients that normalize names internally.

/// The MCP tool name is the official action name, without renaming.
pub fn tool_name(action: &str) -> String {
    action.to_string()
}

/// A client-style namespaced tool name: `mcp__<server>__<tool>`.
pub fn namespaced(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
}

/// Normalize a name to an ASCII identifier for collision checking.
pub fn normalize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Verify that no two tool names collide once normalized.
///
/// Returns the colliding pairs, if any. Used by a unit test over the embedded
/// catalog so a future snapshot with a new special name fails loudly.
pub fn collisions<'a>(actions: impl IntoIterator<Item = &'a str>) -> Vec<(String, String)> {
    let mut seen: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    let mut found = Vec::new();
    for action in actions {
        let normalized = normalize(&tool_name(action));
        if let Some(previous) = seen.get(&normalized) {
            found.push((previous.clone(), action.to_string()));
        } else {
            seen.insert(normalized, action.to_string());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    #[test]
    fn official_names_are_preserved() {
        assert_eq!(tool_name(".get_word_slices"), ".get_word_slices");
        assert_eq!(tool_name("get_group_msg_history"), "get_group_msg_history");
    }

    #[test]
    fn namespaced_name_uses_server_and_tool() {
        assert_eq!(
            namespaced("qq", "get_group_msg_history"),
            "mcp__qq__get_group_msg_history"
        );
    }

    #[test]
    fn catalog_has_no_collisions() {
        let catalog = Catalog::load().expect("embedded catalog loads");
        let names: Vec<&str> = catalog.actions.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(collisions(names), Vec::<(String, String)>::new());
    }

    #[test]
    fn catalog_tool_names_follow_mcp_name_characters() {
        let catalog = Catalog::load().expect("embedded catalog loads");
        for action in &catalog.actions {
            let name = tool_name(&action.name);
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')),
                "tool name `{name}` has invalid MCP name characters"
            );
        }
    }

    #[test]
    fn every_alias_resolves_to_its_action() {
        // Aliases resolve to the canonical action without a second tool.
        let catalog = Catalog::load().expect("embedded catalog loads");
        for action in &catalog.actions {
            for alias in &action.aliases {
                let resolved = catalog.get(alias).expect("alias resolves");
                assert_eq!(resolved.name, action.name, "alias `{alias}`");
            }
        }
    }

    #[test]
    fn tool_names_are_unique_and_one_to_one() {
        // Canonical actions map one-to-one onto tools.
        let catalog = Catalog::load().expect("embedded catalog loads");
        let mut names: Vec<String> = catalog.actions.iter().map(|a| tool_name(&a.name)).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "tool names must be unique");
    }
}
