//! Run the real password reader inside a pseudo-terminal, without touching config or APIs.

#[test]
fn test_password_terminal_input() {
    let status = std::process::Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/password_terminal.py"
        ))
        .arg(std::env::current_exe().expect("test executable"))
        .status()
        .expect("python3 is required for the pseudo-terminal regression test");
    assert!(status.success(), "password terminal regression failed");
}

#[test]
fn test_password_terminal_child() {
    let Ok(case) = std::env::var("RWD_PASSWORD_TERMINAL_TEST") else {
        return;
    };
    let result = super::read_password_with_esc("TEST PASSWORD: ");
    match case.as_str() {
        "paste" | "chunks" => {
            assert_eq!(
                result.expect("input"),
                Some(format!("sk-test-{}", "PasteOnlySecret".repeat(32)))
            );
        }
        "edit" => assert_eq!(result.expect("input"), Some("sk-test-final".into())),
        "empty" => assert_eq!(result.expect("input"), Some(String::new())),
        "escape" => assert!(result.expect("input").is_none()),
        "interrupt" => {
            let error = result.expect_err("Ctrl+C must interrupt input");
            assert_eq!(
                error
                    .downcast_ref::<std::io::Error>()
                    .expect("I/O error")
                    .kind(),
                std::io::ErrorKind::Interrupted
            );
        }
        "no-terminal" => assert!(result.is_err()),
        _ => panic!("unknown terminal test case"),
    }
    eprintln!("PASSWORD RESULT OK");
}
