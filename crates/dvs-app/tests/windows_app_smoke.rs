#![cfg(target_os = "windows")]

use std::path::PathBuf;

use dvs_app::{AppConfig, run};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn fixture_path() -> PathBuf {
    std::env::var_os("DVS_DECODER_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("docs/fixtures/test_4k_hevc_8bit30.mp4"))
}

#[test]
#[ignore = "requires Windows D3D11VA hardware and the 4K HEVC fixture"]
fn windows_app_smoke_plays_fixture_to_eof() {
    let fixture = fixture_path();
    assert!(
        fixture.is_file(),
        "fixture not found at {} — place the 4K HEVC fixture or set DVS_DECODER_FIXTURE",
        fixture.display()
    );

    let config =
        AppConfig::smoke_test_with_post_eof_resize(&fixture).expect("valid smoke-test config");
    assert!(config.smoke_post_eof_resize());
    run(config).expect("production app smoke playback");
}
