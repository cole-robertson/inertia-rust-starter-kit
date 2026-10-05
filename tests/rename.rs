//! `bin/rename`: renames the crate, binary, service names and display name, and nothing else.
//!
//! Runs the script on a temp copy of the files it touches and asserts on the result. That the
//! renamed app then builds and passes this suite was checked by hand
//! (docs/BUILDING_YOUR_APP.md, "Rename the app").

use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Everything `bin/rename` reads or rewrites, minus the large directories it only greps.
const COPY: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "migration",
    "package.json",
    "Dockerfile",
    "README.md",
    "bin",
    "config",
    "deploy/cloudflare/cloudflare.config.ts",
    "deploy/cloudflare/deploy.sh",
    "deploy/cloudflare/package.json",
    "deploy/compose",
    "deploy/systemd",
    "deploy/fly",
    "deploy/render",
    "frontend/entrypoints/app.ts",
    "frontend/components/app-sidebar.tsx",
    "SECURITY.md",
    "src/bin/main.rs",
    "tests/rename.rs",
    "tests/routes_fresh.rs",
    "docs/BENCHMARK.md",
];

fn copy(from: &Path, to: &Path) {
    if from.is_dir() {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            copy(&entry.path(), &to.join(entry.file_name()));
        }
    } else {
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(from, to).unwrap();
    }
}

fn app_copy(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("irsk-rename-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    for rel in COPY {
        copy(&source.join(rel), &root.join(rel));
    }
    root
}

fn rename(root: &Path, args: &[&str]) -> String {
    // A partial copy: no Cargo workspace or node_modules to format.
    let out = Command::new("bash")
        .arg("bin/rename")
        .args(args)
        .env("RENAME_NO_FORMAT", "1")
        .current_dir(root)
        .output()
        .expect("bash runs");
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "bin/rename {args:?} failed:\n{text}");
    text
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

fn crate_name(cargo_toml: &str) -> String {
    cargo_toml
        .lines()
        .find_map(|l| l.strip_prefix("name = \""))
        .and_then(|l| l.strip_suffix('"'))
        .expect("Cargo.toml names the package")
        .to_string()
}

/// The renamed `Cargo.lock` is the one cargo would write: running cargo again leaves it as it
/// is (the renamed package already sits in sorted order, so the first build after a rename
/// doesn't move it), and it holds the same lines as `before` with only the name changed (no
/// version was resolved anew). `old_name` is the crate name `before` was written for.
///
/// `--filter-platform host-tuple`, as `bin/rename` runs it: offline, cargo has only this
/// machine's crates after `cargo build` (CI's `cargo fetch` gets every platform's); it writes the
/// same lock either way.
fn assert_lock_is_cargos(root: &Path, before: &str, old_name: &str) {
    let renamed = read(root, "Cargo.lock");
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--offline",
            "--format-version",
            "1",
            "--filter-platform",
            "host-tuple",
        ])
        .current_dir(root)
        .output()
        .expect("cargo runs");
    assert!(
        out.status.success(),
        "cargo metadata: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        read(root, "Cargo.lock"),
        renamed,
        "cargo re-sorted the lock bin/rename left"
    );
    let sorted = |lock: &str| {
        let mut lines: Vec<String> = lock
            .lines()
            .map(|l| l.replace(old_name, "acme_crm"))
            .collect();
        lines.sort();
        lines
    };
    assert_eq!(sorted(&renamed), sorted(before), "only the order changed");
}

/// The summary line's "<N> occurrences in <M> files".
fn summary(out: &str) -> &str {
    let line = out
        .lines()
        .find(|l| l.starts_with("bin/rename: "))
        .unwrap_or_else(|| panic!("no summary line:\n{out}"));
    line.split(" change ")
        .nth(1)
        .or_else(|| line.split(" changed ").nth(1))
        .unwrap_or_else(|| panic!("no count in {line:?}"))
}

/// A dry run changes nothing, and its per-file counts and total are the real run's (overlapping
/// pairs such as `<snake>-cli` and `<kebab>` in deploy/systemd are counted once).
#[test]
fn a_dry_run_reports_what_the_real_run_changes_and_changes_nothing() {
    let root = app_copy("dry");
    let before = read(&root, "Cargo.toml");
    let dry = rename(&root, &["--dry-run", "acme-crm", "Acme CRM"]);
    assert!(dry.contains("would change"), "{dry}");
    assert_eq!(read(&root, "Cargo.toml"), before);
    let unit = format!(
        "deploy/systemd/{}.service",
        crate_name(&before).replace('_', "-")
    );
    assert!(root.join(&unit).exists(), "{unit} is not renamed");
    let real = rename(&root, &["acme-crm", "Acme CRM"]);
    let files = |out: &str| -> Vec<String> {
        out.lines()
            // "  <file>  <count>"; notes are indented further.
            .filter(|l| l.starts_with("  ") && !l.starts_with("   "))
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(files(&dry), files(&real), "per-file counts");
    assert_eq!(summary(&dry), summary(&real));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn rename_changes_every_app_identifier_and_keeps_credits_and_history() {
    let root = app_copy("real");
    let benchmark = read(&root, "docs/BENCHMARK.md");
    let this_test = read(&root, "tests/rename.rs");
    let lock = read(&root, "Cargo.lock");
    let old_name = crate_name(&read(&root, "Cargo.toml"));
    rename(&root, &["acme-crm", "Acme CRM"]);

    let cargo = read(&root, "Cargo.toml");
    assert!(cargo.contains("name = \"acme_crm\""));
    assert_lock_is_cargos(&root, &lock, &old_name);
    assert!(cargo.contains("default-run = \"acme_crm-cli\""));
    assert!(read(&root, "Cargo.lock").contains("name = \"acme_crm\""));
    assert!(
        read(&root, "src/bin/main.rs").contains("use acme_crm::{app::App, db, generate, start};")
    );
    assert!(read(&root, "tests/routes_fresh.rs").contains("use acme_crm::"));
    for env in ["development", "test", "production"] {
        assert!(read(&root, &format!("config/{env}.yaml")).contains("app_name: Acme CRM"));
    }
    assert!(read(&root, "config/development.yaml").contains("sqlite://acme_crm_development.sqlite"));
    assert!(read(&root, "bin/setup").contains("sqlite://acme_crm_development.sqlite"));
    assert!(read(&root, "frontend/entrypoints/app.ts").contains("?? \"Acme CRM\""));

    let deploy = read(&root, "config/deploy.yml");
    assert!(deploy.contains("service: acme_crm\n"));
    assert!(deploy.contains("/app/acme_crm-cli db migrate"));
    assert!(deploy.contains("your-user/acme-crm-build-cache"));
    assert!(read(&root, "Dockerfile").contains("CMD [\"/app/acme_crm-cli\""));
    assert!(read(&root, "bin/docker-entrypoint").contains("/app/acme_crm-cli db migrate"));
    assert!(read(&root, "deploy/cloudflare/cloudflare.config.ts").contains("name: \"acme-crm\""));
    assert!(read(&root, "deploy/cloudflare/deploy.sh").contains("-t \"acme-crm:$tag\""));

    let compose = read(&root, "deploy/compose/compose.yaml");
    assert!(compose.contains("name: acme-crm\n") && compose.contains("image: acme-crm\n"));
    assert!(
        !root
            .join("deploy/systemd/inertia-rust-starter-kit.service")
            .exists(),
        "the unit file is renamed with the service"
    );
    let unit = read(&root, "deploy/systemd/acme-crm.service");
    assert!(unit.contains("Description=Acme CRM\n"));
    assert!(unit.contains("StateDirectory=acme-crm\n"));
    assert!(unit.contains("EnvironmentFile=/etc/acme-crm/env\n"));
    assert!(
        unit.contains("ExecStart=/opt/acme-crm/acme_crm-cli start"),
        "the binary keeps the crate's snake_case name: {unit}"
    );
    assert!(read(&root, "deploy/fly/fly.toml").contains("app = \"acme-crm\""));
    assert!(read(&root, "deploy/render/render.yaml").contains("name: acme-crm\n"));
    let security = read(&root, "SECURITY.md");
    assert!(
        security.contains("(https://github.com/your-user/acme-crm/security/advisories/new)"),
        "the app's vulnerability reports go to the app's repository, not the kit's: {security}"
    );

    let readme = read(&root, "README.md");
    assert!(
        readme.starts_with("# Acme CRM\n"),
        "the title is renamed and the kit's wordmark above it is dropped"
    );
    assert!(!readme.contains("docs/logo/wordmark.svg"));
    for line in [
        "sudo install -m 755 target/release/acme_crm-cli /opt/acme-crm/",
        "sudo cp deploy/systemd/acme-crm.service /etc/systemd/system/",
        "sudo systemctl daemon-reload && sudo systemctl enable --now acme-crm",
    ] {
        assert!(readme.contains(line), "the README's systemd block: {line}");
    }
    assert!(
        readme.contains("[Inertia Rails React Starter Kit](https://github.com/inertia-rails/"),
        "the credit to the Rails kit keeps its name"
    );
    assert!(
        read(&root, "frontend/components/app-sidebar.tsx")
            .contains("github.com/cole-robertson/inertia-rust-starter-kit"),
        "links to the kit's repository are left alone"
    );
    assert_eq!(
        read(&root, "docs/BENCHMARK.md"),
        benchmark,
        "docs/ is history"
    );
    assert_eq!(
        read(&root, "tests/rename.rs"),
        this_test,
        "this test's expected strings are the kit's names, so a renamed app's suite stays green"
    );

    let again = rename(&root, &["acme_crm", "Acme CRM"]);
    assert!(
        again.contains("changed 0 occurrences"),
        "renaming is idempotent:\n{again}"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// The kit's names, as `bin/rename` finds them in the kit itself.
const KIT_NAMES: [&str; 3] = [
    "inertia_rust_starter_kit",
    "inertia-rust-starter-kit",
    "Inertia Rust Starter Kit",
];

/// Where the kit's names stay after a rename: history (`docs/`, `bench/`), the rename script and
/// this test, and links to the kit's repository (the README's clone command, the header and
/// sidebar links).
const KEPT_UNDER: [&str; 4] = ["docs/", "bench/", "bin/rename", "tests/rename.rs"];
const KIT_REPOSITORY: &str = "github.com/cole-robertson/inertia-rust-starter-kit";

/// The whole app, renamed: no file outside [`KEPT_UNDER`] still names the kit. Copies every file
/// git knows about (tracked, or new and not ignored), so a file added later is checked too.
#[test]
fn renaming_leaves_the_kits_names_only_in_history_and_links() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let listed = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(source)
        .output();
    let files = match listed {
        Ok(out) if out.status.success() => out.stdout,
        _ => {
            eprintln!("SKIPPED: not a git checkout, so the files to rename can't be listed");
            return;
        }
    };
    let root = std::env::temp_dir().join(format!("irsk-rename-all-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let files: Vec<&str> = files
        .split(|&b| b == 0)
        .filter(|f| !f.is_empty())
        .map(|f| std::str::from_utf8(f).expect("UTF-8 path"))
        .filter(|f| source.join(f).is_file())
        .collect();
    for rel in &files {
        copy(&source.join(rel), &root.join(rel));
    }
    rename(&root, &["acme-crm", "Acme CRM"]);

    let mut left = Vec::new();
    for entry in walk(&root) {
        let rel = entry
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if KEPT_UNDER.iter().any(|kept| rel.starts_with(kept)) {
            continue;
        }
        let text =
            String::from_utf8_lossy(&std::fs::read(&entry).unwrap()).replace(KIT_REPOSITORY, "");
        for (n, line) in text.lines().enumerate() {
            if KIT_NAMES.iter().any(|name| line.contains(name)) {
                left.push(format!("{rel}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        left.is_empty(),
        "bin/rename left the kit's name in {} line(s); rename them in bin/rename, or add the \
         path to KEPT_UNDER if it is history:\n{}",
        left.len(),
        left.join("\n")
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn invalid_names_are_refused_before_anything_changes() {
    let root = app_copy("invalid");
    let before = read(&root, "Cargo.toml");
    for args in [
        &["Acme"][..],
        &["9lives"],
        &["migration"],
        &["ok_name", "Bad/Name"],
    ] {
        let out = Command::new("bash")
            .arg("bin/rename")
            .args(args)
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?} was accepted");
    }
    assert_eq!(read(&root, "Cargo.toml"), before);
    std::fs::remove_dir_all(&root).unwrap();
}

/// A one-word name is its own snake and kebab form (`trackline`), so renaming it again must still
/// give the build cache and the Cloudflare Worker the kebab name, not the snake one.
#[test]
fn renaming_a_one_word_name_keeps_the_kebab_service_names() {
    let root = app_copy("oneword");
    rename(&root, &["trackline", "Trackline"]);
    rename(&root, &["acme-crm", "Acme CRM"]);
    let deploy = read(&root, "config/deploy.yml");
    assert!(
        deploy.contains("your-user/acme-crm-build-cache"),
        "{deploy}"
    );
    assert!(read(&root, "deploy/cloudflare/cloudflare.config.ts").contains("name: \"acme-crm\""));
    assert!(read(&root, "Cargo.toml").contains("name = \"acme_crm\""));
    let unit = read(&root, "deploy/systemd/acme-crm.service");
    assert!(
        unit.contains("ExecStart=/opt/acme-crm/acme_crm-cli start"),
        "{unit}"
    );
    // The README's systemd block too: paths and unit name are the kebab name.
    let readme = read(&root, "README.md");
    for line in [
        "sudo install -d /opt/acme-crm /etc/acme-crm",
        "sudo cp deploy/systemd/acme-crm.service /etc/systemd/system/",
        "enable --now acme-crm",
    ] {
        assert!(readme.contains(line), "README lacks {line:?}");
    }
    std::fs::remove_dir_all(&root).unwrap();
}
