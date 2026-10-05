//! `cargo loco start` and the scheduler.
//!
//! The image starts `--all` (server, queue worker and scheduler), so jobs under `scheduler:` in
//! `config/<env>.yaml` run without anyone changing the start mode. Loco refuses to start a
//! scheduler that has no jobs (`Error: Scheduler(Empty)`), so when none are configured,
//! `src/bin/main.rs` runs the same command minus the scheduler (`--all` becomes
//! `--server-and-worker`). The reverse case, jobs configured in a mode that never runs them,
//! prints a warning at boot.

/// What `start` does about the scheduler.
#[derive(Debug, PartialEq, Eq)]
pub enum Scheduler {
    /// Not a `start` command, or nothing to change.
    Unchanged,
    /// The mode asks for the scheduler and there are no jobs: run these arguments instead.
    Skip(Vec<String>),
    /// These jobs are configured, but the mode never runs the scheduler.
    NotRun(Vec<String>),
}

/// The index of the subcommand in `args` (binary first), skipping `-e <env>`.
fn subcommand(args: &[String]) -> Option<usize> {
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-e" | "--environment" => i += 2,
            a if a.starts_with('-') => i += 1,
            _ => return Some(i),
        }
    }
    None
}

/// The plan for `args` (the full command line, binary first) given the configured job names.
/// `SCHEDULER_CONFIG` (Loco reads the jobs from that file) leaves everything to Loco.
#[must_use]
pub fn plan(args: &[String], jobs: &[String]) -> Scheduler {
    let Some(start) = subcommand(args).filter(|&i| args[i] == "start" || args[i] == "s") else {
        return Scheduler::Unchanged;
    };
    if std::env::var_os("SCHEDULER_CONFIG").is_some() {
        return Scheduler::Unchanged;
    }
    let flags = &args[start + 1..];
    let all = flags.iter().any(|f| f == "--all" || f == "-a");
    let scheduler = all || flags.iter().any(|f| f == "--scheduler");
    match (scheduler, jobs.is_empty()) {
        (true, true) => Scheduler::Skip(
            args[..=start]
                .iter()
                .cloned()
                .chain(flags.iter().filter_map(|f| match f.as_str() {
                    "--scheduler" => None,
                    "--all" | "-a" => Some("--server-and-worker".to_owned()),
                    _ => Some(f.clone()),
                }))
                .collect(),
        ),
        (false, false) => {
            let mut jobs = jobs.to_vec();
            jobs.sort();
            Scheduler::NotRun(jobs)
        }
        _ => Scheduler::Unchanged,
    }
}

/// The environment named on the command line (`-e test`, `--environment=test`), if any.
#[must_use]
pub fn environment(args: &[String]) -> Option<String> {
    args.iter().enumerate().find_map(|(i, a)| match a.as_str() {
        "-e" | "--environment" => args.get(i + 1).cloned(),
        _ => a.strip_prefix("--environment=").map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    #[test]
    fn all_without_jobs_runs_the_server_and_worker() {
        assert_eq!(
            plan(&args("cli start --all --no-banner"), &[]),
            Scheduler::Skip(args("cli start --server-and-worker --no-banner"))
        );
        assert_eq!(
            plan(&args("cli -e production start -a"), &[]),
            Scheduler::Skip(args("cli -e production start --server-and-worker"))
        );
        assert_eq!(
            plan(&args("cli start --worker --scheduler"), &[]),
            Scheduler::Skip(args("cli start --worker"))
        );
    }

    #[test]
    fn all_with_jobs_and_other_commands_are_left_alone() {
        let jobs = ["prune".to_owned()];
        assert_eq!(plan(&args("cli start --all"), &jobs), Scheduler::Unchanged);
        assert_eq!(plan(&args("cli start"), &[]), Scheduler::Unchanged);
        assert_eq!(plan(&args("cli task --all"), &[]), Scheduler::Unchanged);
        assert_eq!(plan(&args("cli -e start task"), &[]), Scheduler::Unchanged);
    }

    #[test]
    fn jobs_in_a_mode_without_the_scheduler_are_named() {
        let jobs = ["prune".to_owned(), "digest".to_owned()];
        let expected = Scheduler::NotRun(vec!["digest".into(), "prune".into()]);
        assert_eq!(
            plan(&args("cli start --server-and-worker"), &jobs),
            expected
        );
        assert_eq!(plan(&args("cli start"), &jobs), expected);
        assert_eq!(plan(&args("cli start --worker"), &jobs), expected);
    }

    #[test]
    fn the_environment_flag() {
        assert_eq!(environment(&args("cli -e test start")), Some("test".into()));
        assert_eq!(
            environment(&args("cli start --environment=production")),
            Some("production".into())
        );
        assert_eq!(environment(&args("cli start --all")), None);
    }
}
