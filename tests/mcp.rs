use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn new_vault() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let vault = dir.path().join("data");
    let o = Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["init", vault.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    (dir, vault)
}

/// Runs one MCP session and returns each response by request id. Every stdout line must be JSON.
/// Responses are read as they arrive, and stdin closes once every request has been answered.
fn session(vault: &Path, calls: &[(&str, Value)]) -> HashMap<i64, Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_phv"))
        .args(["--vault", vault.to_str().unwrap(), "mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    let mut send = |message: Value| writeln!(stdin, "{message}").unwrap();
    send(
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}),
    );
    send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    for (i, (name, arguments)) in calls.iter().enumerate() {
        send(
            json!({"jsonrpc":"2.0","id":10 + i,"method":"tools/call","params":{"name":name,"arguments":arguments}}),
        );
    }

    // Requests are handled concurrently, so wait for every response before closing stdin.
    let mut expected: Vec<i64> = vec![1, 2];
    expected.extend((0..calls.len()).map(|i| 10 + i as i64));
    let mut responses = HashMap::new();
    let parse = |line: &str| {
        serde_json::from_str::<Value>(line)
            .unwrap_or_else(|e| panic!("stdout is not protocol JSON: {line}: {e}"))
    };
    while !expected.iter().all(|id| responses.contains_key(id)) {
        let line = rx
            .recv_timeout(Duration::from_secs(60))
            .expect("the server answered every request");
        let message = parse(&line);
        if let Some(id) = message["id"].as_i64() {
            responses.insert(id, message);
        }
    }
    drop(stdin);
    // Anything still on stdout after the last response must also be protocol JSON.
    for line in rx.iter() {
        let message = parse(&line);
        if let Some(id) = message["id"].as_i64() {
            responses.insert(id, message);
        }
    }
    reader.join().unwrap();
    child.wait().unwrap();
    responses
}

fn one(vault: &Path, name: &str, arguments: Value) -> Value {
    session(vault, &[(name, arguments)])
        .remove(&10)
        .expect("a response")["result"]
        .clone()
}

fn text(result: &Value) -> String {
    result["content"][0]["text"].as_str().unwrap().to_string()
}

fn add_patient(vault: &Path) -> String {
    let result = one(
        vault,
        "add",
        json!({"resource_json": r#"{"resourceType":"Patient","birthDate":"1980-01-01"}"#}),
    );
    assert_eq!(result["isError"], false, "{result}");
    let report: Value = serde_json::from_str(&text(&result)).unwrap();
    report["created"][0].as_str().unwrap().to_string()
}

fn lab(patient: &str) -> String {
    format!(
        r#"{{"resourceType":"Observation","status":"final","code":{{"text":"Creatinine"}},"subject":{{"reference":"{patient}"}},"valueQuantity":{{"value":1.50,"unit":"mg/dL"}}}}"#
    )
}

#[test]
fn it_lists_the_tools_and_offers_no_removal() {
    let (_d, v) = new_vault();
    let tools = session(&v, &[]).remove(&2).unwrap();
    let mut names: Vec<String> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "add",
            "check",
            "get",
            "list",
            "replace",
            "source_add",
            "validate",
            "vault_info"
        ]
    );
}

#[test]
fn vault_info_names_the_person_so_a_session_can_confirm_the_vault() {
    let (_d, v) = new_vault();
    let patient = add_patient(&v);
    let info: Value = serde_json::from_str(&text(&one(&v, "vault_info", json!({})))).unwrap();
    assert_eq!(info["patient"], patient);
    assert_eq!(info["path"], v.to_str().unwrap());
}

#[test]
fn a_document_is_uploaded_by_path_and_linked_to_its_records() {
    let (d, v) = new_vault();
    let patient = add_patient(&v);
    let file = d.path().join("lab.pdf");
    std::fs::write(&file, b"%PDF synthetic").unwrap();

    let stored: Value = serde_json::from_str(&text(&one(
        &v,
        "source_add",
        json!({"path": file.to_str().unwrap()}),
    )))
    .unwrap();
    let sha = stored["sha256"].as_str().unwrap().to_string();
    assert_eq!(
        std::fs::read(v.join(format!("sources/{sha}.pdf"))).unwrap(),
        b"%PDF synthetic"
    );

    let result = one(
        &v,
        "add",
        json!({"resource_json": lab(&patient), "source": sha, "agent": "phv-add-lab"}),
    );
    assert_eq!(result["isError"], false, "{result}");
    assert!(
        std::fs::read_to_string(v.join("records/Provenance.ndjson"))
            .unwrap()
            .contains("phv-add-lab")
    );
}

#[test]
fn decimals_stay_exact_through_the_tool() {
    let (_d, v) = new_vault();
    let patient = add_patient(&v);
    assert_eq!(
        one(&v, "add", json!({"resource_json": lab(&patient)}))["isError"],
        false
    );
    let listed = text(&one(
        &v,
        "list",
        json!({"resource_type": "Observation", "where": "status = 'final'"}),
    ));
    assert!(listed.contains(r#""value":1.50"#), "{listed}");
}

#[test]
fn a_filter_with_no_answers_returns_a_warning_the_model_can_read() {
    let (_d, v) = new_vault();
    add_patient(&v);
    let result = one(
        &v,
        "list",
        json!({"resource_type": "Patient", "where": "brithDate = @1980-01-01"}),
    );
    assert_eq!(result["isError"], false, "{result}");
    assert!(
        result["content"][1]["text"]
            .as_str()
            .unwrap()
            .contains("no true or false answer"),
        "{result}"
    );
}

#[test]
fn problems_come_back_as_tool_errors_and_write_nothing() {
    let (d, v) = new_vault();
    let patient = add_patient(&v);

    let invalid = one(
        &v,
        "add",
        json!({"resource_json": r#"{"resourceType":"Observation"}"#}),
    );
    assert_eq!(invalid["isError"], true);
    assert!(text(&invalid).contains("status"), "{invalid}");

    let broken = one(&v, "add", json!({"resource_json": lab("Patient/none")}));
    assert_eq!(broken["isError"], true);
    assert!(text(&broken).contains("PHV-REF"), "{broken}");

    let duplicate = one(
        &v,
        "add",
        json!({"resource_json": r#"{"resourceType":"Patient","resourceType":"Observation"}"#}),
    );
    assert_eq!(duplicate["isError"], true);

    let missing = one(
        &v,
        "source_add",
        json!({"path": d.path().join("nope.pdf").to_str().unwrap()}),
    );
    assert_eq!(missing["isError"], true);

    assert!(!v.join("records/Observation.ndjson").exists());
    assert_eq!(
        one(&v, "get", json!({"reference": "Observation/nope"}))["isError"],
        true
    );
    let _ = patient;
}

#[test]
fn validate_writes_nothing_and_replace_keeps_the_id() {
    let (_d, v) = new_vault();
    let patient = add_patient(&v);
    let issues = one(&v, "validate", json!({"resource_json": lab(&patient)}));
    assert_eq!(issues["isError"], false, "{issues}");
    assert!(!v.join("records/Observation.ndjson").exists());

    let id = patient.split_once('/').unwrap().1;
    let replaced = one(
        &v,
        "replace",
        json!({"reference": patient, "resource_json": r#"{"resourceType":"Patient","birthDate":"1980-01-02"}"#}),
    );
    assert_eq!(replaced["isError"], false, "{replaced}");
    assert!(
        text(&one(
            &v,
            "get",
            json!({"reference": format!("Patient/{id}")})
        ))
        .contains("1980-01-02")
    );
}

#[test]
fn check_reports_a_healthy_vault() {
    let (_d, v) = new_vault();
    add_patient(&v);
    let result = one(&v, "check", json!({}));
    assert_eq!(result["isError"], false, "{result}");
}
