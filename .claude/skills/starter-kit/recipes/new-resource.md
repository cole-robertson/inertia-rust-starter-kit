# Recipe: a new resource (the generator)

**When:** the user wants a new kind of record with the usual pages: list, show, new, edit,
delete. Rails: `rails g scaffold`.

## Commands

```sh
# sea-orm-cli must be installed once: cargo install --locked sea-orm-cli@2.0.4
cargo loco generate scaffold projects name:string! description:text due_on:date archived:bool!
cargo loco task scaffold:pages resource:projects
```

**Account-scoped by default.** The kit adds `account:references` (it says so), so `projects`
belongs to an account and lives at `/{account_slug}/projects`. Pass `--global` for a resource
outside accounts (`/projects`, any signed-in user); write `account:references` yourself to
place the column. `--global` with an `account` column is refused.

**Several resources? Generate every model first, then migrate once.** Each `cargo loco
generate` rebuilds the CLI before it runs (about 2 minutes after a schema change), and each
`scaffold`/`model` migrates and regenerates entities. Run the `generate` commands back to back,
then the `scaffold:pages` tasks, then `cargo test`.

Name is **plural**. Column DSL: bare = nullable, `!` = required, `^` = unique + required;
`references` = required FK, `references?` = nullable FK. Full grammar in
`.claude/skills/loco/workflow.md`.

Supported by the kit's forms: `string`, `text`, `int`, `big_int`, `small_int`, `float`,
`double`, `bool`, `date`, `references`. Any other column (`tstz` like `archived_at`, `uuid`,
`json`, `decimal`, `enum:…`), and a `references` column whose table doesn't exist (say
`creator:references`; for a user write `users:references:creator_id`), is **left out** of the
params, the props and the pages, and the generator and `scaffold:pages` print a note naming
it. The column is still in the table: set it in the model (`create`, a model method) and add it
to `ProjectProps` and `to_props` if the page should see it. A required one must be set in `create`, or the insert
fails.

## What it writes

| File | From |
|---|---|
| `migration/src/m<ts>_projects.rs` (+ registered in `migration/src/lib.rs`) | Loco's model template |
| `src/models/_entities/projects.rs` | `cargo loco db entities` (run for you) |
| `src/models/projects.rs`: `ProjectParams` (never `account_id`), `assign` (casting + validation), `list(db, account_id)`, `find_in_account(db, account_id, id)`, `create(db, account_id, params)`, `update`, `destroy`, `to_props` and the `ProjectProps` struct (`Serialize` + `ts_rs::TS`, the entity's field types), registered in `src/page_types.rs` | `.loco-templates/scaffold/api/dto.t` |
| `src/controllers/projects.rs`: index/show/new/edit/create/update/destroy, `CurrentAccount` (a non-member gets 404), every lookup through the account, precognition on create/update | `.loco-templates/scaffold/api/controller.t` |
| `src/route_table.rs`: `PROJECTS = "/{account_slug}/projects"`, `NEW_PROJECT`, `PROJECT`, `EDIT_PROJECT`, `projects_path(slug)`, `new_project_path(slug)`, `project_path(slug, id)`, `edit_project_path(slug, id)`, seven routes | same |
| `src/controllers/mod.rs`, `src/app.rs` registration | same |
| `tests/requests/projects.rs` (sign-in redirect, full CRUD in Acme, **another account's project is a 404** through every action, blank-required errors) | `dto.t` |
| `tests/models/projects.rs` | `.loco-templates/model/test.t` |
| `frontend/pages/projects/{index,show,new,edit,form}.tsx`: the slug from `useCurrentAccount()`, `routes.show({ accountSlug, id })` | `scaffold:pages` from `.loco-templates/scaffold/pages/` |
| sidebar link in the account's nav (`projects.index(account.slug)`) in `frontend/components/app-sidebar.tsx`; `frontend/routes/*.ts`; `frontend/types/generated/ProjectProps.ts`, which the pages import | `scaffold:pages` |

With `--global` the same files have the unscoped shape: `Authenticated`, `find_by_id(db, id)`,
`/projects/{id}`, `project_path(id)`, and the sidebar link goes in the global list
(`// scaffold:nav-global`).

## After generating

- A `references` column is `belongs_to`: `ProjectParams` checks the parent row exists on
  create and update (`validate`), and a blank or missing one is `{"<association>": ["must
  exist"]}` (e.g. `project`, not `project_id`), redirected back to the form like any other
  validation error. Other than `user`, it is a shadcn/ui Select fed by
  `<Model>::<association>_options` (props `<association>_options` on new/edit); the label is
  the parent's `name`, else `title`, else `#id`, chosen in that function. `references?` adds a
  "None" choice. The request test creates parent rows itself (`create_parents`) and asserts
  the "must exist" error.
- **A parent that belongs to accounts too** (its table has `account_id`, e.g. `tasks
  project:references` after an account-scoped `projects`): the select lists only the current
  account's projects, and another account's project id is "must exist", not a cross-account
  link. The request test checks both (`a_parent_from_another_account_is_not_offered_…`).
- Scope further (to the owner, to a project's members) by adding to the model's finders: the
  generated ones are scoped to the account and nothing more.
- **Columns the app sets, not the user** (`last_status`, `last_error`, `synced_at` on a feed the
  app polls) become params and form fields like any other, as in Rails. Leave them out of the
  `generate` command and add them with `cargo loco generate migration` afterwards, or delete them
  from `ProjectParams`/`assign` and `form.tsx` (keep them in `ProjectProps` if a page shows them).
- Extra validations go in `ProjectParams::assign` (`errors.add("name", "is too long")`).
- `to_props()` and `ProjectProps` decide what reaches the browser. The pages import the
  TypeScript type generated from that struct: after changing it, run
  `cargo loco task types:generate`, and `npm run check` shows every page the change breaks
  ([Typed props](inertia-page.md#typed-props)).
- To regenerate pages, delete them first: `scaffold:pages` never overwrites.

## Undo

Delete the files in the table, the injected lines (`git diff` shows them), then
`cargo loco db down`, `cargo loco task routes:generate` and `cargo loco task types:generate`.

## Verify

```sh
cargo loco routes | grep projects          # 7 routes
cargo test --test mod requests::projects   # the generated request test
npm run check && npm run lint
bin/dev                                    # sign in as one@, open /acme/projects
```

Changing the templates? `cargo test --test scaffold_generator` renders them with Loco's real
generator against a temp copy and fails if an injection anchor stopped matching, and
`cargo test --test controller_generator -- --ignored` generates scaffolds and controllers in a
full copy of the app and builds and tests them (`bin/ci` runs it).
