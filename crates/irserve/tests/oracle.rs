use std::path::PathBuf;
use std::process::Command;

#[test]
fn oracle_l0_must_match() {
    let bin = env!("CARGO_BIN_EXE_irserve");

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR should live two levels under workspace root");
    let runner = workspace_root.join("tools").join("probe").join("run.mjs");

    assert!(
        runner.exists(),
        "probe runner missing at {} — is the workspace layout intact?",
        runner.display()
    );

    let status = Command::new("node")
        .arg(&runner)
        .arg("--all")
        .arg("--target=irserve")
        .arg("--snapshot=verify")
        .env("IRSERVE_BIN", bin)
        .current_dir(workspace_root)
        .status()
        .expect("failed to spawn `node`; oracle harness needs Node.js on PATH");

    assert!(
        status.success(),
        "oracle harness exited non-zero (see runner output above)"
    );
}
