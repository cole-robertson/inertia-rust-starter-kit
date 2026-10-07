//! What the kit changes in `cargo loco generate` before Loco parses the command line.
//!
//! Accounts are core in this kit, so generated resources and controllers live under
//! `/{account_slug}` by default:
//!
//! - `generate scaffold widgets name:string!` gets an `account:references` column (unless it
//!   names one), which the kit's scaffold templates turn into an account-scoped resource.
//! - `generate scaffold|controller … --global` is the escape hatch: the flag is removed (Loco's
//!   own parser would reject it) and the resource or controller is left outside accounts.
//! - `generate controller todos/completions create destroy` is a nested controller (Rails'
//!   `Todos::CompletionsController`): `src/controllers/todos/completions.rs`, a singular
//!   resource at `/{account_slug}/todos/{todo_id}/completion`. Its parent module is created
//!   here, because the generator can only inject into files that exist.
//!
//! The templates read the choices from environment variables (Tera's `get_env`), listed in
//! [`Plan::env`]. `src/bin/main.rs` calls [`plan`] and runs the binary again with the result.

use std::path::Path;

use cruet::Inflector;

/// `account` or `global`: whether generated code lives under `/{account_slug}`.
pub const SCOPE_ENV: &str = "KIT_GENERATE_SCOPE";
/// Comma-separated tables that have an entity in `src/models/_entities/`.
pub const TABLES_ENV: &str = "KIT_GENERATE_TABLES";
/// Comma-separated tables whose entity has a required `account_id`.
pub const ACCOUNT_TABLES_ENV: &str = "KIT_GENERATE_ACCOUNT_TABLES";
/// For a nested controller (`todos/completions`): the parent resource, plural (`todos`).
pub const PARENT_ENV: &str = "KIT_GENERATE_PARENT";
/// … its singular (`todo`), which names the path param (`{todo_id}`).
pub const PARENT_SINGULAR_ENV: &str = "KIT_GENERATE_PARENT_SINGULAR";
/// … the nested resource's singular (`completion`), the last path segment.
pub const CHILD_SINGULAR_ENV: &str = "KIT_GENERATE_CHILD_SINGULAR";
/// … `1` when the parent is an account-scoped model with `find_in_account` (the generated
/// handlers then look it up in the account, so another account's id is a 404).
pub const PARENT_SCOPED_ENV: &str = "KIT_GENERATE_PARENT_SCOPED";
/// A flat controller's singular (`notes` → `note`), which names its member route (`NOTE`,
/// `/notes/{id}`).
pub const SINGULAR_ENV: &str = "KIT_GENERATE_SINGULAR";
/// … the file the `pub mod completions;` line goes into (`src/controllers/todos/mod.rs`, or
/// `src/controllers/todos.rs` when the parent is a flat module).
pub const PARENT_MODULE_ENV: &str = "KIT_GENERATE_PARENT_MODULE";

/// How to run the generator.
#[derive(Debug, PartialEq, Eq)]
pub struct Plan {
    pub args: Vec<String>,
    /// The variables the templates read.
    pub env: Vec<(&'static str, String)>,
    /// One line for the terminal when the command line changed.
    pub note: Option<String>,
}

/// The kit's plan for `generate scaffold|controller` in the app at `root`, or `None` for any
/// other command. For a nested controller it creates the parent module file (and its `pub mod`
/// line in `src/controllers/mod.rs`) when missing.
///
/// # Errors
/// A message when the arguments contradict each other (`--global` with `account:references`)
/// or a nested name is malformed, or when the parent module can't be written.
pub fn plan(args: &[String], root: &Path) -> Result<Option<Plan>, String> {
    let Some((kind, name)) = generator(args) else {
        return Ok(None);
    };
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(None);
    }
    let global = args.iter().any(|a| a == "--global");
    let has_account = args
        .iter()
        .any(|a| a.split_once(':').is_some_and(|(name, _)| name == "account"));
    let mut args: Vec<String> = args.iter().filter(|a| *a != "--global").cloned().collect();
    let mut note = None;
    match (kind, global) {
        ("scaffold", true) if has_account => {
            return Err(
                "`--global` and an `account` column contradict each other: drop one (a global \
                 resource has no account)"
                    .to_owned(),
            )
        }
        (_, true) => {
            note = Some(format!(
                "generate {kind}: global, outside accounts (--global)"
            ))
        }
        ("scaffold", false) if !has_account => {
            args.push("account:references".to_owned());
            note = Some(
                "generate scaffold: scoped to the account (added `account:references`; pass \
                 --global for a resource outside accounts)"
                    .to_owned(),
            );
        }
        _ => {}
    }
    let tables = entity_tables(root);
    let mut env = vec![
        (
            SCOPE_ENV,
            if global { "global" } else { "account" }.to_owned(),
        ),
        (
            TABLES_ENV,
            tables
                .iter()
                .map(|(t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
        (
            ACCOUNT_TABLES_ENV,
            tables
                .iter()
                .filter(|(_, scoped)| *scoped)
                .map(|(t, _)| t.as_str())
                .collect::<Vec<_>>()
                .join(","),
        ),
    ];
    if kind == "controller" {
        match name.split_once('/') {
            Some((parent, child)) => env.extend(prepare_nested(root, parent, child)?),
            None => env.push((SINGULAR_ENV, name.to_singular())),
        }
    }
    Ok(Some(Plan { args, env, note }))
}

/// The parent module of `parent/child` (created when missing) and the names the templates
/// need.
fn prepare_nested(
    root: &Path,
    parent: &str,
    child: &str,
) -> Result<Vec<(&'static str, String)>, String> {
    let valid = |s: &str| {
        !s.is_empty()
            && s.starts_with(|c: char| c.is_ascii_lowercase())
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    };
    if !valid(parent) || !valid(child) {
        return Err(format!(
            "a nested controller is `parent/child` in snake_case, e.g. `todos/completions` (got \
             `{parent}/{child}`)"
        ));
    }
    let flat = format!("src/controllers/{parent}.rs");
    let dir_module = format!("src/controllers/{parent}/mod.rs");
    let module = if root.join(&flat).exists() {
        flat
    } else {
        if !root.join(&dir_module).exists() {
            std::fs::create_dir_all(root.join(format!("src/controllers/{parent}")))
                .map_err(|e| e.to_string())?;
            std::fs::write(
                root.join(&dir_module),
                format!(
                    "//! Controllers nested under {parent} (`cargo loco generate controller \
                     {parent}/<name>`).\n"
                ),
            )
            .map_err(|e| e.to_string())?;
        }
        let mod_rs = root.join("src/controllers/mod.rs");
        let source = std::fs::read_to_string(&mod_rs).map_err(|e| e.to_string())?;
        let line = format!("pub mod {parent};");
        if !source.lines().any(|l| l.trim() == line) {
            std::fs::write(&mod_rs, format!("{}\n{line}\n", source.trim_end()))
                .map_err(|e| e.to_string())?;
        }
        dir_module
    };
    let scoped_finder = std::fs::read_to_string(root.join(format!("src/models/{parent}.rs")))
        .is_ok_and(|model| model.contains("pub async fn find_in_account("));
    Ok(vec![
        (
            PARENT_SCOPED_ENV,
            if scoped_finder { "1" } else { "" }.to_owned(),
        ),
        (PARENT_ENV, parent.to_owned()),
        (PARENT_SINGULAR_ENV, parent.to_singular()),
        (CHILD_SINGULAR_ENV, child.to_singular()),
        (PARENT_MODULE_ENV, module),
    ])
}

/// `cargo loco generate channel <name>` (Loco has none): render `.loco-templates/channel/` under
/// `root` for `<Name>Channel`, returning the generator's messages and the client snippet to
/// print, or `None` when `args` is another command.
///
/// # Errors
/// A malformed name, or a template or injection that fails.
pub fn channel(args: &[String], root: &Path, app_name: &str) -> Result<Option<String>, String> {
    let positional: Vec<&str> = args
        .iter()
        .skip(1)
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let name = match positional.as_slice() {
        ["generate" | "g", "channel", name] => *name,
        ["generate" | "g", "channel", ..] => {
            return Err("usage: cargo loco generate channel <name>, e.g. projects".to_owned())
        }
        _ => return Ok(None),
    };
    let file_name = name.trim_end_matches("_channel").to_snake_case();
    if file_name.is_empty() || !file_name.starts_with(|c: char| c.is_ascii_lowercase()) {
        return Err(format!(
            "`{name}` is not a channel name; try e.g. `projects`"
        ));
    }
    let channel = file_name.to_pascal_case() + "Channel";
    let vars = serde_json::json!({
        "file_name": file_name,
        "channel": channel,
        "pkg_name": app_name,
    });
    let rrgen = loco_gen::RRgen::with_working_dir(root);
    let mut messages = Vec::new();
    for template in ["channel.t", "test.t"] {
        let path = root.join(".loco-templates/channel").join(template);
        let source =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        match rrgen.generate(&source, &vars).map_err(|e| e.to_string())? {
            loco_gen::GenResult::Generated { message: Some(m) } => messages.push(format!("* {m}")),
            loco_gen::GenResult::Generated { message: None } => {}
            loco_gen::GenResult::Skipped => {
                messages.push(format!(
                    "* skipped .loco-templates/channel/{template}: the file exists"
                ));
            }
        }
    }
    messages.push(format!(
        "\nIn a page (frontend/lib/live.ts):\n\n  import {{ useLiveReload }} from \"@/lib/live\"\n  \
         useLiveReload(\"{channel}\", {{ account: slug, id }}, {{ only: [\"…\"] }})\n\n\
         and after a write commits:\n\n  {channel}::broadcast_to(record.account_id, record.id, json!({{ \"type\": \"changed\" }}));"
    ));
    Ok(Some(messages.join("\n")))
}

/// Every table with an entity under `root`, and whether that entity has a required
/// `account_id`, sorted by table.
#[must_use]
pub fn entity_tables(root: &Path) -> Vec<(String, bool)> {
    let Ok(entries) = std::fs::read_dir(root.join("src/models/_entities")) else {
        return Vec::new();
    };
    let mut tables: Vec<(String, bool)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let table = path.file_stem()?.to_str()?.to_owned();
            let source = std::fs::read_to_string(&path).ok()?;
            source.contains("pub struct Model").then(|| {
                let scoped = source
                    .lines()
                    .any(|line| line.trim() == "pub account_id: i64,");
                (table, scoped)
            })
        })
        .collect();
    tables.sort();
    tables
}

/// What to say after `cargo loco generate migration <name>` succeeds, or `None` for any other
/// command. Loco's own message names `db migrate && db entities`, but not that (unlike
/// `generate model`) it ran neither, nor that once code uses the new columns the CLI that would
/// regenerate the entities no longer builds.
#[must_use]
pub fn migration_next_steps(args: &[String]) -> Option<&'static str> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return None;
    }
    matches!(
        first_positionals(args).as_slice(),
        ["generate" | "g", "migration", _]
    )
    .then_some(
        "Not run yet: `cargo loco db migrate && cargo loco db entities`. Run both before any \
         code uses the new columns (the CLI has to build to run them), then commit \
         src/models/_entities/. Edit the migration first if it needs an index or a unique key; \
         to change it after it ran: `cargo loco db down`, edit, then both again. \
         tests/entities_fresh.rs fails while the committed entities are stale.",
    )
}

/// `("scaffold" | "controller", name)` when `args` is `generate|g scaffold|controller <name>
/// …`.
fn generator(args: &[String]) -> Option<(&'static str, &str)> {
    match first_positionals(args).as_slice() {
        ["generate" | "g", "scaffold", name, ..] => Some(("scaffold", name)),
        ["generate" | "g", "controller", name, ..] => Some(("controller", name)),
        _ => None,
    }
}

/// The first three positional arguments after the binary, skipping flags and Loco's global
/// `-e/--environment <env>` wherever it sits.
fn first_positionals(args: &[String]) -> Vec<&str> {
    let mut positional = Vec::new();
    let mut rest = args.iter().skip(1);
    while let Some(arg) = rest.next() {
        if arg == "-e" || arg == "--environment" {
            rest.next();
        } else if !arg.starts_with('-') {
            positional.push(arg.as_str());
            if positional.len() == 3 {
                break;
            }
        }
    }
    positional
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split(' ').map(str::to_owned).collect()
    }

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn env_of<'a>(plan: &'a Plan, key: &str) -> &'a str {
        plan.env
            .iter()
            .find(|(k, _)| *k == key)
            .map_or("", |(_, v)| v.as_str())
    }

    #[test]
    fn a_scaffold_is_account_scoped_by_default() {
        let out = plan(&args("cli generate scaffold widgets name:string!"), root())
            .unwrap()
            .unwrap();
        assert_eq!(
            out.args,
            args("cli generate scaffold widgets name:string! account:references")
        );
        assert_eq!(env_of(&out, SCOPE_ENV), "account");
        assert!(out.note.is_some());
    }

    #[test]
    fn an_explicit_account_column_is_left_as_written() {
        for line in [
            "cli g scaffold widgets account:references name:string!",
            "cli generate scaffold widgets account:references?",
        ] {
            let out = plan(&args(line), root()).unwrap().unwrap();
            assert_eq!(out.args, args(line));
            assert_eq!(env_of(&out, SCOPE_ENV), "account");
            assert_eq!(out.note, None);
        }
    }

    #[test]
    fn global_is_removed_and_passed_as_the_scope() {
        let out = plan(
            &args("cli -e test generate scaffold gizmos --global name:string!"),
            root(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            out.args,
            args("cli -e test generate scaffold gizmos name:string!")
        );
        assert_eq!(env_of(&out, SCOPE_ENV), "global");
        let out = plan(
            &args("cli generate controller reports summary --global"),
            root(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(out.args, args("cli generate controller reports summary"));
        assert_eq!(env_of(&out, SCOPE_ENV), "global");
    }

    #[test]
    fn generate_channel_writes_registers_and_tests_a_channel() {
        let tmp = std::env::temp_dir().join(format!("irsk-channel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for dir in ["src/channels", "tests/requests", ".loco-templates/channel"] {
            std::fs::create_dir_all(tmp.join(dir)).unwrap();
        }
        for t in ["channel.t", "test.t"] {
            std::fs::copy(
                root().join(".loco-templates/channel").join(t),
                tmp.join(".loco-templates/channel").join(t),
            )
            .unwrap();
        }
        std::fs::copy(
            root().join("src/channels/mod.rs"),
            tmp.join("src/channels/mod.rs"),
        )
        .unwrap();
        std::fs::write(
            tmp.join("tests/requests/mod.rs"),
            "mod accounts;\nmod live;\n\nuse x;\n",
        )
        .unwrap();

        let out = channel(&args("cli generate channel todo_lists"), &tmp, "app")
            .unwrap()
            .unwrap();
        assert!(out.contains("Channel `TodoListsChannel`"), "{out}");
        assert!(out.contains("useLiveReload(\"TodoListsChannel\""), "{out}");
        let source = std::fs::read_to_string(tmp.join("src/channels/todo_lists.rs")).unwrap();
        assert!(source.contains("pub struct TodoListsChannel;"));
        assert!(source.contains("impl Channel for TodoListsChannel {"));
        let registry = std::fs::read_to_string(tmp.join("src/channels/mod.rs")).unwrap();
        // After the last `pub mod` line, whatever channels the app already has.
        let mods: Vec<&str> = registry
            .lines()
            .filter(|l| l.starts_with("pub mod "))
            .collect();
        assert_eq!(mods.last(), Some(&"pub mod todo_lists;"), "{registry}");
        assert!(registry.contains(
            "        Arc::new(todo_lists::TodoListsChannel),\n        // channels-inject"
        ));
        let test =
            std::fs::read_to_string(tmp.join("tests/requests/todo_lists_channel.rs")).unwrap();
        assert!(test.contains("use app::{"), "{test}");
        assert!(test.contains("async fn a_non_member_is_rejected_and_receives_nothing()"));
        assert!(test.contains("async fn the_same_id_in_another_account_is_not_received()"));
        // The stream key carries the account, so a bare id never crosses accounts.
        assert!(
            source.contains("stream_for(Self::key(membership.account_id, id))"),
            "{source}"
        );
        let mods = std::fs::read_to_string(tmp.join("tests/requests/mod.rs")).unwrap();
        assert!(
            mods.starts_with("mod accounts;\nmod live;\nmod todo_lists_channel;\n"),
            "{mods}"
        );

        // `_channel` on the name is fine; a second run overwrites nothing.
        let again = channel(&args("cli g channel todo_lists_channel"), &tmp, "app")
            .unwrap()
            .unwrap();
        assert!(again.contains("skipped"), "{again}");
        assert_eq!(
            channel(&args("cli generate channel"), &tmp, "app").unwrap_err(),
            "usage: cargo loco generate channel <name>, e.g. projects"
        );
        assert_eq!(
            channel(&args("cli generate scaffold x"), &tmp, "app").unwrap(),
            None
        );
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn a_flat_controller_learns_its_singular() {
        let out = plan(&args("cli generate controller notes create update"), root())
            .unwrap()
            .unwrap();
        assert_eq!(env_of(&out, SINGULAR_ENV), "note");
        assert_eq!(env_of(&out, PARENT_ENV), "");
    }

    #[test]
    fn global_with_an_account_column_is_refused() {
        let err = plan(
            &args("cli g scaffold gizmos account:references --global"),
            root(),
        )
        .unwrap_err();
        assert!(err.contains("contradict"), "{err}");
    }

    #[test]
    fn other_commands_and_help_are_untouched() {
        for line in [
            "cli start",
            "cli generate model widgets name:string!",
            "cli generate scaffold --help",
            "cli task scaffold:pages resource:widgets",
            "cli db migrate",
        ] {
            assert_eq!(plan(&args(line), root()).unwrap(), None, "{line}");
        }
    }

    #[test]
    fn generate_migration_names_the_commands_that_regenerate_the_entities() {
        for line in [
            "cli generate migration AddViewsToPosts views:int!",
            "cli -e test g migration add_views_to_posts views:int",
        ] {
            let next = migration_next_steps(&args(line)).expect(line);
            assert!(
                next.contains("cargo loco db migrate && cargo loco db entities"),
                "{next}"
            );
        }
        for line in [
            "cli generate migration --help",
            "cli generate model posts title:string!",
            "cli db migrate",
        ] {
            assert_eq!(migration_next_steps(&args(line)), None, "{line}");
        }
    }

    #[test]
    fn the_templates_learn_which_tables_belong_to_an_account() {
        let out = plan(&args("cli g scaffold widgets name:string!"), root())
            .unwrap()
            .unwrap();
        let tables: Vec<&str> = env_of(&out, TABLES_ENV).split(',').collect();
        let scoped: Vec<&str> = env_of(&out, ACCOUNT_TABLES_ENV).split(',').collect();
        for table in ["accounts", "invitations", "memberships", "users"] {
            assert!(tables.contains(&table), "{table} in {tables:?}");
        }
        assert!(scoped.contains(&"memberships") && scoped.contains(&"invitations"));
        assert!(!scoped.contains(&"users") && !scoped.contains(&"accounts"));
    }

    #[test]
    fn a_nested_controller_gets_its_parent_module_and_singular_names() {
        let tmp = std::env::temp_dir().join(format!("irsk-nested-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("src/controllers")).unwrap();
        std::fs::write(tmp.join("src/controllers/mod.rs"), "pub mod home;\n").unwrap();

        let line = "cli generate controller todos/completions create destroy";
        let out = plan(&args(line), &tmp).unwrap().unwrap();
        assert_eq!(out.args, args(line));
        assert_eq!(env_of(&out, PARENT_ENV), "todos");
        assert_eq!(env_of(&out, PARENT_SINGULAR_ENV), "todo");
        assert_eq!(env_of(&out, CHILD_SINGULAR_ENV), "completion");
        assert_eq!(
            env_of(&out, PARENT_MODULE_ENV),
            "src/controllers/todos/mod.rs"
        );
        assert_eq!(env_of(&out, PARENT_SCOPED_ENV), "", "no todos model here");
        assert!(tmp.join("src/controllers/todos/mod.rs").exists());
        let mod_rs = std::fs::read_to_string(tmp.join("src/controllers/mod.rs")).unwrap();
        assert_eq!(mod_rs, "pub mod home;\npub mod todos;\n");
        // Again: nothing added twice.
        plan(&args(line), &tmp).unwrap();
        let again = std::fs::read_to_string(tmp.join("src/controllers/mod.rs")).unwrap();
        assert_eq!(again, mod_rs);

        // A flat parent module (`projects.rs`, e.g. a scaffold) takes the `pub mod` line itself,
        // and a scoped scaffold's model is looked up in the account.
        std::fs::write(tmp.join("src/controllers/projects.rs"), "// projects\n").unwrap();
        std::fs::create_dir_all(tmp.join("src/models")).unwrap();
        std::fs::write(
            tmp.join("src/models/projects.rs"),
            "pub async fn find_in_account(db: &C, account_id: i64, id: i64) {}\n",
        )
        .unwrap();
        let out = plan(&args("cli g controller projects/archives create"), &tmp)
            .unwrap()
            .unwrap();
        assert_eq!(
            env_of(&out, PARENT_MODULE_ENV),
            "src/controllers/projects.rs"
        );
        assert_eq!(env_of(&out, CHILD_SINGULAR_ENV), "archive");
        assert_eq!(env_of(&out, PARENT_SCOPED_ENV), "1");
        assert!(!tmp.join("src/controllers/projects").exists());

        let err = plan(&args("cli g controller Todos/Completions create"), &tmp).unwrap_err();
        assert!(err.contains("snake_case"), "{err}");
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
