//! Public contracts for the development and release image.

const CONTAINERFILE: &str = include_str!("../../images/rust-node/Containerfile");

#[test]
fn installs_the_codex_linux_sandbox_prerequisite() {
    assert!(
        CONTAINERFILE
            .lines()
            .any(|line| line.trim() == "bubblewrap \\"),
        "the image must install Debian's bubblewrap package"
    );
}
