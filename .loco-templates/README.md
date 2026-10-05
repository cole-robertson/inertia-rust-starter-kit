# Kit generator templates

`cargo loco generate` reads a template from this directory instead of its built-in copy when
one exists at the same path (Loco's `generate override` mechanism). These make the scaffold
produce code for this kit: Inertia pages, the route table, the session-cookie auth, Rails-worded
validation errors.

```sh
cargo loco generate scaffold posts title:string! body:text published:bool    # in the account
cargo loco task scaffold:pages resource:posts

cargo loco generate controller reports summary        # pages that aren't CRUD
cargo loco task scaffold:pages controller:reports
cargo loco generate controller notes create update destroy           # write actions
cargo loco generate controller todos/completions create destroy      # nested (Todos::Completions)

cargo loco generate scaffold gizmos label:string! --global           # outside accounts
```

Accounts are core in this kit, so generated code lives under `/{account_slug}` and takes
`CurrentAccount` unless `--global`. Loco's CLI can't take a new flag, so `src/bin/main.rs` plans
the command first (`src/generate.rs`): it adds `account:references` to a scaffold, removes
`--global`, creates a nested controller's parent module, and runs the generator with
`KIT_GENERATE_*` environment variables the templates read through Tera's `get_env` (the scope;
which tables exist and which belong to accounts; a nested controller's names). The templates
default to the account scope when run without them.

| Template | Replaces Loco's | Writes |
|---|---|---|
| `scaffold/api/controller.t` | JSON API controller | `src/controllers/<plural>.rs` (Inertia `render` + `Redirect`); injects `src/controllers/mod.rs`, `src/app.rs`, and the paths + routes into `src/route_table.rs` |
| `scaffold/api/dto.t` | the `ts-rs` DTO | `tests/requests/<plural>.rs`; fills `src/models/<plural>.rs` with `<Singular>Params`, casting + validation, finders, `create`/`update`/`destroy`, `to_props` |
| `model/test.t` | model test using `insta` | `tests/models/<plural>.rs` without `insta` (not a kit dependency) |
| `worker/{worker,test}.t` | the worker and its test | Loco's worker with `Default` derived on `WorkerArgs`, and a test that seeds and builds the args as `WorkerArgs::default()`, so it still compiles after you add fields (Loco's writes `WorkerArgs {}`) |
| `task/test.t` | the task test | Loco's, with the seeds loaded before the task runs (as `model/test.t` does); the task itself is Loco's |
| `mailer/mailer.t` | the mailer module | `src/mailers/<name>.rs`, the same as Loco's but sending from `settings.mail_from` (Loco's sends from `System <system@example.com>`); the `.t` message templates are Loco's |
| `controller/api/controller.t` | JSON API controller (`format::empty()` under `/api/<name>`) | `src/controllers/<name>.rs`: a GET page per read action (plus `index`, unless every action is a write), a member page for `show:<param>` (`show:id`, `show:slug`, or a glob `show:*path`, at `<name>/{param}`), and `create`/`update`/`destroy` write handlers that redirect back with errors and refuse Precognition; `CurrentAccount` (or `Authenticated` with `--global`); injects `src/controllers/mod.rs`, `src/app.rs`, and the paths + routes into `src/route_table.rs`. `parent/child` is a nested singular resource (`src/controllers/parent/child.rs`, `/{account_slug}/parent/{parent_id}/child`) |
| `controller/api/test.t` | a test expecting JSON at `/api/<name>` | `tests/requests/<name>.rs`: each page renders its component (`inertia_get`), each write redirects back and refuses Precognition, signed-out visitors go to sign in, a non-member gets 404; `mod` line with the others |
| `controller/pages/page.t` | (none) | `frontend/pages/<name>/<action>.tsx`, one per component the controller renders, by `cargo loco task scaffold:pages controller:<name>` |
| `channel/{channel,test}.t` | (none: Loco has no channel generator) | `cargo loco generate channel <name>`, rendered by `src/generate.rs`: `src/channels/<name>.rs` (registered in `src/channels/mod.rs`) and `tests/requests/<name>_channel.rs` |
| `scaffold/pages/*.t` | (none: Loco can't write this kit's React) | `frontend/pages/<plural>/{index,show,new,edit,form}.tsx`, rendered by `cargo loco task scaffold:pages` (`src/tasks/scaffold_pages.rs`) |

The migration, entity and `src/models/<plural>.rs` skeleton come from Loco's own `model`
templates unchanged.

Supported column types: `string`, `text`, `int`, `big_int`, `small_int`, `float`, `double`,
`bool`, `date`, `references` (and their `!`/`^`/`?` forms). Any other column, and a reference
to a table that doesn't exist, is left out of params, props and pages with a note naming it
(the same rule in `controller.t`, `dto.t` and `src/tasks/scaffold_pages.rs`). A required
`account_id` makes the resource account-scoped: it is never a param, `create` takes it from the
URL, and every finder filters by it.

A `references` column (an `i64` named `<association>_id`) is Rails' `belongs_to`: `dto.t`
casts it with `cast::reference`/`optional_reference` and checks the parent row exists before
saving (`"<association>": ["must exist"]`), and adds `<association>_options` (id + label) for
the form; `controller.t` passes those to new/edit; `pages/form.t` renders a shadcn/ui Select
(with "None" for `references?`). When the parent table belongs to accounts too, both the check
and the options are limited to the current account. The request test creates its own parent
rows. `user_id` is checked the same way but gets no select. `src/tasks/scaffold_pages.rs` recognises references by
the same rule.

`dto.t` edits the model file that `cargo loco db entities` wrote a moment earlier; its injection
anchors (`use sea_orm::entity::prelude::*;` and the `impl Model {}` placeholders) are that
file's current shape in Loco 1.2. `tests/scaffold_generator.rs` renders every template against
a copy of this app and fails if an anchor stops matching, so a Loco upgrade that changes them
shows up in `cargo test`, not in a user's first scaffold. `tests/controller_generator.rs` does
the same for `generate controller`, and its `--ignored` test (run by `bin/ci` and CI) runs the
real `cargo loco generate` for two scaffolds (scoped, `--global`) and three controllers (pages,
writes, nested) in a copy of the app, then clippy, their request tests, `tsc` and ESLint.

The templates are Tera 1 (rrgen). In the `.tsx` templates a JSX `{` directly before a Tera
`{{` is written `{ {{- x }}` or `{% raw %}{{% endraw %}`; `{{` alone would start a Tera tag.
