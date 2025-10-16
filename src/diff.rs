use serde_json::{Value};
use std::collections::BTreeMap;

// Flatten string leaves with dot paths
pub fn flatten_string_paths(v: &Value, prefix: Option<&str>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    match v {
        Value::Object(obj) => {
            for (k, val) in obj.iter() {
                let seg = escape_key(k);
                let key = match prefix { Some(p) if !p.is_empty() => format!("{}.{}", p, seg), _ => seg };
                map.extend(flatten_string_paths(val, Some(&key)));
            }
        }
        Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                let key = match prefix { Some(p) => format!("{}[{}]", p, i), None => format!("[{}]", i) };
                map.extend(flatten_string_paths(val, Some(&key)));
            }
        }
        Value::String(s) => {
            if let Some(p) = prefix { map.insert(p.to_string(), s.to_string()); }
        }
        _ => {}
    }
    map
}

fn escape_key(k: &str) -> String { k.replace('.', "\\.") }

// Compute list of (path, english) to fill on target. If overwrite=true, include all string leaves.
pub fn compute_missing_translations(source: &Value, target: &Value, overwrite: bool) -> Vec<(String, String)> {
    let src = flatten_string_paths(source, None);
    let tgt = flatten_string_paths(target, None);
    let mut out = Vec::new();
    for (path, english) in src.into_iter() {
        if overwrite {
            out.push((path, english));
        } else if !tgt.contains_key(&path) || tgt.get(&path).map(|s| s.is_empty()).unwrap_or(true) {
            out.push((path, english));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_only_when_not_overwrite() {
        let source: Value = serde_json::json!({"a": {"b": "hello"}});
        let target: Value = serde_json::json!({"a": {"b": ""}});
        let v = compute_missing_translations(&source, &target, false);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].0, "a.b");
    }

    #[test]
    fn all_when_overwrite() {
        let source: Value = serde_json::json!({"a": {"b": "hello"}});
        let target: Value = serde_json::json!({"a": {"b": "world"}});
        let v = compute_missing_translations(&source, &target, true);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].0, "a.b");
        assert_eq!(v[0].1, "hello");
    }
}



