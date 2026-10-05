//! The real binary's `start --all` (the Docker image's CMD) with no `scheduler:` jobs, which
//! Loco alone refuses with `Error: Scheduler(Empty)` (src/start.rs).

use std::{
    io::{Read, Write},
    net::TcpStream,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn get_up(port: u16) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .write_all(b"GET /up HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    Some(response)
}

#[test]
fn start_all_without_scheduler_jobs_serves_requests() {
    let port = 15_000 + u16::try_from(std::process::id() % 1000).unwrap();
    let dir = std::env::temp_dir().join(format!("irsk-start-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_inertia_rust_starter_kit-cli"))
        .args(["start", "--all", "--no-banner", "--binding", "127.0.0.1"])
        .args(["--port", &port.to_string()])
        .env("LOCO_ENV", "test")
        .env(
            "DATABASE_URL",
            format!("sqlite://{}/app.sqlite?mode=rwc", dir.display()),
        )
        .env(
            "QUEUE_URL",
            format!("sqlite://{}/queue.sqlite?mode=rwc", dir.display()),
        )
        .env_remove("SCHEDULER_CONFIG")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("stderr.log")).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let response = loop {
        if let Some(status) = child.try_wait().unwrap() {
            let stderr = std::fs::read_to_string(dir.join("stderr.log")).unwrap_or_default();
            panic!("`start --all` exited ({status}):\n{stderr}");
        }
        if let Some(response) = get_up(port) {
            break response;
        }
        assert!(Instant::now() < deadline, "no answer on :{port} in 30 s");
        std::thread::sleep(Duration::from_millis(200));
    };
    child.kill().unwrap();
    child.wait().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
}
