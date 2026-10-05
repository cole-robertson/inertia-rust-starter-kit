use inertia_rust_starter_kit::{app::App, db, generate, start};
use loco_rs::{app::Hooks, cli, environment::resolve_from_env};
use migration::Migrator;

// mimalloc instead of glibc malloc: +50–66% req/s on the page benchmark, at ~3× the (small)
// resident memory. Numbers in docs/BENCHMARK.md, "Allocator and release profile".
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// `loco_rs::Error` is large (>128 bytes), which trips clippy::result_large_err. That is
// loco's type, not ours, and main returns it at most once, so boxing would buy nothing.
#[allow(clippy::result_large_err)]
#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    // `cargo loco generate scaffold|controller`: the kit's account-scope defaults, `--global`
    // and nested controllers (src/generate.rs). Loco reads the real command line and the
    // templates read the environment, so the generator runs as a child process with both set.
    let args: Vec<String> = std::env::args().collect();
    // `cargo loco generate channel <name>`: the kit's own generator (Loco has none).
    match generate::channel(&args, std::path::Path::new("."), env!("CARGO_PKG_NAME")) {
        Ok(None) => {}
        Ok(Some(messages)) => {
            println!("{messages}");
            return Ok(());
        }
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(2);
        }
    }
    if std::env::var_os(generate::SCOPE_ENV).is_none() {
        match generate::plan(&args, std::path::Path::new(".")) {
            Ok(None) => {}
            Ok(Some(plan)) => {
                if let Some(note) = &plan.note {
                    println!("{note}");
                }
                let status = std::process::Command::new(std::env::current_exe()?)
                    .args(&plan.args[1..])
                    .envs(plan.env)
                    .status()?;
                std::process::exit(status.code().unwrap_or(1));
            }
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(2);
            }
        }
    }
    // `db reset` / `db seed --reset` on a one-connection pool, so sea-orm-migration's
    // `foreign_keys = OFF` holds for every table it drops (src/db.rs, `drops_every_table`).
    // Every config/*.yaml reads `max_connections` from DB_MAX_CONNECTIONS. Set before Loco reads
    // the config, while no other thread reads the environment.
    if db::drops_every_table(&args) {
        std::env::set_var("DB_MAX_CONNECTIONS", "1");
    }
    // `start --all` with no `scheduler:` jobs: the same command without the scheduler, which
    // Loco would refuse to start (src/start.rs). Jobs in a mode that never runs them: a warning.
    if let Ok(config) = App::load_config(
        &start::environment(&args)
            .unwrap_or_else(resolve_from_env)
            .into(),
    )
    .await
    {
        let jobs: Vec<String> = config
            .scheduler
            .map(|s| s.jobs.into_keys().collect())
            .unwrap_or_default();
        match start::plan(&args, &jobs) {
            start::Scheduler::Unchanged => {}
            start::Scheduler::NotRun(jobs) => eprintln!(
                "WARN scheduler: {} configured under `scheduler:`, but this start mode doesn't \
                 run the scheduler. Start with `--all` (or `--scheduler`).",
                jobs.join(", ")
            ),
            start::Scheduler::Skip(args) => {
                let mut command = std::process::Command::new(std::env::current_exe()?);
                command.args(&args[1..]);
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    return Err(command.exec().into());
                }
                #[cfg(not(unix))]
                std::process::exit(command.status()?.code().unwrap_or(1));
            }
        }
    }
    cli::main::<App, Migrator>().await
}
