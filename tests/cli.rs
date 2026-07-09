use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

fn write_message(dir: &tempfile::TempDir, name: &str, contents: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn top_level_options_do_not_apply_to_subcommands() {
    let dir = tempdir().unwrap();
    let file = write_message(
        &dir,
        "message.eml",
        b"Message-ID: <m@example.com>\r\nSubject: Test\r\n\r\nBody.\r\n",
    );

    Command::cargo_bin("email-cli")
        .unwrap()
        .arg("--quotes")
        .arg("drop")
        .arg("thread")
        .arg(file)
        .assert()
        .failure();
}

#[test]
fn no_args_reads_piped_stdin() {
    let output = Command::cargo_bin("email-cli")
        .unwrap()
        .write_stdin("Message-ID: <stdin@example.com>\r\nSubject: Piped\r\n\r\nBody.\r\n")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["source"]["path"], "-");
    assert_eq!(json["message"]["message_id"], "stdin@example.com");
}

#[test]
fn messages_json_is_schema_envelope_with_diagnostics() {
    let dir = tempdir().unwrap();
    let good = write_message(
        &dir,
        "good.eml",
        b"Message-ID: <good@example.com>\r\nSubject: Good\r\n\r\nBody.\r\n",
    );
    let bad = write_message(&dir, "bad.eml", b"");

    let output = Command::cargo_bin("email-cli")
        .unwrap()
        .arg("messages")
        .arg(good)
        .arg(bad)
        .arg("--format")
        .arg("json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["schema_version"], "1.1");
    assert_eq!(json["messages"].as_array().unwrap().len(), 1);
    assert_eq!(json["diagnostics"][0]["code"], "PARSE_FAILED");
}

#[test]
fn thread_json_continues_after_bad_file() {
    let dir = tempdir().unwrap();
    let good = write_message(
        &dir,
        "good.eml",
        b"Message-ID: <good@example.com>\r\nSubject: Good\r\n\r\nBody.\r\n",
    );
    let bad = write_message(&dir, "bad.eml", b"");

    let output = Command::cargo_bin("email-cli")
        .unwrap()
        .arg("thread")
        .arg(good)
        .arg(bad)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["schema_version"], "1.1");
    assert_eq!(json["threads"].as_array().unwrap().len(), 1);
    assert_eq!(json["diagnostics"][0]["code"], "PARSE_FAILED");
}

#[test]
fn messages_ndjson_uses_record_discriminators() {
    let dir = tempdir().unwrap();
    let good = write_message(
        &dir,
        "good.eml",
        b"Message-ID: <good@example.com>\r\nSubject: Good\r\n\r\nBody.\r\n",
    );
    let bad = write_message(&dir, "bad.eml", b"");

    let output = Command::cargo_bin("email-cli")
        .unwrap()
        .arg("messages")
        .arg(good)
        .arg(bad)
        .arg("--format")
        .arg("ndjson")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let records = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records[0]["record_type"], "message");
    assert_eq!(records[1]["record_type"], "diagnostic");
}

#[test]
fn messages_exits_nonzero_when_every_input_fails() {
    let dir = tempdir().unwrap();
    let bad = write_message(&dir, "bad.eml", b"");

    Command::cargo_bin("email-cli")
        .unwrap()
        .arg("messages")
        .arg(bad)
        .assert()
        .failure();
}
