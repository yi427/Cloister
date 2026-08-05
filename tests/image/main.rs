//! Public contracts for the development and release image.

const CONTAINERFILE: &str = include_str!("../../images/rust-node/Containerfile");
const DOCKERIGNORE: &str = include_str!("../../.dockerignore");

#[test]
fn installs_the_codex_linux_sandbox_prerequisite() {
    assert!(
        CONTAINERFILE
            .lines()
            .any(|line| line.trim() == "bubblewrap \\"),
        "the image must install Debian's bubblewrap package"
    );
}

#[test]
fn includes_the_project_licenses_in_the_image() {
    assert!(
        CONTAINERFILE
            .contains("COPY LICENSE-APACHE LICENSE-MIT /usr/local/share/cloister/licenses/")
    );
    assert!(DOCKERIGNORE.lines().any(|line| line == "!LICENSE-APACHE"));
    assert!(DOCKERIGNORE.lines().any(|line| line == "!LICENSE-MIT"));
}
