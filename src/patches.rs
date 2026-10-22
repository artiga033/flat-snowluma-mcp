//! Description selection: official wording first, authored text only for gaps.

use crate::catalog::ActionDoc;

/// Official summaries are returned verbatim. When a summary is absent, reuse
/// an existing official description or return description without paraphrasing.
pub fn summary(action: &ActionDoc) -> Option<&str> {
    if let Some(summary) = action.summary.as_deref().filter(|text| !text.is_empty()) {
        return Some(summary);
    }
    let pointer = match action.name.as_str() {
        "can_send_image" | "can_send_record" => Some("/properties/yes/description"),
        "get_group_album_list" => Some("/description"),
        _ => None,
    };
    if let Some(text) =
        pointer.and_then(|pointer| action.returns_schema.as_ref()?.pointer(pointer)?.as_str())
    {
        return Some(text);
    }
    if let Some(returns) = action.returns.as_deref().filter(|text| !text.is_empty()) {
        return Some(returns);
    }
    // These four actions have neither a summary nor a return description.
    match action.name.as_str() {
        "cancel_group_album_media_like" => Some("取消群相册媒体点赞"),
        "do_group_album_comment" => Some("评论群相册媒体"),
        "set_group_album_media_like" => Some("点赞群相册媒体"),
        "upload_image_to_qun_album" => Some("上传图片到群相册"),
        _ => None,
    }
}

/// No undocumented compatibility fields are currently enabled.
pub fn is_compat_field(_action: &str, _field: &str) -> bool {
    false
}

/// Keep official summary, constraints and aliases verbatim, separated by
/// newlines. Do not add synonyms, rewrite summaries or duplicate limitations.
pub fn description(action: &ActionDoc) -> String {
    let mut parts = vec![summary(action).unwrap_or(&action.name).to_string()];
    parts.extend(action.invariants.iter().cloned());
    if !action.aliases.is_empty() {
        parts.push(action.aliases.join(" "));
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    #[test]
    fn official_wording_is_preserved_for_every_action() {
        let catalog = Catalog::load().unwrap();
        for action in &catalog.actions {
            let description = description(action);
            assert!(!description.is_empty(), "{}", action.name);
            if let Some(official) = &action.summary {
                assert_eq!(summary(action), Some(official.as_str()), "{}", action.name);
            }
            for invariant in &action.invariants {
                assert!(description.contains(invariant), "{}", action.name);
            }
            for alias in &action.aliases {
                assert!(description.contains(alias), "{}", action.name);
            }
        }
    }

    #[test]
    fn missing_summaries_reuse_official_text_before_authored_fallbacks() {
        let catalog = Catalog::load().unwrap();
        let mut authored = Vec::new();
        for action in &catalog.actions {
            let selected = summary(action).expect("every action has a description");
            let official = action.summary.as_deref() == Some(selected)
                || action.returns.as_deref() == Some(selected)
                || action.returns_schema.as_ref().is_some_and(|schema| {
                    schema.pointer("/description").and_then(|v| v.as_str()) == Some(selected)
                        || schema
                            .pointer("/properties/yes/description")
                            .and_then(|v| v.as_str())
                            == Some(selected)
                });
            if !official {
                authored.push(action.name.as_str());
            }
        }
        assert_eq!(
            authored,
            vec![
                "cancel_group_album_media_like",
                "do_group_album_comment",
                "set_group_album_media_like",
                "upload_image_to_qun_album",
            ]
        );
    }
}
