use std::path::Path;
use std::process::{Command, Output};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rwd"))
        .env("HOME", home)
        .env("RWD_DISABLE_UPDATE_CHECK", "1")
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
