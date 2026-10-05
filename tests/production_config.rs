//! config/production.yaml: a first deploy boots without SMTP, and SMTP stays all-or-nothing.

use inertia_rust_starter_kit::app::App;
use loco_rs::{app::Hooks, environment::Environment};
use serial_test::serial;

const REQUIRED: [(&str, &str); 4] = [
    ("SECRET_KEY_BASE", "x"),
    ("HOST", "https://app.example.com"),
    ("DATABASE_URL", "sqlite://p.sqlite?mode=rwc"),
    ("QUEUE_URL", "sqlite://q.sqlite?mode=rwc"),
];
const MAILER: [&str; 3] = ["MAILER_HOST", "MAILER_USER", "MAILER_PASSWORD"];

async fn production_config(mailer: &[(&str, &str)]) -> loco_rs::Result<loco_rs::config::Config> {
    for (k, v) in REQUIRED.iter().chain(mailer) {
        std::env::set_var(k, v);
    }
    let config = App::load_config(&Environment::Production).await;
    for k in REQUIRED.iter().map(|(k, _)| k).chain(&MAILER) {
        std::env::remove_var(k);
    }
    config
}

#[tokio::test]
#[serial]
async fn production_without_mailer_host_boots_on_the_stub_transport() {
    let config = production_config(&[]).await.unwrap();
    let mailer = config.mailer.unwrap();
    assert!(mailer.stub, "no SMTP host: Loco's stub, so nothing is sent");
    assert!(!mailer.smtp.unwrap().enable);
}

#[tokio::test]
#[serial]
async fn production_with_mailer_host_sends_over_smtp_with_credentials() {
    let config = production_config(&[
        ("MAILER_HOST", "smtp.example.com"),
        ("MAILER_USER", "postmaster@example.com"),
        ("MAILER_PASSWORD", "hunter2"),
    ])
    .await
    .unwrap();
    let mailer = config.mailer.unwrap();
    assert!(!mailer.stub);
    let smtp = mailer.smtp.unwrap();
    assert!(smtp.enable);
    assert_eq!(smtp.host, "smtp.example.com");
    assert_eq!(smtp.auth.unwrap().user, "postmaster@example.com");
}

#[tokio::test]
#[serial]
async fn production_with_mailer_host_but_no_credentials_refuses_to_load() {
    let err = production_config(&[("MAILER_HOST", "smtp.example.com")])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("MAILER_USER"), "{err}");
}

#[tokio::test]
#[serial]
async fn production_still_requires_secret_key_base() {
    std::env::set_var("HOST", "https://app.example.com");
    std::env::set_var("DATABASE_URL", "sqlite://p.sqlite?mode=rwc");
    std::env::set_var("QUEUE_URL", "sqlite://q.sqlite?mode=rwc");
    let config = App::load_config(&Environment::Production).await;
    for k in ["HOST", "DATABASE_URL", "QUEUE_URL"] {
        std::env::remove_var(k);
    }
    let err = config.unwrap_err();
    assert!(err.to_string().contains("SECRET_KEY_BASE"), "{err}");
}

/// The image and the systemd unit start `--all`, so `scheduler:` jobs in `config/production.yaml`
/// run without anyone remembering to change the start mode.
#[test]
fn deploys_start_the_scheduler_with_the_server_and_worker() {
    let dockerfile = std::fs::read_to_string("Dockerfile").unwrap();
    assert!(
        dockerfile.contains(r#"CMD ["/app/inertia_rust_starter_kit-cli", "start", "--all""#),
        "Dockerfile CMD"
    );
    let unit = std::fs::read_to_string("deploy/systemd/inertia-rust-starter-kit.service").unwrap();
    assert!(
        unit.contains("inertia_rust_starter_kit-cli start --all "),
        "systemd ExecStart"
    );
}
