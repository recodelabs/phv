use std::process::Command;

#[test]
fn no_http_clients_or_sqlite_in_the_build() {
    let out = Command::new(env!("CARGO"))
        .args([
            "tree",
            "--locked",
            "--target",
            "all",
            "-e",
            "normal,build",
            "--prefix",
            "none",
        ])
        .output()
        .expect("cargo tree runs");
    assert!(out.status.success());
    let tree = String::from_utf8(out.stdout).unwrap();
    assert!(
        tree.lines().any(|l| l.starts_with("octofhir-fhirschema ")),
        "tree looks wrong: {tree}"
    );
    for banned in ["reqwest ", "ureq ", "rusqlite ", "libsqlite3-sys ", "sqlx "] {
        let found = tree.lines().any(|line| line.starts_with(banned));
        assert!(!found, "{banned}must not be in the dependency tree");
    }
}
