//! `cargo loco task scaffold:pages resource:<plural>`: the React half of the kit's scaffold.
//!
//! `cargo loco generate scaffold posts title:string! body:text` writes the Rust half (migration,
//! entity, model, Inertia controller, route-table entries, request test; see
//! `.loco-templates/`). Loco's generator cannot write this kit's React pages, so this task does,
//! from the entity it just generated: `frontend/pages/<plural>/{index,show,new,edit,form}.tsx`
//! rendered from `.loco-templates/scaffold/pages/*.t`, a sidebar link, and
//! `frontend/routes/*.ts` regenerated. Existing pages are never overwritten.
//!
//! `cargo loco task scaffold:pages controller:<name>` does the same for a controller made by
//! `cargo loco generate controller <name> [actions]...`: one placeholder page per component the
//! controller renders, from `.loco-templates/controller/pages/page.t`.

use std::{path::Path, process::Command};

use loco_rs::prelude::*;
use serde_json::{json, Value};

use crate::tasks::routes_generate;

/// Where the page templates live, relative to the app root.
pub const TEMPLATE_DIR: &str = ".loco-templates/scaffold/pages";
/// The sidebar the resource's nav link is added to.
pub const SIDEBAR: &str = "frontend/components/app-sidebar.tsx";
/// The line in [`SIDEBAR`] an account-scoped resource's nav link goes above (in the account's
/// nav, with the slug in the URL).
pub const NAV_ANCHOR: &str = "// scaffold:nav";
/// The line in [`SIDEBAR`] a global resource's nav link goes above.
pub const GLOBAL_NAV_ANCHOR: &str = "// scaffold:nav-global";
/// The page template for `controller:<name>`, relative to the app root.
pub const CONTROLLER_PAGE_TEMPLATE: &str = ".loco-templates/controller/pages/page.t";

pub struct ScaffoldPages;

#[async_trait]
impl Task for ScaffoldPages {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "scaffold:pages".to_string(),
            detail: "Generate the React pages for a resource made by `cargo loco generate \
                     scaffold` (resource:<plural, e.g. posts>) or a controller made by `cargo \
                     loco generate controller` (controller:<name, e.g. reports>)"
                .to_string(),
        }
    }

    async fn run(&self, _ctx: &AppContext, vars: &task::Vars) -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let (resource, written) = if let Ok(controller) = vars.cli_arg("controller") {
            let written = generate_controller(root, controller).map(|written| Generated {
                written,
                skipped: Vec::new(),
            });
            (controller, written)
        } else {
            let resource = vars.cli_arg("resource")?;
            (resource, generate(root, resource))
        };
        let Generated { written, skipped } = written.map_err(|e| Error::string(&e))?;
        println!("scaffold:pages: wrote {}", written.join(", "));
        if !skipped.is_empty() {
            println!(
                "scaffold:pages: not in the form or the pages (no input for them): {}",
                skipped.join(", ")
            );
        }
        println!("{}", routes_generate::write(root).map_err(Error::wrap)?);
        if !written.is_empty() {
            let prettier = Command::new("npx")
                .args(["prettier", "--write", "--log-level", "warn"])
                .args(&written)
                .current_dir(root)
                .status();
            if !prettier.is_ok_and(|s| s.success()) {
                println!("warning: `npx prettier --write` failed; run `npm run format:fix`");
            }
        }
        println!("Next: `cargo test`, then open the {resource} pages in `bin/dev`.");
        Ok(())
    }
}

/// One column of the resource, as the page templates see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    /// The Rust type without `Option<…>`.
    pub rust_type: String,
    pub nullable: bool,
    /// `#[sea_orm(column_type = "Text")]`: edited in a textarea.
    pub text: bool,
}

impl Field {
    /// A `references` column: an `i64` named `<association>_id`, the same rule as
    /// `.loco-templates/scaffold/api/dto.t`.
    fn association(&self) -> Option<&str> {
        (self.rust_type == "i64")
            .then(|| self.name.strip_suffix("_id"))
            .flatten()
    }

    /// Edited with a select of the parent rows. `user_id` is not: the owner is set by the
    /// controller, not picked from every user.
    fn is_select(&self) -> bool {
        self.association().is_some_and(|a| a != "user")
    }

    /// The form input, or why the column has none (it is then left out of the pages, as the
    /// scaffold's controller and model leave it out of params and props).
    fn input(&self, tables: &[String]) -> std::result::Result<&'static str, String> {
        if self.is_select() {
            let parent = cruet::to_plural(self.association().unwrap_or_default());
            if !tables.is_empty() && !tables.contains(&parent) {
                return Err(format!("{} (no `{parent}` table to pick from)", self.name));
            }
            return Ok("select");
        }
        Ok(match self.rust_type.as_str() {
            "String" if self.text => "textarea",
            "String" => "text",
            "bool" => "checkbox",
            "Date" => "date",
            "i16" | "i32" | "i64" | "f32" | "f64" => "number",
            other => return Err(format!("{} ({other}: no form input)", self.name)),
        })
    }

    fn ts_type(&self) -> &'static str {
        match self.rust_type.as_str() {
            "bool" => "boolean",
            "String" | "Date" => "string",
            _ => "number",
        }
    }

    fn to_json(&self, tables: &[String]) -> std::result::Result<Value, String> {
        let input = self.input(tables)?;
        let ts_type = self.ts_type();
        let association = self.association();
        Ok(json!({
            "name": self.name,
            // Where the server puts this field's errors: `belongs_to`'s "must exist" is on the
            // association (`project`), not the column.
            "error_key": association.unwrap_or(&self.name),
            // The `<association>_options` page prop and its camelCase form prop.
            "options_prop": association.map(|a| format!("{a}_options")),
            "options_camel": association.map(|a| camel_case(&format!("{a}_options"))),
            "label": humanize(&self.name),
            "input": input,
            "step": if self.rust_type.starts_with('f') { "any" } else { "1" },
            "nullable": self.nullable,
            "ts_type": if self.nullable { format!("{ts_type} | null") } else { ts_type.to_string() },
        }))
    }
}

/// The user-editable columns of `src/models/_entities/<plural>.rs` (everything except `id`,
/// `created_at` and `updated_at`), in declaration order.
///
/// # Errors
/// When the file has no `pub struct Model`.
pub fn parse_entity(source: &str) -> std::result::Result<Vec<Field>, String> {
    let body = source
        .split_once("pub struct Model {")
        .and_then(|(_, rest)| rest.split_once("\n}"))
        .map(|(body, _)| body)
        .ok_or("no `pub struct Model { … }` in the entity")?;
    let mut fields = Vec::new();
    let mut text = false;
    for line in body.lines().map(str::trim) {
        if line.starts_with("#[sea_orm(") {
            text |= line.contains("column_type = \"Text\"");
            continue;
        }
        let Some((name, ty)) = line
            .strip_prefix("pub ")
            .and_then(|l| l.strip_suffix(','))
            .and_then(|l| l.split_once(": "))
        else {
            continue;
        };
        let (rust_type, nullable) = ty
            .strip_prefix("Option<")
            .and_then(|t| t.strip_suffix('>'))
            .map_or((ty, false), |t| (t, true));
        if !matches!(name, "id" | "created_at" | "updated_at") {
            fields.push(Field {
                name: name.to_string(),
                rust_type: rust_type.to_string(),
                nullable,
                text,
            });
        }
        text = false;
    }
    Ok(fields)
}

/// The `Foo` of `pub struct FooParams` in the scaffolded model, i.e. the resource's singular
/// name as the generator inflected it.
fn singular_from_model(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        line.trim()
            .strip_prefix("pub struct ")?
            .strip_suffix("Params {")
            .map(str::to_string)
    })
}

/// What [`generate`] did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Generated {
    /// The files written (relative to the app root).
    pub written: Vec<String>,
    /// The columns left out of the pages, each with the reason.
    pub skipped: Vec<String>,
}

/// Render the pages and link the sidebar under `root`. Existing pages are skipped, so
/// re-running it is safe.
///
/// # Errors
/// When the resource was not scaffolded with the kit's templates, or on any filesystem or
/// template error.
pub fn generate(root: &Path, resource: &str) -> std::result::Result<Generated, String> {
    let read = |rel: &str| {
        std::fs::read_to_string(root.join(rel)).map_err(|e| {
            format!(
                "{rel}: {e}. Run `cargo loco generate scaffold {resource} <field:type>...` first."
            )
        })
    };
    let entity = read(&format!("src/models/_entities/{resource}.rs"))?;
    let model = read(&format!("src/models/{resource}.rs"))?;
    let pascal_singular = singular_from_model(&model).ok_or_else(|| {
        format!(
            "src/models/{resource}.rs has no `pub struct <Name>Params`: it was not generated \
             with the kit's scaffold templates (.loco-templates/scaffold/api/)"
        )
    })?;
    let fields = parse_entity(&entity)?;
    let tables: Vec<String> = crate::generate::entity_tables(root)
        .into_iter()
        .map(|(table, _)| table)
        .collect();
    let (vars, skipped) = vars(resource, &pascal_singular, &fields, &tables);

    let rrgen = loco_gen::RRgen::with_working_dir(root);
    let mut templates: Vec<_> = std::fs::read_dir(root.join(TEMPLATE_DIR))
        .map_err(|e| format!("{TEMPLATE_DIR}: {e}"))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "t"))
        .collect();
    templates.sort();
    let mut written = Vec::new();
    for template in &templates {
        let source = std::fs::read_to_string(template).map_err(|e| e.to_string())?;
        match rrgen.generate(&source, &vars).map_err(|e| e.to_string())? {
            loco_gen::GenResult::Generated { .. } => {
                let page = template.file_stem().unwrap_or_default().to_string_lossy();
                written.push(format!("frontend/pages/{resource}/{page}.tsx"));
            }
            loco_gen::GenResult::Skipped => {}
        }
    }

    let sidebar_path = root.join(SIDEBAR);
    if let Ok(sidebar) = std::fs::read_to_string(&sidebar_path) {
        if let Some(linked) = link_sidebar(&sidebar, &vars) {
            std::fs::write(&sidebar_path, linked).map_err(|e| e.to_string())?;
            written.push(SIDEBAR.to_string());
        }
    }

    Ok(Generated { written, skipped })
}

/// Render a placeholder page for every component `src/controllers/<name>.rs` renders
/// (`render(inertia, "<name>/<action>", ..)`) under `root`; returns the files written. Existing
/// pages are skipped.
///
/// # Errors
/// When the controller doesn't exist or renders no `<name>/…` page, or on any filesystem or
/// template error.
pub fn generate_controller(root: &Path, name: &str) -> std::result::Result<Vec<String>, String> {
    let rel = format!("src/controllers/{name}.rs");
    let source = std::fs::read_to_string(root.join(&rel)).map_err(|e| {
        format!("{rel}: {e}. Run `cargo loco generate controller {name} [actions]...` first.")
    })?;
    let actions = rendered_actions(&source, name);
    if actions.is_empty() {
        return Err(format!("{rel} renders no \"{name}/<action>\" page"));
    }
    let template = std::fs::read_to_string(root.join(CONTROLLER_PAGE_TEMPLATE))
        .map_err(|e| format!("{CONTROLLER_PAGE_TEMPLATE}: {e}"))?;
    // A controller under `/{account_slug}` takes `CurrentAccount`; its pages need the slug.
    let scoped = source.contains("CurrentAccount");
    // Breadcrumbs link to the index page when there is one. A `show` that takes a path param
    // (`Path(...)` in its handler) is a member page, which can't link to itself without it.
    let has_index = actions.iter().any(|a| a == "index");
    let member_show = source
        .split("async fn show(")
        .nth(1)
        .and_then(|rest| rest.split(") -> ").next())
        .is_some_and(|signature| signature.contains("Path("));
    let rrgen = loco_gen::RRgen::with_working_dir(root);
    let mut written = Vec::new();
    for action in &actions {
        let mut vars = controller_vars(name, action);
        vars["scoped"] = Value::Bool(scoped);
        vars["has_index"] = Value::Bool(has_index);
        vars["member"] = Value::Bool(member_show && action == "show");
        match rrgen
            .generate(&template, &vars)
            .map_err(|e| e.to_string())?
        {
            loco_gen::GenResult::Generated { .. } => {
                written.push(format!("frontend/pages/{name}/{action}.tsx"));
            }
            loco_gen::GenResult::Skipped => {}
        }
    }
    Ok(written)
}

/// The `<action>` of every `"<name>/<action>"` string in a controller, in order, once each.
fn rendered_actions(source: &str, name: &str) -> Vec<String> {
    let prefix = format!("\"{name}/");
    let mut actions: Vec<String> = Vec::new();
    for (at, _) in source.match_indices(&prefix) {
        let rest = &source[at + prefix.len()..];
        let Some(end) = rest.find('"') else { continue };
        let action = &rest[..end];
        let valid = !action.is_empty()
            && action
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if valid && !actions.iter().any(|a| a == action) {
            actions.push(action.to_string());
        }
    }
    actions
}

fn controller_vars(name: &str, action: &str) -> Value {
    let label = humanize(name);
    let action_label = humanize(action);
    let pascal = |s: &str| {
        let camel = camel_case(s);
        let mut chars = camel.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect()
        })
    };
    json!({
        "file_name": name,
        "pascal": pascal(name),
        "camel": camel_case(name),
        "label": label,
        "action": action,
        "action_camel": camel_case(action),
        "action_pascal": pascal(action),
        "action_label": action_label,
        "title": if action == "index" { label.clone() } else { format!("{label}: {}", action_label.to_lowercase()) },
    })
}

/// The template variables, and the columns left out (with the reason). A required
/// `account_id` makes the resource account-scoped (its pages take the slug) and is never a
/// field.
fn vars(
    resource: &str,
    pascal_singular: &str,
    fields: &[Field],
    tables: &[String],
) -> (Value, Vec<String>) {
    let snake_singular = snake_case(pascal_singular);
    let scoped = fields
        .iter()
        .any(|f| f.name == "account_id" && f.rust_type == "i64" && !f.nullable);
    let mut skipped = Vec::new();
    let mut editable = Vec::new();
    let mut shown: Vec<&Field> = Vec::new();
    for f in fields {
        if scoped && f.name == "account_id" {
            continue;
        }
        match f.to_json(tables) {
            Ok(json) => {
                editable.push(json);
                shown.push(f);
            }
            Err(reason) => skipped.push(reason),
        }
    }
    let title_field = shown
        .iter()
        .find(|f| f.rust_type == "String")
        .or_else(|| shown.first())
        .map_or("id", |f| f.name.as_str());
    let selects: Vec<Value> = editable
        .iter()
        .filter(|f| f["input"] == "select")
        .cloned()
        .collect();
    let vars = json!({
        "snake_plural": resource,
        "snake_singular": snake_singular,
        "pascal_singular": pascal_singular,
        "camel_plural": camel_case(resource),
        "camel_singular": camel_case(&snake_singular),
        "label_plural": humanize(resource),
        "label_singular": humanize(&snake_singular),
        "title_field": title_field,
        "scoped": scoped,
        "fields": editable,
        "selects": selects,
    });
    (vars, skipped)
}

/// `sidebar` with a nav link to the resource's index added above [`NAV_ANCHOR`] and its route
/// namespace added to the `@/routes` import; `None` when there is no anchor or it is linked.
#[must_use]
pub fn link_sidebar(sidebar: &str, vars: &Value) -> Option<String> {
    let namespace = vars["camel_plural"].as_str()?;
    let scoped = vars["scoped"].as_bool().unwrap_or(false);
    let (anchor, args) = if scoped {
        (NAV_ANCHOR, "account.slug")
    } else {
        (GLOBAL_NAV_ANCHOR, "")
    };
    let item = format!(
        "  {{ title: \"{}\", href: {namespace}.index({args}).url, icon: NotebookText }},",
        vars["label_plural"].as_str()?
    );
    if sidebar.contains(&format!("href: {namespace}.index(")) {
        return None;
    }
    if !sidebar.lines().any(|line| line.trim() == anchor) {
        return None;
    }
    let mut out = Vec::new();
    for line in sidebar.lines() {
        if line.trim() == anchor {
            out.push(item.clone());
        }
        out.push(line.to_string());
    }
    let linked = out.join("\n") + "\n";
    let linked = add_named_import(&linked, "@/routes", namespace)?;
    add_named_import(&linked, "lucide-react", "NotebookText")
}

/// `source` with `name` added, in sorted position, to its named import from `module`, whether
/// that import is on one line or wrapped by Prettier over several; `None` when there is no such
/// import. The import is written back on one line (Prettier rewraps it).
fn add_named_import(source: &str, module: &str, name: &str) -> Option<String> {
    let from = format!("}} from \"{module}\"");
    let end = source.find(&from)?;
    let start = source[..end].rfind("import {")?;
    let names: Vec<&str> = source[start + "import {".len()..end]
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    let mut names = names;
    if !names.contains(&name) {
        names.push(name);
        names.sort_unstable();
    }
    Some(format!(
        "{}import {{ {} }} from \"{module}\"{}",
        &source[..start],
        names.join(", "),
        &source[end + from.len()..]
    ))
}

fn snake_case(pascal: &str) -> String {
    let mut out = String::new();
    for (i, c) in pascal.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
    }
    out
}

fn camel_case(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// `blog_posts` -> `Blog posts` (Rails' `humanize`).
fn humanize(snake: &str) -> String {
    let spaced = snake.trim_end_matches("_id").replace('_', " ");
    let mut chars = spaced.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTITY: &str = r#"use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "blog_posts")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub body: Option<String>,
    pub published: Option<bool>,
    #[sea_orm(column_type = "Double")]
    pub rating: f64,
    pub user_id: i64,
}
"#;

    #[test]
    fn entity_columns_become_fields_without_id_and_timestamps() {
        let fields = parse_entity(ENTITY).unwrap();
        let names: Vec<_> = fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["title", "body", "published", "rating", "user_id"]);
        assert_eq!(
            fields[1],
            Field {
                name: "body".into(),
                rust_type: "String".into(),
                nullable: true,
                text: true,
            }
        );
        let inputs: Vec<_> = fields.iter().map(|f| f.input(&[]).unwrap()).collect();
        assert_eq!(inputs, ["text", "textarea", "checkbox", "number", "number"]);
    }

    #[test]
    fn a_reference_is_a_select_keyed_by_its_association_except_the_owner() {
        let entity = ENTITY.replace(
            "    pub user_id: i64,",
            "    pub user_id: i64,\n    pub project_id: i64,\n    pub parent_id: Option<i64>,",
        );
        let (v, skipped) = vars(
            "blog_posts",
            "BlogPost",
            &parse_entity(&entity).unwrap(),
            &[],
        );
        assert!(skipped.is_empty());
        let project = &v["fields"][5];
        assert_eq!(project["input"], "select");
        assert_eq!(project["error_key"], "project");
        assert_eq!(project["options_prop"], "project_options");
        assert_eq!(project["options_camel"], "projectOptions");
        assert_eq!(v["fields"][6]["input"], "select");
        assert_eq!(v["fields"][6]["nullable"], true);
        let user = &v["fields"][4];
        assert_eq!(
            user["input"], "number",
            "the owner is not picked from every user"
        );
        assert_eq!(user["error_key"], "user");
        let selects: Vec<_> = v["selects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].clone())
            .collect();
        assert_eq!(selects, ["project_id", "parent_id"]);
    }

    #[test]
    fn a_column_without_a_form_input_is_left_out_with_a_note() {
        let entity = ENTITY.replace(
            "pub rating: f64",
            "pub key: Uuid,\n    pub archived_at: Option<DateTimeWithTimeZone>,\n    pub creator_id: i64",
        );
        let tables = ["blog_posts".to_owned(), "users".to_owned()];
        let (v, skipped) = vars(
            "blog_posts",
            "BlogPost",
            &parse_entity(&entity).unwrap(),
            &tables,
        );
        assert_eq!(
            skipped,
            [
                "key (Uuid: no form input)",
                "archived_at (DateTimeWithTimeZone: no form input)",
                "creator_id (no `creators` table to pick from)",
            ]
        );
        let names: Vec<_> = v["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["name"].clone())
            .collect();
        assert_eq!(names, ["title", "body", "published", "user_id"]);
    }

    #[test]
    fn a_required_account_id_scopes_the_pages_and_is_never_a_field() {
        let entity = ENTITY.replace(
            "pub user_id: i64,",
            "pub user_id: i64,\n    pub account_id: i64,",
        );
        let (v, skipped) = vars(
            "blog_posts",
            "BlogPost",
            &parse_entity(&entity).unwrap(),
            &[],
        );
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(v["scoped"], true);
        assert!(v["fields"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["name"] != "account_id"));
        assert!(v["selects"].as_array().unwrap().is_empty());
        let (v, _) = vars(
            "blog_posts",
            "BlogPost",
            &parse_entity(ENTITY).unwrap(),
            &[],
        );
        assert_eq!(v["scoped"], false);
    }

    #[test]
    fn names_are_inflected_from_the_scaffolded_model() {
        let model = "impl X {}\npub struct BlogPostParams {\n}";
        let singular = singular_from_model(model).unwrap();
        let (v, _) = vars("blog_posts", &singular, &parse_entity(ENTITY).unwrap(), &[]);
        assert_eq!(v["snake_singular"], "blog_post");
        assert_eq!(v["camel_plural"], "blogPosts");
        assert_eq!(v["camel_singular"], "blogPost");
        assert_eq!(v["label_plural"], "Blog posts");
        assert_eq!(v["title_field"], "title");
        assert_eq!(v["fields"][4]["label"], "User");
        assert_eq!(v["fields"][3]["step"], "any");
        assert_eq!(v["fields"][2]["ts_type"], "boolean | null");
    }

    #[test]
    fn a_controllers_pages_are_the_components_it_renders() {
        let source = r#"
async fn index(..) { render(inertia, "monthly_reports/index", json!({})).await }
async fn summary(..) { render(inertia, "monthly_reports/summary", json!({})).await }
async fn again(..) { render(inertia, "monthly_reports/index", json!({})).await }
async fn other(..) { render(inertia, "dashboard/index", json!({})).await }
"#;
        assert_eq!(
            rendered_actions(source, "monthly_reports"),
            ["index", "summary"]
        );
        let v = controller_vars("monthly_reports", "year_end");
        assert_eq!(v["pascal"], "MonthlyReports");
        assert_eq!(v["camel"], "monthlyReports");
        assert_eq!(v["action_camel"], "yearEnd");
        assert_eq!(v["action_pascal"], "YearEnd");
        assert_eq!(v["title"], "Monthly reports: year end");
        assert_eq!(
            controller_vars("monthly_reports", "index")["title"],
            "Monthly reports"
        );
    }

    #[test]
    fn the_sidebar_gets_one_sorted_link_per_resource_in_the_right_list() {
        let sidebar = "import { BookOpen, LayoutGrid } from \"lucide-react\"\n\
                       import { accounts } from \"@/routes\"\n\
                       const globalNavItems: NavItem[] = [\n  // scaffold:nav-global\n]\n\
                       const mainNavItems: NavItem[] = [\n  { title: \"Overview\" },\n  // scaffold:nav\n]\n";
        let (v, _) = vars(
            "blog_posts",
            "BlogPost",
            &parse_entity(ENTITY).unwrap(),
            &[],
        );
        let linked = link_sidebar(sidebar, &v).unwrap();
        assert!(
            linked.contains("import { BookOpen, LayoutGrid, NotebookText } from \"lucide-react\"")
        );
        assert!(linked.contains("import { accounts, blogPosts } from \"@/routes\""));
        assert!(linked.contains(
            "  { title: \"Blog posts\", href: blogPosts.index().url, icon: NotebookText },\n  // scaffold:nav-global"
        ));
        assert_eq!(link_sidebar(&linked, &v), None, "linking twice is a no-op");

        // Imports Prettier wrapped over several lines get the names too.
        let wrapped = sidebar.replace(
            "import { BookOpen, LayoutGrid } from \"lucide-react\"",
            "import {\n  BookOpen,\n  LayoutGrid,\n} from \"lucide-react\"",
        );
        let linked = link_sidebar(&wrapped, &v).unwrap();
        assert!(
            linked.contains("import { BookOpen, LayoutGrid, NotebookText } from \"lucide-react\""),
            "{linked}"
        );

        // An account-scoped resource goes in the account's nav, with the slug.
        let entity = ENTITY.replace(
            "pub user_id: i64,",
            "pub user_id: i64,\n    pub account_id: i64,",
        );
        let (v, _) = vars("widgets", "Widget", &parse_entity(&entity).unwrap(), &[]);
        let linked = link_sidebar(sidebar, &v).unwrap();
        assert!(linked.contains(
            "  { title: \"Widgets\", href: widgets.index(account.slug).url, icon: NotebookText },\n  // scaffold:nav\n"
        ), "{linked}");
    }
}
