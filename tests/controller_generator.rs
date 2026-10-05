//! The kit's `cargo loco generate controller` templates (`.loco-templates/controller/`) against
//! Loco's real generator, with the command line and environment `src/generate.rs` plans.
//!
//! The fast tests render them into a copy of the files they inject into and assert on the
//! result (page controllers, write actions, nested controllers, account-scoped or `--global`),
//! so a Loco upgrade or an edit that moves an anchor fails `cargo test`.
//! `generated_code_builds_and_passes_its_tests` goes further: in a full copy of the app it runs
//! the real `cargo loco generate` for two scaffolds (account-scoped `widgets`, global `gizmos`),
//! five controllers (a page controller, a write controller, a nested `widgets/approvals`, a glob
//! member page `folders show:*path` and a create-only `pings`), a channel, a worker (given an
//! args field) and a task (that reads a seeded row), then `scaffold:pages`, `cargo clippy`, every generated request test, `tsc` and
//! ESLint there.
//! That takes minutes (a second target dir, since this test's own `cargo test` holds the lock on
//! `target/`), so it is `#[ignore]`d and run by `bin/ci` and the "Generated code builds" CI job:
//!
//! ```sh
//! cargo test --test controller_generator -- --ignored
//! ```
//!
//! Its own test binary, because Loco's generator resolves `.loco-templates/` against the process
//! working directory, which these tests change.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use inertia_rust_starter_kit::{generate::plan, tasks::scaffold_pages};
use serial_test::serial;

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// Copy `from` to `to`, skipping build output, dependencies, databases and git.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            "target" | "node_modules" | ".git" | "tmp" | "test-results" | "playwright-report"
        ) || name.contains(".sqlite")
        {
            continue;
        }
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn app_copy(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("irsk-controller-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    copy_dir(Path::new(env!("CARGO_MANIFEST_DIR")), &root);
    root
}

/// `cargo loco generate <line>` for a controller, in-process: the planned command line and
/// environment (src/generate.rs), then Loco's generator with the kit's templates.
fn generate_controller(root: &Path, line: &str) -> Result<String, String> {
    let args: Vec<String> = std::iter::once("cli")
        .chain(line.split(' '))
        .map(str::to_owned)
        .collect();
    let plan = plan(&args, root)?.expect("a controller");
    for key in [
        "KIT_GENERATE_PARENT",
        "KIT_GENERATE_PARENT_SINGULAR",
        "KIT_GENERATE_CHILD_SINGULAR",
        "KIT_GENERATE_PARENT_MODULE",
        "KIT_GENERATE_PARENT_SCOPED",
        "KIT_GENERATE_SINGULAR",
    ] {
        std::env::remove_var(key);
    }
    for (key, value) in &plan.env {
        std::env::set_var(key, value);
    }
    let name = plan.args[3].clone();
    let actions = plan.args[4..].to_vec();
    std::env::set_current_dir(root).unwrap();
    let results = loco_gen::generate(
        &loco_gen::RRgen::with_working_dir(root),
        loco_gen::Component::Controller {
            name,
            actions,
            auth: false,
        },
        &loco_gen::AppInfo {
            app_name: env!("CARGO_PKG_NAME").into(),
            working_dir: root.to_path_buf(),
        },
    );
    std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).unwrap();
    results
        .map(|r| loco_gen::collect_messages(&r))
        .map_err(|err| {
            // Tera puts a `throw` message on the error's source chain, not its top-level text.
            let mut chain = err.to_string();
            let mut source = std::error::Error::source(&err);
            while let Some(cause) = source {
                chain.push_str(&format!(": {cause}"));
                source = cause.source();
            }
            chain
        })
}

/// `cargo loco generate controller monthly_reports summary year_end`.
fn generate(root: &Path) -> String {
    generate_controller(root, "generate controller monthly_reports summary year_end")
        .expect("the kit's controller templates render and every injection anchor matches")
}

fn compact(s: &str) -> String {
    s.split_whitespace().collect()
}

#[test]
#[serial]
fn generate_controller_writes_an_inertia_page_controller() {
    let root = app_copy("shape");
    let messages = generate(&root);
    for template in ["controller/api/controller.t", "controller/api/test.t"] {
        assert!(
            messages.contains(&format!(".loco-templates/{template}")),
            "{template} was not picked up from .loco-templates:\n{messages}"
        );
    }
    assert!(
        messages.contains("cargo loco task scaffold:pages controller:monthly_reports"),
        "the message names the follow-up command:\n{messages}"
    );

    // A signed-in Inertia page per action (plus index), not Loco's JSON `format::empty()`.
    let controller = read(&root, "src/controllers/monthly_reports.rs");
    for (action, component) in [
        ("index", "monthly_reports/index"),
        ("summary", "monthly_reports/summary"),
        ("year_end", "monthly_reports/year_end"),
    ] {
        assert!(
            controller.contains(&format!("async fn {action}(_: CurrentAccount,")),
            "{controller}"
        );
        assert!(controller.contains(&format!("render(inertia, \"{component}\", json!({{}}))")));
    }
    assert!(controller.contains(".add(route_table::MONTHLY_REPORTS_YEAR_END, get(year_end))"));
    assert!(!controller.contains("format::empty()") && !controller.contains("\"api/"));

    let table = read(&root, "src/route_table.rs");
    assert!(
        table.contains("pub const MONTHLY_REPORTS: &str = \"/{account_slug}/monthly_reports\";\n")
    );
    assert!(table.contains(
        "pub const MONTHLY_REPORTS_YEAR_END: &str = \"/{account_slug}/monthly_reports/year_end\";\n"
    ));
    assert!(table.contains("pub fn monthly_reports_path(slug: &str) -> String"));
    assert!(
        table.find("pub const MONTHLY_REPORTS").unwrap() < table.find("// scaffold:paths").unwrap()
    );
    let compact: String = table.split_whitespace().collect();
    assert!(compact.contains(
        "route(\"monthly_reports.year_end\",Get,MONTHLY_REPORTS_YEAR_END,ts(\"MonthlyReportsController\",\"monthlyReports\",\"yearEnd\",None)),"
    ));

    // `mod` lines go with the others, right after the last one (not appended after the helper
    // functions at the end of tests/requests/mod.rs).
    for (rel, line) in [
        ("src/controllers/mod.rs", "pub mod monthly_reports;"),
        ("tests/requests/mod.rs", "mod monthly_reports;"),
    ] {
        let lines: Vec<String> = read(&root, rel).lines().map(str::to_string).collect();
        let at = lines
            .iter()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("{rel}: no `{line}`"));
        assert!(
            at > 0 && lines[at - 1].trim_start_matches("pub ").starts_with("mod "),
            "{rel}: `{line}` follows `{}`, not another mod line",
            lines[at - 1]
        );
        assert!(
            !lines[at + 1..]
                .iter()
                .any(|l| l.trim_start_matches("pub ").starts_with("mod ")),
            "{rel}: `{line}` is not after the last mod line"
        );
    }
    assert!(read(&root, "src/app.rs").contains(
        "AppRoutes::empty()\n            .add_route(controllers::monthly_reports::routes())"
    ));

    let test = read(&root, "tests/requests/monthly_reports.rs");
    assert!(test.contains("let page = inertia_get(&server, &ctx, &acme(\"summary\")).await;"));
    assert!(test.contains("async fn a_non_member_gets_404()"));
    assert!(test.contains("assert_eq!(page[\"component\"], \"monthly_reports/year_end\");"));
    assert!(test.contains("route_table::SIGN_IN"));

    // The pages, the way `scaffold:pages` writes a resource's.
    let written = scaffold_pages::generate_controller(&root, "monthly_reports").unwrap();
    assert_eq!(
        written,
        [
            "frontend/pages/monthly_reports/index.tsx",
            "frontend/pages/monthly_reports/summary.tsx",
            "frontend/pages/monthly_reports/year_end.tsx",
        ]
    );
    let page = read(&root, "frontend/pages/monthly_reports/year_end.tsx");
    assert!(
        page.contains("export default function MonthlyReportsYearEnd() {"),
        "{page}"
    );
    assert!(
        page.contains("href: routes.yearEnd(accountSlug).url"),
        "{page}"
    );
    assert!(page.contains("const { slug: accountSlug } = useCurrentAccount()"));
    assert!(page.contains("<Head title=\"Monthly reports: year end\" />"));
    assert!(
        !page.contains("{{") && !page.contains("{%"),
        "no unrendered Tera"
    );
    assert!(
        scaffold_pages::generate_controller(&root, "monthly_reports")
            .unwrap()
            .is_empty(),
        "a second run overwrites nothing"
    );

    // A write action with another name than Rails' is refused, naming them.
    let err = generate_controller(&root, "generate controller imports publish").unwrap();
    assert!(
        err.contains("ImportsController"),
        "a GET page action is fine: {err}"
    );
    let err = generate_controller(&root, "generate controller exports delete").unwrap_err();
    assert!(
        err.contains("`delete`: write actions are `create`, `update` and `destroy`"),
        "{err}"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
#[serial]
fn generate_controller_writes_redirect_back_write_actions() {
    let root = app_copy("writes");
    let messages =
        generate_controller(&root, "generate controller notes create update destroy").unwrap();
    assert!(messages.contains("under /{account_slug}"), "{messages}");
    assert!(messages.contains("redirect back"), "{messages}");

    let controller = read(&root, "src/controllers/notes.rs");
    for action in ["create", "update", "destroy"] {
        assert!(
            controller.contains(&format!(
                "async fn {action}(\n    _: NoPrecognition,\n    current: CurrentAccount,"
            )),
            "{action} refuses precognition and is account-scoped:\n{controller}"
        );
    }
    assert!(controller.contains(
        "let back = Redirect::back(&headers, route_table::notes_path(&current.account.slug));"
    ));
    assert!(controller.contains("return Ok(back.errors(errors).into_response());"));
    assert!(controller.contains("struct NoteParams {}"));
    // Write actions only: no index page, test or GET route (Rails' `resources :notes, only:
    // %i[create update destroy]`).
    assert!(compact(&controller).contains(&compact(
        ".add(route_table::NOTES, post(create)) .add(route_table::NOTE, patch(update).put(update).delete(destroy))"
    )), "{controller}");
    assert!(!controller.contains("async fn index") && !controller.contains("render("));

    let table = read(&root, "src/route_table.rs");
    assert!(table.contains("pub const NOTES: &str = \"/{account_slug}/notes\";"));
    assert!(table.contains("pub const NOTE: &str = \"/{account_slug}/notes/{id}\";"));
    assert!(
        !compact(&table).contains("route(\"notes.index\""),
        "no index route"
    );
    assert!(table.contains("pub fn note_path(slug: &str, id: i64) -> String"));
    let flat = compact(&table);
    for (action, method, path) in [
        ("create", "Post", "NOTES"),
        ("update", "Patch", "NOTE"),
        ("destroy", "Delete", "NOTE"),
    ] {
        assert!(
            flat.contains(&format!("route(\"notes.{action}\",{method},{path},ts(\"NotesController\",\"notes\",\"{action}\",None)),")),
            "{action}"
        );
    }

    let test = read(&root, "tests/requests/notes.rs");
    assert!(!test.contains("index_renders_its_page"), "{test}");
    assert!(test.contains("assert_redirect(&server.post(&acme(\"\")).json(&json!({})).await, route_table::SIGN_IN);"), "{test}");
    assert!(test.contains("async fn write_actions_redirect_back_and_refuse_precognition()"));
    assert!(test.contains("assert_redirect(&res, \"/back-here\");"));
    assert!(test.contains("assert_eq!(res.status_code(), 400, \"destroy refuses precognition\");"));
    assert!(test.contains(".delete(&route_table::note_path(\"globex\", 1))"));

    assert!(
        scaffold_pages::generate_controller(&root, "notes")
            .unwrap_err()
            .contains("renders no"),
        "and so no pages to write"
    );

    // `update`/`destroy` alone: no collection path; they redirect back to the account.
    generate_controller(&root, "generate controller tags update destroy").unwrap();
    let controller = read(&root, "src/controllers/tags.rs");
    assert!(controller.contains(
        "let back = Redirect::back(&headers, route_table::account_path(&current.account.slug));"
    ));
    let table = read(&root, "src/route_table.rs");
    assert!(!table.contains("pub const TAGS:") && !table.contains("fn tags_path"));
    assert!(table.contains("pub const TAG: &str = \"/{account_slug}/tags/{id}\";"));

    // `--global`: signed in, no account.
    generate_controller(&root, "generate controller exports create --global").unwrap();
    let controller = read(&root, "src/controllers/exports.rs");
    assert!(controller.contains("_: Authenticated,"));
    assert!(!controller.contains("CurrentAccount"));
    assert!(read(&root, "src/route_table.rs").contains("pub const EXPORTS: &str = \"/exports\";"));
    assert!(!read(&root, "tests/requests/exports.rs").contains("globex"));

    std::fs::remove_dir_all(&root).unwrap();
}

/// `show:<param>` is the member page at `<name>/{param}`; `show:*param` a glob (the rest of the
/// path, slashes kept), Rails' `get "folders/*path"`.
#[test]
#[serial]
fn generate_controller_writes_a_member_show_page_with_its_param() {
    let root = app_copy("member");
    generate_controller(&root, "generate controller folders show:*path").unwrap();
    let table = read(&root, "src/route_table.rs");
    assert!(table.contains("pub const FOLDER: &str = \"/{account_slug}/folders/{*path}\";"));
    assert!(table.contains("pub fn folder_path(slug: &str, path: &str) -> String"));
    assert!(table.contains("path.split('/').map(encode_segment).collect()"));
    assert!(compact(&table).contains(
        "route(\"folders.show\",Get,FOLDER,ts(\"FoldersController\",\"folders\",\"show\",None)),"
    ));
    let controller = read(&root, "src/controllers/folders.rs");
    assert!(
        controller.contains("Path((_, path)): Path<(String, String)>,"),
        "{controller}"
    );
    assert!(
        controller.contains("render(inertia, \"folders/show\", json!({ \"path\": path })).await")
    );
    assert!(
        compact(&controller).contains(&compact(
            ".add(route_table::FOLDERS, get(index)) .add(route_table::FOLDER, get(show))"
        )),
        "{controller}"
    );
    let test = read(&root, "tests/requests/folders.rs");
    assert!(test.contains(
        "inertia_get(&server, &ctx, &route_table::folder_path(\"acme\", \"a/b c\")).await;"
    ));
    assert!(test.contains(
        "assert_eq!(page[\"props\"][\"path\"], \"a/b c\", \"the glob keeps its slashes\");"
    ));

    // The show page breadcrumbs to the index only (its own URL needs the param).
    let written = scaffold_pages::generate_controller(&root, "folders").unwrap();
    assert_eq!(
        written,
        [
            "frontend/pages/folders/index.tsx",
            "frontend/pages/folders/show.tsx"
        ]
    );
    let page = read(&root, "frontend/pages/folders/show.tsx");
    assert!(
        page.contains("href: routes.index(accountSlug).url"),
        "{page}"
    );
    assert!(!page.contains("routes.show"), "{page}");

    // `show:id` with update and destroy: one member path, `{id}`, an i64.
    generate_controller(
        &root,
        "generate controller labels show:id update destroy --global",
    )
    .unwrap();
    let table = read(&root, "src/route_table.rs");
    assert!(table.contains("pub const LABEL: &str = \"/labels/{id}\";"));
    assert!(table.contains("pub fn label_path(id: i64) -> String"));
    let controller = read(&root, "src/controllers/labels.rs");
    assert!(controller.contains("Path(id): Path<i64>,"), "{controller}");
    assert!(
        compact(&controller).contains(&compact(
            ".add(route_table::LABEL, get(show).patch(update).put(update).delete(destroy))"
        )),
        "{controller}"
    );

    // A string param next to update/destroy (which take `{id}`) is refused, and so is a param
    // on anything but `show`.
    let err =
        generate_controller(&root, "generate controller badges show:slug destroy").unwrap_err();
    assert!(
        err.contains("update and destroy take the record's `{id}`"),
        "{err}"
    );
    let err = generate_controller(&root, "generate controller badges edit:id").unwrap_err();
    assert!(
        err.contains("only a flat controller's `show` takes a param"),
        "{err}"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

/// `cargo loco generate worker|task` with the kit's templates (.loco-templates/{worker,task}/).
fn generate_component(root: &Path, component: loco_gen::Component) -> String {
    std::env::set_current_dir(root).unwrap();
    let results = loco_gen::generate(
        &loco_gen::RRgen::with_working_dir(root),
        component,
        &loco_gen::AppInfo {
            app_name: env!("CARGO_PKG_NAME").into(),
            working_dir: root.to_path_buf(),
        },
    );
    std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).unwrap();
    loco_gen::collect_messages(&results.expect("the templates render"))
}

/// The generated worker test keeps compiling when `WorkerArgs` gets a field (it builds the
/// args from `Default`), and the worker and task tests load the seeds first, like model tests.
/// The --ignored build test compiles and runs both with a field and a seeded-row lookup.
#[test]
#[serial]
fn generate_worker_and_task_tests_seed_and_survive_new_args() {
    let root = app_copy("jobs");
    let messages = generate_component(
        &root,
        loco_gen::Component::Worker {
            name: "digest".into(),
        },
    );
    for template in ["worker/worker.t", "worker/test.t"] {
        assert!(
            messages.contains(&format!(".loco-templates/{template}")),
            "{messages}"
        );
    }
    let worker = read(&root, "src/workers/digest.rs");
    assert!(worker
        .contains("#[derive(Deserialize, Debug, Default, Serialize)]\npub struct WorkerArgs {"));
    let test = read(&root, "tests/workers/digest.rs");
    assert!(test.contains("let args = WorkerArgs::default();"), "{test}");
    assert!(!test.contains("WorkerArgs {}"));
    assert!(test.contains("seed::<App>(&boot.app_context).await.unwrap();"));

    let messages = generate_component(
        &root,
        loco_gen::Component::Task {
            name: "prune".into(),
        },
    );
    assert!(
        messages.contains(".loco-templates/task/test.t"),
        "{messages}"
    );
    let test = read(&root, "tests/tasks/prune.rs");
    let seeded = test
        .find("seed::<App>(&boot.app_context).await.unwrap();")
        .expect("seeds");
    assert!(
        seeded < test.find("run_task::<App>").unwrap(),
        "seeded before the task runs"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
#[serial]
fn generate_controller_writes_a_nested_singular_resource() {
    let root = app_copy("nested");
    let messages = generate_controller(
        &root,
        "generate controller gadgets/approvals create destroy",
    )
    .unwrap();
    assert!(
        messages.contains("Gadgets::ApprovalsController"),
        "{messages}"
    );

    // Rails' `Gadgets::ApprovalsController`: its own module under the parent's.
    let controller = read(&root, "src/controllers/gadgets/approvals.rs");
    assert!(read(&root, "src/controllers/gadgets/mod.rs").contains("pub mod approvals;"));
    assert!(read(&root, "src/controllers/mod.rs").contains("pub mod gadgets;"));
    assert!(
        read(&root, "src/app.rs").contains(".add_route(controllers::gadgets::approvals::routes())")
    );
    assert!(controller
        .contains("async fn create(\n    _: NoPrecognition,\n    current: CurrentAccount,"));
    assert!(controller.contains(".add(route_table::GADGET_APPROVAL, post(create).delete(destroy))"));
    // No `gadgets` model here, so the handler can't look the gadget up for you; it says how.
    assert!(controller.contains("Look the gadget up in the account here (`find_in_account`)"));

    let table = read(&root, "src/route_table.rs");
    assert!(table.contains(
        "pub const GADGET_APPROVAL: &str = \"/{account_slug}/gadgets/{gadget_id}/approval\";"
    ));
    assert!(table.contains("pub fn gadget_approval_path(slug: &str, gadget_id: i64) -> String"));
    assert!(compact(&table).contains("route(\"gadgets.approvals.create\",Post,GADGET_APPROVAL,ts(\"Gadgets/ApprovalsController\",\"gadgetsApprovals\",\"create\",None)),"));

    let test = read(&root, "tests/requests/gadgets_approvals.rs");
    assert!(read(&root, "tests/requests/mod.rs").contains("mod gadgets_approvals;"));
    assert!(test.contains(".post(&route_table::gadget_approval_path(\"acme\", gadget))"));
    assert!(test.contains("async fn other_accounts_are_not_found()"));

    // Pages belong to the parent's controller.
    let err = generate_controller(&root, "generate controller gadgets/archives show").unwrap_err();
    assert!(
        err.contains("a nested controller (`gadgets/archives`) writes create/update/destroy only"),
        "{err}"
    );

    std::fs::remove_dir_all(&root).unwrap();
}

fn run(root: &Path, program: &str, args: &[&str], target_dir: &Path) {
    let out = Command::new(program)
        .args(args)
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target_dir)
        .output()
        .unwrap_or_else(|e| panic!("{program}: {e}"));
    assert!(
        out.status.success(),
        "{program} {args:?} failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[serial]
#[ignore = "builds a full copy of the app (minutes); bin/ci and CI run it with --ignored"]
fn generated_code_builds_and_passes_its_tests() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(
        source.join("node_modules/.bin/tsc").exists(),
        "needs node_modules (npm ci) to type-check the generated pages"
    );
    let root = app_copy("build");
    #[cfg(unix)]
    std::os::unix::fs::symlink(source.join("node_modules"), root.join("node_modules")).unwrap();

    // Kept between runs (and cached in CI under target/), so only the first run is cold.
    let target_dir = source.join("target/generator-check");
    let cargo = env!("CARGO");
    // The real CLI in the copy: `generate scaffold` migrates and runs `db entities`, which
    // needs sea-orm-cli on PATH (`cargo install --locked sea-orm-cli@2.0.4`).
    let loco = |args: &[&str]| {
        let mut full = vec!["loco"];
        full.extend_from_slice(args);
        run(&root, cargo, &full, &target_dir);
    };
    // Generate every model first, then the rest (each generator rebuilds the CLI).
    loco(&[
        "generate",
        "scaffold",
        "widgets",
        "name:string!",
        "notes:text",
        "archived_at:tstz",
    ]);
    loco(&[
        "generate",
        "scaffold",
        "gizmos",
        "label:string!",
        "--global",
    ]);
    loco(&[
        "generate",
        "controller",
        "monthly_reports",
        "summary",
        "year_end",
    ]);
    loco(&[
        "generate",
        "controller",
        "notes",
        "create",
        "update",
        "destroy",
    ]);
    loco(&[
        "generate",
        "controller",
        "widgets/approvals",
        "create",
        "destroy",
    ]);
    loco(&["generate", "controller", "folders", "show:*path"]);
    loco(&["generate", "controller", "pings", "create"]);
    loco(&["generate", "channel", "widgets"]);
    // A worker whose args get a field, and a task that needs a seeded row: their generated tests
    // must build and pass as they are.
    loco(&["generate", "worker", "digest"]);
    let worker = read(&root, "src/workers/digest.rs");
    std::fs::write(
        root.join("src/workers/digest.rs"),
        worker.replace(
            "pub struct WorkerArgs {\n}",
            "pub struct WorkerArgs {\n    pub account_id: i64,\n}",
        ),
    )
    .unwrap();
    loco(&["generate", "task", "touch_acme"]);
    let task = read(&root, "src/tasks/touch_acme.rs");
    std::fs::write(
        root.join("src/tasks/touch_acme.rs"),
        task.replace(
            "async fn run(&self, _app_context: &AppContext, _vars: &task::Vars) -> Result<()> {",
            "async fn run(&self, app_context: &AppContext, _vars: &task::Vars) -> Result<()> {\n        crate::models::accounts::Model::find_by_slug(&app_context.db, \"acme\").await?;",
        ),
    )
    .unwrap();
    for task in [
        "resource:widgets",
        "resource:gizmos",
        "controller:monthly_reports",
        "controller:folders",
    ] {
        loco(&["task", "scaffold:pages", task]);
    }

    run(&root, cargo, &["fmt", "--all"], &target_dir);
    // The copy's own task: this binary's route table is the unmodified app's.
    loco(&["task", "routes:generate"]);
    run(
        &root,
        cargo,
        &["clippy", "--all-targets", "--", "-D", "warnings"],
        &target_dir,
    );
    for module in [
        "requests::widgets",
        "requests::gizmos",
        "requests::monthly_reports",
        "requests::notes",
        "requests::widgets_approvals",
        "requests::widgets_channel",
        "requests::folders",
        "requests::pings",
        "workers::digest",
        "tasks::touch_acme",
    ] {
        run(
            &root,
            cargo,
            &["test", "--test", "mod", module],
            &target_dir,
        );
    }
    run(
        &root,
        cargo,
        &["test", "--test", "routes_fresh"],
        &target_dir,
    );
    run(
        &root,
        "npx",
        &[
            "prettier",
            "--write",
            "--log-level",
            "warn",
            "frontend/pages",
            "frontend/components/app-sidebar.tsx",
        ],
        &target_dir,
    );
    run(&root, "npm", &["run", "--silent", "check"], &target_dir);
    run(&root, "npm", &["run", "--silent", "lint"], &target_dir);

    // `generate scaffold` regenerated every entity (`db entities`). The kit's own `Model`s (their
    // columns and `#[sea_orm(..)]` attributes) came back byte for byte, so an app that commits
    // after generating loses nothing; only relations to the new tables (`has_many widgets` on
    // accounts) are added.
    let model = |source: &str| {
        let start = source.find("pub struct Model {").expect("a Model struct");
        source[start..start + source[start..].find("\n}").unwrap()].to_owned()
    };
    for entry in std::fs::read_dir(source.join("src/models/_entities")).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        if name != "mod.rs" && name != "prelude.rs" {
            let rel = format!("src/models/_entities/{name}");
            assert_eq!(
                model(&read(&root, &rel)),
                model(&read(source, &rel)),
                "{rel}: `cargo loco db entities` rewrote the committed Model"
            );
        }
    }

    // The scoped scaffold's skipped column stayed out of the pages; the nested controller's
    // handlers look the widget up in the account.
    assert!(!read(&root, "frontend/pages/widgets/form.tsx").contains("archived_at"));
    assert!(read(&root, "src/controllers/widgets/approvals.rs")
        .contains("widgets::Model::find_in_account(&ctx.db, current.account.id, widget_id)"));

    std::fs::remove_dir_all(&root).unwrap();
}
