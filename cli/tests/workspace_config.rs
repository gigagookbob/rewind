use std::path::Path;
use std::process::{Command, Output};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rwd"))
        .env("HOME", home)
        .env("RWD_DISABLE_UPDATE_CHECK", "1")
        .env("RWD_DISABLE_NOTIFICATIONS", "1")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn workspace_setting_round_trips_and_rejects_invalid_input_without_saving() {
    let home = std::env::temp_dir().join(format!("rwd-workspace-cli-{}", std::process::id()));
    let dir = home.join(".config/rwd");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        "[llm]\nprovider='anthropic'\nanthropic_api_key='synthetic'\n[output]\npath='/synthetic'\n",
    )
    .unwrap();
    let output = run(
        &home,
        &["config", "anthropic-workspace-id", "wrkspc_test123"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved = std::fs::read_to_string(&path).unwrap();
    assert!(saved.contains("anthropic_workspace_id = \"wrkspc_test123\""));
    let status = run(&home, &["auth", "status"]);
    assert!(String::from_utf8_lossy(&status.stdout).contains("wrkspc_test123"));
    for invalid in [
        "org_test",
        "wrkspc_",
        "wrkspc_test\nInjected: value",
        "sk-ant-secret",
    ] {
        let output = run(&home, &["config", "anthropic-workspace-id", invalid]);
        assert!(!output.status.success());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!error.contains("sk-ant-secret"));
        assert!(!error.contains("Injected:"));
    }
    let output = run(&home, &["config", "anthropic-workspace-id", ""]);
    assert!(output.status.success());
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("anthropic_workspace_id")
    );
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn today_and_worker_report_malformed_and_missing_config_without_silent_exit() {
    let home = std::env::temp_dir().join(format!("rwd-config-err-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    // Missing config in worker mode still writes worker.log and removes worker.lock.
    let worker_missing = run(&home, &["today", "--worker", "--lang", "ko"]);
    assert!(!worker_missing.status.success());
    assert!(!home.join(".rwd/worker.lock").exists());
    let log = std::fs::read_to_string(home.join(".rwd/worker.log")).unwrap();
    assert!(log.contains("No config found"), "{log}");

    // Broken TOML is reported as parse failure rather than missing config.
    let dir = home.join(".config/rwd");
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = dir.join("config.toml");
    std::fs::write(
        &cfg,
        "[llm]\nprovider = 'anthropic'\nanthropic_api_key = 'sk-ant-secret-unclosed\n",
    )
    .unwrap();

    let today_broken = run(&home, &["today", "--lang", "ko"]);
    assert!(!today_broken.status.success());
    let stderr = String::from_utf8_lossy(&today_broken.stderr);
    assert!(stderr.contains("Failed to parse config"), "{stderr}");
    assert!(!stderr.contains("No config found"), "{stderr}");
    assert!(!stderr.contains("sk-ant-secret-unclosed"), "{stderr}");

    let dry_run_broken = run(&home, &["today", "--dry-run"]);
    assert!(dry_run_broken.status.success());
    let dry_stderr = String::from_utf8_lossy(&dry_run_broken.stderr);
    assert!(
        dry_stderr.contains("Failed to parse config"),
        "{dry_stderr}"
    );

    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn config_redactor_toggles_masking_and_lists_supported_keys() {
    let home = std::env::temp_dir().join(format!("rwd-redactor-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let dir = home.join(".config/rwd");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        "[llm]\nprovider='anthropic'\nanthropic_api_key='synthetic'\n[output]\npath='/synthetic'\n",
    )
    .unwrap();

    let off = run(&home, &["config", "redactor", "off"]);
    assert!(
        off.status.success(),
        "{}",
        String::from_utf8_lossy(&off.stderr)
    );
    let saved_off = std::fs::read_to_string(&path).unwrap();
    assert!(saved_off.contains("enabled = false"), "{saved_off}");

    let on = run(&home, &["config", "redactor", "on"]);
    assert!(
        on.status.success(),
        "{}",
        String::from_utf8_lossy(&on.stderr)
    );
    let saved_on = std::fs::read_to_string(&path).unwrap();
    assert!(saved_on.contains("enabled = true"), "{saved_on}");

    let unknown = run(&home, &["config", "unknown-key", "val"]);
    assert!(!unknown.status.success());
    let err = String::from_utf8_lossy(&unknown.stderr);
    assert!(err.contains("redactor"), "{err}");
    assert!(err.contains("lang"), "{err}");

    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn slack_handles_empty_sessions_and_rejects_invalid_lang() {
    let home = std::env::temp_dir().join(format!("rwd-slack-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let dir = home.join(".config/rwd");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("config.toml"),
        "lang = 'ko'\n[llm]\nprovider='anthropic'\nanthropic_api_key='synthetic'\n[output]\npath='/synthetic'\n",
    )
    .unwrap();

    let empty = run(&home, &["slack", "--date", "2099-01-01", "--lang", "ko"]);
    assert!(
        empty.status.success(),
        "empty slack should succeed, stderr: {}",
        String::from_utf8_lossy(&empty.stderr)
    );

    let invalid = run(
        &home,
        &["slack", "--date", "2099-01-01", "--lang", "invalid"],
    );
    assert!(!invalid.status.success());
    let stderr = String::from_utf8_lossy(&invalid.stderr);
    assert!(stderr.contains("Unsupported language"), "{stderr}");

    std::fs::remove_dir_all(home).unwrap();
}
