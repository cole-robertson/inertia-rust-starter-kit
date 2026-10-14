//! Fails when the committed `src/models/_entities/` differ from what `cargo loco db entities`
//! writes for the schema the migrations make. Editing a generated migration (an index, a unique
//! key) after `generate model|scaffold` already ran `db entities` leaves them stale, and nothing
//! else notices until code needs the missing `unique`/`has_one`. Fix with
//! `cargo loco db migrate && cargo loco db entities` (`db reset` instead of `db migrate` after
//! editing a migration that already ran) and commit the result.
//!
//! Runs the app's own CLI on a fresh SQLite database in a temp copy of `Cargo.toml`, `config/`
//! and `src/models/`, so the committed files are never touched. Needs `sea-orm-cli` (what
//! `db entities` runs); without it the test is skipped, and `bin/ci` refuses to start the step.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Copy `from` to `to`, minus git-ignored local config (`config/*.local.yaml` could point the
/// copy at a real database).
fn copy(from: &Path, to: &Path) {
    if from.is_dir() {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().ends_with(".local.yaml") {
                continue;
            }
            copy(&entry.path(), &to.join(entry.file_name()));
        }
    } else {
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(from, to).unwrap();
    }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| PathBuf::from(e.unwrap().file_name()))
        .collect();
    names.sort();
    names
}

/// `<cli> <args>` in `root` on the database at `db`, panicking with its output on failure.
///
/// The development environment: Loco runs `db entities` inside `tracing::warn!`'s arguments,
/// which aren't evaluated when the logger is off, as `config/test.yaml` has it, so there it is a
/// silent no-op. `RUST_LOG` is removed for the same reason.
fn cli(root: &Path, db: &Path, args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_inertia_rust_starter_kit-cli"))
        .args(args)
        .current_dir(root)
        .env("LOCO_ENV", "development")
        .env("LOG_LEVEL", "warn")
        .env_remove("RUST_LOG")
        .env(
            "DATABASE_URL",
            format!("sqlite://{}?mode=rwc", db.display()),
        )
        .env(
            "QUEUE_URL",
            format!("sqlite://{}?mode=rwc", root.join("queue.sqlite").display()),
        )
        .output()
        .expect("the CLI runs");
    assert!(
        out.status.success(),
        "{args:?} failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn committed_entities_match_the_migrations() {
    if cfg!(feature = "bench") {
        // `--features bench` adds the bench_events migration, which has no committed entity.
        eprintln!("SKIPPED: the bench feature's migrations have no committed entities");
        return;
    }
    if Command::new("sea-orm-cli")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!(
            "SKIPPED: sea-orm-cli is not installed (cargo install --locked sea-orm-cli@2.0.4)"
        );
        return;
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = std::env::temp_dir().join(format!("irsk-entities-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // `db entities` reads `[package.metadata.db.entity]` from Cargo.toml, writes
    // `src/models/_entities/`, and adds a model file for any entity that has none.
    for rel in ["Cargo.toml", "config", "src/models"] {
        copy(&source.join(rel), &root.join(rel));
    }
    let entities = "src/models/_entities";
    std::fs::remove_dir_all(root.join(entities)).unwrap();
    std::fs::create_dir_all(root.join(entities)).unwrap();
    let db = root.join("entities.sqlite");
    cli(&root, &db, &["db", "migrate"]);
    cli(&root, &db, &["db", "entities"]);

    let committed = source.join(entities);
    let generated = root.join(entities);
    let mut problems = Vec::new();
    let (ours, theirs) = (files(&committed), files(&generated));
    for name in &theirs {
        let rel = Path::new(entities).join(name);
        match std::fs::read_to_string(committed.join(name)) {
            Ok(text) if text == std::fs::read_to_string(generated.join(name)).unwrap() => {}
            Ok(_) => problems.push(format!("{} is out of date", rel.display())),
            Err(_) => problems.push(format!("{} is missing", rel.display())),
        }
    }
    for name in ours.iter().filter(|n| !theirs.contains(n)) {
        problems.push(format!(
            "{} has no table (stale)",
            Path::new(entities).join(name).display()
        ));
    }
    std::fs::remove_dir_all(&root).unwrap();

    assert!(
        problems.is_empty(),
        "src/models/_entities is stale for the schema the migrations make; run \
         `cargo loco db migrate && cargo loco db entities` (`db reset` instead of `db migrate` \
         after editing a migration that already ran) and commit the result:\n  {}",
        problems.join("\n  ")
    );
}
