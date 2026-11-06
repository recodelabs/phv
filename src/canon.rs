//! Deterministic serialization and ID derivation.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn write_string(s: &str, out: &mut String) {
    out.push_str(&serde_json::to_string(s).expect("strings serialize"));
}

fn write_object(map: &Map<String, Value>, keys: &[&String], out: &mut String) {
    out.push('{');
    for (i, key) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_string(key, out);
        out.push(':');
        write_value(&map[key.as_str()], out);
    }
    out.push('}');
}

fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            write_object(map, &keys, out);
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&serde_json::to_string(other).expect("scalars serialize")),
    }
}

/// Canonical text with every key in bytewise order.
pub fn canon_value(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

/// Canonical NDJSON line: `resourceType`, `id`, then the other keys in bytewise order.
pub fn canon_resource(resource: &Value) -> String {
    let Some(map) = resource.as_object() else {
        return canon_value(resource);
    };
    let mut keys: Vec<&String> = map
        .keys()
        .filter(|k| *k != "resourceType" && *k != "id")
        .collect();
    keys.sort();
    let mut ordered: Vec<&String> = Vec::with_capacity(map.len());
    for first in ["resourceType", "id"] {
        if let Some((key, _)) = map.get_key_value(first) {
            ordered.push(key);
        }
    }
    ordered.extend(keys);
    let mut out = String::new();
    write_object(map, &ordered, &mut out);
    out
}

/// Removes `meta.lastUpdated` and `meta.versionId`, and `meta` itself if that empties it.
pub fn strip_volatile(resource: &mut Value) {
    let Some(map) = resource.as_object_mut() else {
        return;
    };
    let empty = match map.get_mut("meta").and_then(Value::as_object_mut) {
        Some(meta) => {
            meta.remove("lastUpdated");
            meta.remove("versionId");
            meta.is_empty()
        }
        None => false,
    };
    if empty {
        map.remove("meta");
    }
}

/// First 32 hex characters of the SHA-256 of the type and canonical content, without `id` or volatile `meta`.
pub fn derive_id(resource: &Value) -> String {
    let mut copy = resource.clone();
    strip_volatile(&mut copy);
    if let Some(map) = copy.as_object_mut() {
        map.remove("id");
    }
    let resource_type = copy
        .get("resourceType")
        .and_then(Value::as_str)
        .unwrap_or("");
    let digest = sha256_hex(format!("{resource_type}\n{}", canon_value(&copy)).as_bytes());
    digest[..32].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn resource_type_and_id_come_first_then_bytewise() {
        let v = parse(
            r#"{"status":"final","id":"a","code":{"text":"x","coding":[]},"resourceType":"Observation"}"#,
        );
        assert_eq!(
            canon_resource(&v),
            r#"{"resourceType":"Observation","id":"a","code":{"coding":[],"text":"x"},"status":"final"}"#
        );
    }

    #[test]
    fn nested_objects_ignore_the_top_level_order() {
        let v = parse(r#"{"resourceType":"Bundle","z":{"resourceType":"Patient","id":"p","a":1}}"#);
        assert_eq!(
            canon_resource(&v),
            r#"{"resourceType":"Bundle","z":{"a":1,"id":"p","resourceType":"Patient"}}"#
        );
    }

    #[test]
    fn numbers_keep_their_written_precision() {
        for lit in [
            "1.50",
            "0.10",
            "100.0",
            "7",
            "0.000012300",
            "12345678901234567890.123456789",
        ] {
            let v = parse(&format!(
                r#"{{"resourceType":"Observation","value":{lit}}}"#
            ));
            assert!(
                canon_resource(&v).contains(&format!(r#""value":{lit}}}"#)),
                "lost {lit}"
            );
        }
    }

    #[test]
    fn canonical_text_round_trips_unchanged() {
        let line = r#"{"resourceType":"Observation","id":"a","value":1.50}"#;
        assert_eq!(canon_resource(&parse(line)), line);
    }

    #[test]
    fn strip_volatile_removes_only_volatile_meta() {
        let mut v = json!({"meta":{"lastUpdated":"2026-01-01","versionId":"3","profile":["p"]}});
        strip_volatile(&mut v);
        assert_eq!(v, json!({"meta":{"profile":["p"]}}));

        let mut v = json!({"meta":{"lastUpdated":"2026-01-01"}});
        strip_volatile(&mut v);
        assert_eq!(v, json!({}));
    }

    #[test]
    fn derive_id_ignores_id_order_and_volatile_meta() {
        let a = parse(
            r#"{"resourceType":"Observation","id":"x","status":"final","meta":{"lastUpdated":"2026-01-01"}}"#,
        );
        let b = parse(r#"{"status":"final","resourceType":"Observation"}"#);
        assert_eq!(derive_id(&a), derive_id(&b));
        assert_eq!(derive_id(&a).len(), 32);
    }

    #[test]
    fn derive_id_depends_on_type_and_content() {
        let a = parse(r#"{"resourceType":"Observation","status":"final"}"#);
        let b = parse(r#"{"resourceType":"Observation","status":"amended"}"#);
        let c = parse(r#"{"resourceType":"Condition","status":"final"}"#);
        assert_ne!(derive_id(&a), derive_id(&b));
        assert_ne!(derive_id(&a), derive_id(&c));
    }
}
