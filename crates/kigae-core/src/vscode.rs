//! Edit VS Code's `settings.json` (JSON with comments) in place, keeping comments and layout.

use std::collections::BTreeMap;

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::{CstInputValue, CstRootNode};

/// Set `set`'s keys and delete `remove`'s, touching nothing else. Keys already holding the
/// wanted value are left exactly as written.
pub fn edit(
    text: &str,
    set: &BTreeMap<String, toml::Value>,
    remove: &[String],
) -> Result<String, String> {
    let opts = ParseOptions::default();
    let current: serde_json::Map<String, serde_json::Value> = if text.trim().is_empty() {
        Default::default()
    } else {
        jsonc_parser::parse_to_serde_value(text, &opts).map_err(|e| e.to_string())?
    };
    let root = CstRootNode::parse(text, &opts).map_err(|e| e.to_string())?;
    let obj = root.object_value_or_set();
    for key in remove {
        if !set.contains_key(key)
            && let Some(prop) = obj.get(key)
        {
            prop.remove();
        }
    }
    for (key, value) in set {
        let want = serde_json::to_value(value).map_err(|e| e.to_string())?;
        if current.get(key) == Some(&want) {
            continue;
        }
        match obj.get(key) {
            Some(prop) => prop.set_value(input(value)),
            None => {
                obj.append(key, input(value));
            }
        }
    }
    Ok(root.to_string())
}

fn input(v: &toml::Value) -> CstInputValue {
    match v {
        toml::Value::String(s) => CstInputValue::String(s.clone()),
        toml::Value::Integer(i) => CstInputValue::Number(i.to_string()),
        toml::Value::Float(f) => CstInputValue::Number(f.to_string()),
        toml::Value::Boolean(b) => CstInputValue::Bool(*b),
        toml::Value::Datetime(d) => CstInputValue::String(d.to_string()),
        toml::Value::Array(a) => CstInputValue::Array(a.iter().map(input).collect()),
        toml::Value::Table(t) => {
            CstInputValue::Object(t.iter().map(|(k, v)| (k.clone(), input(v))).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: &str = r#"{
  "workbench.colorTheme": "Kanagawa",
  // keep me
  "window.autoDetectColorScheme": true,
  "editor.fontSize": 14
}
"#;

    fn map(toml_text: &str) -> BTreeMap<String, toml::Value> {
        toml::from_str(toml_text).unwrap()
    }

    #[test]
    fn sets_removes_and_keeps_comments() {
        let out = edit(
            SETTINGS,
            &map(r#""workbench.colorTheme" = "Catppuccin Mocha""#),
            &["window.autoDetectColorScheme".into()],
        )
        .unwrap();
        assert!(out.contains(r#""workbench.colorTheme": "Catppuccin Mocha""#), "{out}");
        assert!(!out.contains("autoDetectColorScheme"), "{out}");
        assert!(out.contains("// keep me") && out.contains(r#""editor.fontSize": 14"#), "{out}");
    }

    #[test]
    fn appends_nested_objects() {
        let out = edit(
            SETTINGS,
            &map(r##""workbench.colorCustomizations" = { "[X]" = { "titleBar.border" = "#000" } }"##),
            &[],
        )
        .unwrap();
        let v: serde_json::Value = jsonc_parser::parse_to_serde_value(&out, &Default::default()).unwrap();
        assert_eq!(v["workbench.colorCustomizations"]["[X]"]["titleBar.border"], "#000");
    }

    #[test]
    fn nested_keys_keep_the_manifest_order() {
        let out = edit("{}", &map(r##""c" = { "zeta" = 1, "alpha" = 2 }"##), &[]).unwrap();
        assert!(out.find("zeta").unwrap() < out.find("alpha").unwrap(), "{out}");
    }

    #[test]
    fn unchanged_values_leave_text_alone() {
        let out = edit(SETTINGS, &map(r#""workbench.colorTheme" = "Kanagawa""#), &[]).unwrap();
        assert_eq!(out, SETTINGS);
    }

    #[test]
    fn a_key_both_set_and_owned_is_not_removed() {
        let out = edit(
            SETTINGS,
            &map(r#""editor.fontSize" = 15"#),
            &["editor.fontSize".into()],
        )
        .unwrap();
        assert!(out.contains(r#""editor.fontSize": 15"#), "{out}");
    }
}
