//! Deterministic serialization and ID derivation.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

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

/// Parses JSON text, rejecting invalid JSON and objects with a repeated key at any depth.
///
/// Keys are compared after decoding, so `"a"` and `"\u0061"` count as the same key.
/// serde_json silently keeps the last of a repeated key, which would hide data.
pub fn parse_json(text: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    check_duplicate_keys(text)?;
    Ok(value)
}

/// Scans text already known to be valid JSON. The stack holds one entry per open
/// container: a key set for an object, `None` for an array.
fn check_duplicate_keys(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    let mut stack: Vec<Option<HashSet<String>>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => stack.push(Some(HashSet::new())),
            b'[' => stack.push(None),
            b'}' | b']' => {
                stack.pop();
            }
            b'"' => {
                let start = i;
                i += 1;
                while bytes[i] != b'"' {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                let mut next = i + 1;
                while next < bytes.len() && bytes[next].is_ascii_whitespace() {
                    next += 1;
                }
                if bytes.get(next) == Some(&b':')
                    && let Some(Some(keys)) = stack.last_mut()
                {
                    let key: String =
                        serde_json::from_str(&text[start..=i]).map_err(|e| e.to_string())?;
                    if !keys.insert(key.clone()) {
                        return Err(format!("duplicate key `{key}`"));
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
}

/// Canonical text with every key in bytewise order.
pub fn canon_value(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

/// Canonical NDJSON line: `resourceType`, `id`, then the other keys in bytewise order.
///
/// Numbers keep their written digits and precision (`1.50` stays `1.50`), but serde_json
/// normalizes exponent notation (`1E2` becomes `1e+2`). The output is stable when parsed
/// and written again.
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
///
/// This does not check that `resourceType` is present: a missing or non-string one is
/// treated as empty. Callers must validate `resourceType` first (Task 4's `prepare` does).
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

    #[test]
    fn parse_json_rejects_repeated_keys_at_any_depth() {
        for text in [
            r#"{"a":1,"a":2}"#,
            r#"{"x":{"a":1,"a":2}}"#,
            r#"{"x":[{"ok":1},{"a":1,"a":2}]}"#,
            r#"{"a":1,"a":2}"#,
            r#"{"a\n":1,"a\u000a":2}"#,
        ] {
            assert!(parse_json(text).is_err(), "accepted {text}");
        }
        let err = parse_json(r#"{"x":{"dup":1,"dup":2}}"#).unwrap_err();
        assert!(err.contains("dup"), "{err}");
    }

    #[test]
    fn parse_json_allows_same_key_in_sibling_objects() {
        assert!(parse_json(r#"{"x":{"a":1},"y":{"a":2},"z":[{"a":1},{"a":2}]}"#).is_ok());
    }

    #[test]
    fn parse_json_does_not_mistake_strings_for_keys() {
        assert!(parse_json(r#"{"a":"b:c","b":"a"}"#).is_ok());
        assert!(parse_json(r#"{"a":"{\"b\":1,\"b\":2}","b":"}{\\","c":"\\\"a\":"}"#).is_ok());
        assert!(parse_json(r#"{"a{":1,"b}":2,"c\"":3,"d\\":4}"#).is_ok());
        assert!(parse_json(r#"["a:","a:"]"#).is_ok());
    }

    #[test]
    fn parse_json_rejects_invalid_json() {
        for text in ["", "{", r#"{"a":}"#, r#"{"a":1,}"#, "nope", r#"{"a":1} x"#] {
            assert!(parse_json(text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn parse_json_keeps_numbers_exact() {
        let v = parse_json(r#"{"resourceType":"O","v":1.50}"#).unwrap();
        assert!(canon_resource(&v).contains(r#""v":1.50"#));
    }

    #[test]
    fn exponents_are_normalized_by_serde_json_and_then_stable() {
        let v = parse(r#"{"resourceType":"O","v":1E2}"#);
        let once = canon_resource(&v);
        assert_eq!(once, r#"{"resourceType":"O","v":1e+2}"#);
        assert_eq!(canon_resource(&parse(&once)), once);
    }

    #[test]
    fn canonical_text_is_a_fixed_point() {
        for input in [
            r#"{"resourceType":"O","t":"quote \" backslash \\ slash \/ tab \t nl \n"}"#,
            r#"{"resourceType":"O","t":"nul \u0000 bell \u0007 unit sep \u001f del \u007f"}"#,
            r#"{"resourceType":"O","t":"café é 日本語 😀 😀"}"#,
            r#"{"resourceType":"O","a":-0,"b":-0.0,"c":0.0,"d":-1.50}"#,
            r#"{"resourceType":"O","a":1e5,"b":1E-5,"c":1.5e+10,"d":2.50E3}"#,
            r#"{"resourceType":"O","n":{"b":[1,{"z":null,"a":[true,false,[]]}],"a":{}}}"#,
            r#"{"z":1,"id":"x","resourceType":"O","é":"é"}"#,
        ] {
            let once = canon_resource(&parse(input));
            let twice = canon_resource(&parse(&once));
            assert_eq!(once, twice, "not stable for {input}");
        }
    }

    #[test]
    fn derive_id_golden_value() {
        // IDs are persistent: they are written into vault files and referenced by other
        // resources. Changing the hash, the truncation or the canonical form breaks
        // existing vaults, so this value must never change.
        let v = parse(
            r#"{"resourceType":"Observation","id":"ignored","status":"final","valueQuantity":{"value":1.50,"unit":"kg"},"meta":{"versionId":"2"}}"#,
        );
        assert_eq!(derive_id(&v), "50cb1183797d2eee6ecbcfaba122cfe9");
    }

    #[test]
    fn strip_volatile_leaves_nested_and_non_object_meta_alone() {
        let mut v = json!({"contained":[{"meta":{"lastUpdated":"x","versionId":"1"}}]});
        let before = v.clone();
        strip_volatile(&mut v);
        assert_eq!(v, before);

        for meta in [json!("text"), json!(5), json!(null), json!(["lastUpdated"])] {
            let mut v = json!({"meta": meta});
            let before = v.clone();
            strip_volatile(&mut v);
            assert_eq!(v, before);
        }
    }
}
