//! Server-side rendering: an HTTP client for the Inertia SSR server, and a
//! Loco initializer that runs `node ssr/ssr.js` as a supervised child.
//!
//! Every SSR failure is logged and the page falls back to client rendering.
//!
//! The child's stdout and stderr are not inherited: Inertia's SSR error
//! formatter prints the page URL (a password-reset `?sid=…` included), so
//! every line goes through [`request_log::redact_text`] into `tracing`
//! (see [`spawn_redacted`]).

use std::{path::Path, process::Stdio, sync::Arc, time::Duration};

use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Initializer},
    Result,
};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, Command},
    sync::watch,
    task::JoinHandle,
};

use super::{config::Settings, request_log};

/// What `@inertiajs/react/server` returns: head tags and the body HTML
/// (which already contains the `<script data-page>` and `<div id="app">`).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SsrOutput {
    pub head: Vec<String>,
    pub body: String,
}

/// Posts page JSON to the SSR server: inertia-omega's SSR gateway. Unlike omega's
/// `HttpGateway`, a failed render logs only the status and component, never the error body,
/// which echoes the page (a password-reset URL's `?sid=…`, the props).
#[derive(Debug, Clone)]
pub struct SsrClient {
    http: reqwest::Client,
    url: String,
}

impl SsrClient {
    /// The client for `settings`, or `None` when SSR is off.
    ///
    /// Dev server on: `<vite.dev_server_url>/__inertia_ssr`; otherwise `ssr.url`.
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Option<Self> {
        if !settings.ssr.enabled {
            return None;
        }
        let url = if settings.vite.dev_server {
            format!(
                "{}/__inertia_ssr",
                settings.vite.dev_server_url.trim_end_matches('/')
            )
        } else {
            settings.ssr.url.clone()
        };
        Some(Self::new(
            url,
            Duration::from_millis(settings.ssr.timeout_ms),
        ))
    }

    /// # Panics
    /// If the TLS backend cannot initialize (reqwest's `Client::new` does the same).
    #[must_use]
    pub fn new(url: String, timeout: Duration) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest client builds");
        Self { http, url }
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Renders `page`, or `None` (with a warning) on any failure.
    pub async fn render(&self, page: &omega::Page) -> Option<SsrOutput> {
        match self.try_render(page).await {
            Ok(out) => Some(out),
            Err(e) => {
                tracing::warn!(component = %page.component, error = %e,
                    "inertia SSR failed; rendering client-side");
                None
            }
        }
    }

    async fn try_render(&self, page: &omega::Page) -> std::result::Result<SsrOutput, String> {
        let body = serde_json::to_string(page).map_err(|e| e.to_string())?;
        let res = self
            .http
            .post(&self.url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = res.status();
        if !status.is_success() {
            // The error body echoes the page (URL with `?sid=…`, props), so
            // only the status is logged; the component is on the warn line.
            return Err(format!("SSR server returned {status}"));
        }
        res.json::<SsrOutput>().await.map_err(|e| e.to_string())
    }
}

impl omega::ssr::Gateway for SsrClient {
    async fn dispatch(
        &self,
        page: &omega::Page,
        _request: &omega::Request,
    ) -> Option<omega::ssr::Rendered> {
        self.render(page)
            .await
            .map(|SsrOutput { head, body }| omega::ssr::Rendered {
                head: head.join("\n"),
                body,
            })
    }
}

/// Stops the supervised SSR process (see [`SsrSupervisor`]).
#[derive(Clone)]
pub struct SsrShutdown(watch::Sender<bool>);

/// Signals the supervisor (if one runs) to kill the child and stop.
pub fn shutdown(ctx: &AppContext) {
    if let Some(SsrShutdown(tx)) = ctx.shared_store.get::<SsrShutdown>() {
        let _ = tx.send(true);
    }
}

/// Runs `<ssr.node> <ssr.bundle>` when `ssr.enabled && ssr.spawn`, restarting
/// it with exponential backoff (1s..30s) whenever it exits.
pub struct SsrSupervisor;

const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A child that stayed up this long resets the backoff.
const HEALTHY_RUN: Duration = Duration::from_secs(30);

#[async_trait]
impl Initializer for SsrSupervisor {
    fn name(&self) -> String {
        "inertia-ssr-supervisor".to_owned()
    }

    async fn before_run(&self, ctx: &AppContext) -> Result<()> {
        let Some(settings) = ctx.shared_store.get::<Arc<Settings>>() else {
            tracing::warn!("inertia settings missing; SSR supervisor not started");
            return Ok(());
        };
        if !(settings.ssr.enabled && settings.ssr.spawn) {
            return Ok(());
        }
        if !Path::new(&settings.ssr.bundle).exists() {
            tracing::warn!(bundle = %settings.ssr.bundle,
                "SSR bundle missing; not spawning the SSR server (pages render client-side)");
            return Ok(());
        }
        let (tx, rx) = watch::channel(false);
        ctx.shared_store.insert(SsrShutdown(tx));
        tokio::spawn(supervise(
            settings.ssr.node.clone(),
            settings.ssr.bundle.clone(),
            ssr_port(&settings.ssr.url),
            rx,
        ));
        Ok(())
    }
}

/// Env var the SSR entry (`frontend/entrypoints/ssr.tsx`) reads its port from.
pub const PORT_ENV: &str = "INERTIA_SSR_PORT";

/// The port of `ssr.url` (explicit, or the scheme default), which the
/// spawned server is told to listen on through [`PORT_ENV`].
#[must_use]
pub fn ssr_port(url: &str) -> Option<u16> {
    url::Url::parse(url).ok()?.port_or_known_default()
}

async fn supervise(
    node: String,
    bundle: String,
    port: Option<u16>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let started = tokio::time::Instant::now();
        let mut command = Command::new(&node);
        command.arg(&bundle);
        if let Some(port) = port {
            command.env(PORT_ENV, port.to_string());
        }
        match spawn_redacted(command) {
            Ok((mut child, _output)) => {
                tracing::info!(pid = ?child.id(), bundle = %bundle, "SSR server started");
                tokio::select! {
                    status = child.wait() => {
                        tracing::warn!(?status, "SSR server exited; restarting in {backoff:?}");
                    }
                    _ = stop.changed() => {
                        let _ = child.kill().await;
                        tracing::info!("SSR server stopped");
                        return;
                    }
                }
            }
            Err(e) => {
                tracing::error!(node = %node, error = %e, "could not spawn SSR server; retrying in {backoff:?}");
            }
        }
        if started.elapsed() >= HEALTHY_RUN {
            backoff = Duration::from_secs(1);
        }
        tokio::select! {
            () = tokio::time::sleep(backoff) => {}
            _ = stop.changed() => return,
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Spawns `command` (killed on drop) with stdout and stderr piped into
/// `tracing` line by line, sensitive query values redacted
/// ([`request_log::redact_text`]). Stdout lines log at `info`, stderr lines
/// at `warn`, under this module's target, which Loco's default log filter
/// (the app crate) passes. The handle finishes once both streams close.
///
/// # Errors
/// When the process cannot be spawned.
pub fn spawn_redacted(mut command: Command) -> std::io::Result<(Child, JoinHandle<()>)> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child.stdout.take().map(|s| tokio::spawn(forward(s, false)));
    let stderr = child.stderr.take().map(|s| tokio::spawn(forward(s, true)));
    let output = tokio::spawn(async move {
        for task in [stdout, stderr].into_iter().flatten() {
            let _ = task.await;
        }
    });
    Ok((child, output))
}

async fn forward(stream: impl AsyncRead + Unpin, stderr: bool) {
    let mut lines = BufReader::new(stream);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match lines.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let line = request_log::redact_text(line.trim_end_matches(['\n', '\r']));
        if stderr {
            tracing::warn!(ssr_output = %line, "inertia SSR server stderr");
        } else {
            tracing::info!(ssr_output = %line, "inertia SSR server stdout");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_uses_vite_endpoint_and_prod_uses_ssr_url() {
        let mut s: Settings = serde_json::from_value(serde_json::json!({
            "secret_key_base": "x", "app_url": "http://a", "app_name": "T",
            "mail_from": "a@b.c", "encrypt_history": false, "forgery_protection": true,
            "vite": {"dev_server": true, "dev_server_url": "http://localhost:5173/",
                     "manifest_path": "m.json"},
            "ssr": {"enabled": true, "spawn": false, "bundle": "ssr/ssr.js", "timeout_ms": 10}
        }))
        .unwrap();
        assert_eq!(
            SsrClient::from_settings(&s).unwrap().url(),
            "http://localhost:5173/__inertia_ssr"
        );
        s.vite.dev_server = false;
        assert_eq!(
            SsrClient::from_settings(&s).unwrap().url(),
            "http://127.0.0.1:13714/render"
        );
        s.ssr.enabled = false;
        assert!(SsrClient::from_settings(&s).is_none());
    }

    #[test]
    fn the_spawned_server_listens_on_the_ssr_url_port() {
        assert_eq!(ssr_port("http://127.0.0.1:13715/render"), Some(13715));
        assert_eq!(ssr_port("http://127.0.0.1/render"), Some(80));
        assert_eq!(ssr_port("not a url"), None);
    }
}
