use std::process::Command;

#[test]
fn reports_that_environment_management_is_not_implemented() {
    let output = Command::new(env!("CARGO_BIN_EXE_cloister"))
        .output()
        .expect("Cloister binary should start");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        format!(
            "cloister {}: environment management is not implemented yet\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    assert!(output.stderr.is_empty());
}
