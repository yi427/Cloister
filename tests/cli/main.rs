mod check;
mod claude;
mod codex;
mod host;
mod init;
mod profile_check;
mod profile_upgrade;
mod software;

fn seed_software(home: &std::path::Path, agent: &str) {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };
    let root = home
        .join(".local/share/cloister/software/linux-arm64")
        .join(agent);
    fs::create_dir_all(root.join("install-seeded/node_modules/.bin")).unwrap();
    let binary = root.join("install-seeded/node_modules/.bin").join(agent);
    fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let inventory = serde_json::json!({
        "schema_version": 1,
        "latest": {(concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"))): {
            "directory": "install-seeded",
            "checked_at": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        }},
        "installations": [{"directory":"install-seeded","version":"1.2.3","image":concat!("ghcr.io/yi427/cloister:", env!("CARGO_PKG_VERSION"))}]
    });
    fs::write(
        root.join("inventory.json"),
        serde_json::to_vec(&inventory).unwrap(),
    )
    .unwrap();
}
