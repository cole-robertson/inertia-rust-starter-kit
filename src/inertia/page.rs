//! The Inertia page object (protocol keys are camelCase).

use serde::Serialize;
use serde_json::{Map, Value};

use super::resolver::Metadata;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub component: String,
    pub props: Map<String, Value>,
    /// Original path + query of the request.
    pub url: String,
    pub version: String,
    pub encrypt_history: bool,
    pub clear_history: bool,
    /// `{notice, alert}` present keys only; omitted when empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flash: Option<Map<String, Value>>,
    /// Top-level keys that came from shared props; omitted when none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_props: Option<Vec<String>>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub preserve_fragment: bool,
    #[serde(flatten)]
    pub metadata: Metadata,
}

impl Page {
    /// The page as a JSON string.
    ///
    /// # Panics
    /// Never in practice: every field is plain JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("page serializes")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serializes_protocol_keys_and_omits_empty_ones() {
        let mut metadata = Metadata::default();
        metadata.merge_props.push("posts".into());
        let page = Page {
            component: "Home".into(),
            props: Map::new(),
            url: "/?a=1".into(),
            version: "v".into(),
            encrypt_history: false,
            clear_history: true,
            flash: None,
            shared_props: Some(vec!["auth".into()]),
            preserve_fragment: false,
            metadata,
        };
        assert_eq!(
            serde_json::to_value(&page).unwrap(),
            json!({"component": "Home", "props": {}, "url": "/?a=1", "version": "v",
                   "encryptHistory": false, "clearHistory": true,
                   "sharedProps": ["auth"], "mergeProps": ["posts"]})
        );
    }
}
