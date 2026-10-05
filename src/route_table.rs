//! The single source of truth for every URL in the app.
//!
//! Rust handlers register these paths (`Routes::new().add(route_table::SIGN_IN, get(new))`)
//! and redirect to them (`route_table::SIGN_IN`, `route_table::session_path(id)`), and
//! `cargo loco task routes:generate` emits `frontend/routes/*.ts` from [`ROUTES`] — the
//! equivalent of the Rails kit's Typelizer route generator — so neither side can drift.
//! `tests/routes_fresh.rs` fails when the committed TS differs from [`generate_ts`].
//!
//! Paths use axum 0.8 syntax (`{id}`); the TS output uses Rails syntax (`:id`), which is what
//! `frontend/routes/runtime.ts` expects.

use md5::{Digest, Md5};

pub const ROOT: &str = "/";
pub const SIGN_IN: &str = "/sign_in";
pub const SIGN_UP: &str = "/sign_up";
pub const SESSION: &str = "/sessions/{id}";
pub const USERS: &str = "/users";
pub const IDENTITY_EMAIL_VERIFICATION: &str = "/identity/email_verification";
pub const NEW_IDENTITY_PASSWORD_RESET: &str = "/identity/password_reset/new";
pub const EDIT_IDENTITY_PASSWORD_RESET: &str = "/identity/password_reset/edit";
pub const IDENTITY_PASSWORD_RESET: &str = "/identity/password_reset";
pub const DASHBOARD: &str = "/dashboard";
pub const SETTINGS_PROFILE: &str = "/settings/profile";
pub const SETTINGS_PASSWORD: &str = "/settings/password";
pub const SETTINGS_EMAIL: &str = "/settings/email";
pub const SETTINGS_SESSIONS: &str = "/settings/sessions";
pub const SETTINGS_APPEARANCE: &str = "/settings/appearance";
pub const UP: &str = "/up";
pub const ACCOUNTS: &str = "/accounts";
pub const NEW_ACCOUNT: &str = "/accounts/new";
pub const INVITATION: &str = "/invitations/{token}";
pub const ACCEPT_INVITATION: &str = "/invitations/{token}/accept";
/// Live updates (src/live/): the tab's SSE stream, `perform`, and presence heartbeats. Like
/// Action Cable's `/cable`, not in the TS routes: `frontend/lib/live.ts` builds these URLs.
pub const LIVE: &str = "/live";
pub const LIVE_PERFORM: &str = "/live/perform";
pub const LIVE_PRESENCE: &str = "/live/presence";
// Basecamp-style account scope. axum matches static segments before `{account_slug}`, so the
// fixed paths above always win over a slug (Rails gets the same by declaring the scope last).
pub const ACCOUNT: &str = "/{account_slug}";
pub const ACCOUNT_SETTINGS: &str = "/{account_slug}/settings";
pub const ACCOUNT_MEMBERS: &str = "/{account_slug}/members";
pub const ACCOUNT_MEMBER: &str = "/{account_slug}/members/{id}";
pub const ACCOUNT_INVITATIONS: &str = "/{account_slug}/invitations";
pub const ACCOUNT_INVITATION: &str = "/{account_slug}/invitations/{id}";

/// `/{account_slug}`.
#[must_use]
pub fn account_path(slug: &str) -> String {
    ACCOUNT.replace("{account_slug}", &encode_segment(slug))
}

/// `/{account_slug}/settings`.
#[must_use]
pub fn account_settings_path(slug: &str) -> String {
    ACCOUNT_SETTINGS.replace("{account_slug}", &encode_segment(slug))
}

/// `/{account_slug}/members`.
#[must_use]
pub fn account_members_path(slug: &str) -> String {
    ACCOUNT_MEMBERS.replace("{account_slug}", &encode_segment(slug))
}

/// `/{account_slug}/members/{id}`.
#[must_use]
pub fn account_member_path(slug: &str, id: i64) -> String {
    account_members_path(slug) + "/" + &id.to_string()
}

/// `/{account_slug}/invitations`.
#[must_use]
pub fn account_invitations_path(slug: &str) -> String {
    ACCOUNT_INVITATIONS.replace("{account_slug}", &encode_segment(slug))
}

/// `/{account_slug}/invitations/{id}`.
#[must_use]
pub fn account_invitation_path(slug: &str, id: i64) -> String {
    account_invitations_path(slug) + "/" + &id.to_string()
}

/// `/invitations/{token}`.
#[must_use]
pub fn invitation_path(token: &str) -> String {
    INVITATION.replace("{token}", &encode_segment(token))
}

/// `/invitations/{token}/accept`.
#[must_use]
pub fn accept_invitation_path(token: &str) -> String {
    ACCEPT_INVITATION.replace("{token}", &encode_segment(token))
}
// scaffold:paths (`cargo loco generate scaffold` adds path constants and helpers above this line)

/// `/sessions/{id}` with the id filled in.
#[must_use]
pub fn session_path(id: &str) -> String {
    SESSION.replace("{id}", &encode_segment(id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

impl Method {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Post => "post",
            Self::Patch => "patch",
            Self::Delete => "delete",
        }
    }
}

/// Where a route appears in `frontend/routes`.
#[derive(Debug, Clone, Copy)]
pub struct Ts {
    /// Module path under `frontend/routes`, without `.ts`.
    pub module: &'static str,
    /// Name the module's default export is re-exported under in `index.ts`.
    pub namespace: &'static str,
    /// Key of the route function inside the module.
    pub action: &'static str,
    /// Top-level named export in `index.ts` (the Rails `as:` route name, camelized).
    pub alias: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
pub struct RouteDef {
    /// Lookup key for [`path`], `controller.action` in Rails terms (e.g. `sessions.destroy`).
    pub name: &'static str,
    pub method: Method,
    /// axum path (`{param}` segments).
    pub path: &'static str,
    /// `None` for routes that are not exposed to the frontend (like the Rails kit's
    /// `config.routes.exclude = [/^\/(up|rails)/]`).
    pub ts: Option<Ts>,
}

const fn ts(
    module: &'static str,
    namespace: &'static str,
    action: &'static str,
    alias: Option<&'static str>,
) -> Option<Ts> {
    Some(Ts {
        module,
        namespace,
        action,
        alias,
    })
}

const fn route(name: &'static str, method: Method, path: &'static str, ts: Option<Ts>) -> RouteDef {
    RouteDef {
        name,
        method,
        path,
        ts,
    }
}

/// Every route, in the order of the Rails kit's `config/routes.rb` (per controller, this is
/// the order the functions appear in the generated TS module).
pub const ROUTES: &[RouteDef] = {
    use Method::{Delete, Get, Patch, Post};
    const SESSIONS: &str = "SessionsController";
    const USERS_C: &str = "UsersController";
    const EMAIL_VERIFICATIONS: &str = "Identity/EmailVerificationsController";
    const PASSWORD_RESETS: &str = "Identity/PasswordResetsController";
    const PROFILES: &str = "Settings/ProfilesController";
    const PASSWORDS: &str = "Settings/PasswordsController";
    const EMAILS: &str = "Settings/EmailsController";
    const ACCOUNTS_C: &str = "AccountsController";
    const INVITATIONS_C: &str = "InvitationsController";
    const MEMBERS_C: &str = "MembersController";
    const ACCOUNT_INVITATIONS_C: &str = "Accounts/InvitationsController";
    &[
        route(
            "sessions.new",
            Get,
            SIGN_IN,
            ts(SESSIONS, "sessions", "new", Some("signIn")),
        ),
        route(
            "sessions.create",
            Post,
            SIGN_IN,
            ts(SESSIONS, "sessions", "create", None),
        ),
        route(
            "users.new",
            Get,
            SIGN_UP,
            ts(USERS_C, "users", "new", Some("signUp")),
        ),
        route(
            "users.create",
            Post,
            SIGN_UP,
            ts(USERS_C, "users", "create", None),
        ),
        route(
            "sessions.destroy",
            Delete,
            SESSION,
            ts(SESSIONS, "sessions", "destroy", Some("session")),
        ),
        route(
            "users.destroy",
            Delete,
            USERS,
            ts(USERS_C, "users", "destroy", None),
        ),
        route(
            "identity.email_verifications.show",
            Get,
            IDENTITY_EMAIL_VERIFICATION,
            ts(
                EMAIL_VERIFICATIONS,
                "identityEmailVerifications",
                "show",
                Some("identityEmailVerification"),
            ),
        ),
        route(
            "identity.email_verifications.create",
            Post,
            IDENTITY_EMAIL_VERIFICATION,
            ts(
                EMAIL_VERIFICATIONS,
                "identityEmailVerifications",
                "create",
                None,
            ),
        ),
        route(
            "identity.password_resets.new",
            Get,
            NEW_IDENTITY_PASSWORD_RESET,
            ts(
                PASSWORD_RESETS,
                "identityPasswordResets",
                "new",
                Some("newIdentityPasswordReset"),
            ),
        ),
        route(
            "identity.password_resets.edit",
            Get,
            EDIT_IDENTITY_PASSWORD_RESET,
            ts(
                PASSWORD_RESETS,
                "identityPasswordResets",
                "edit",
                Some("editIdentityPasswordReset"),
            ),
        ),
        route(
            "identity.password_resets.update",
            Patch,
            IDENTITY_PASSWORD_RESET,
            ts(
                PASSWORD_RESETS,
                "identityPasswordResets",
                "update",
                Some("identityPasswordReset"),
            ),
        ),
        route(
            "identity.password_resets.create",
            Post,
            IDENTITY_PASSWORD_RESET,
            ts(PASSWORD_RESETS, "identityPasswordResets", "create", None),
        ),
        route(
            "dashboard.index",
            Get,
            DASHBOARD,
            ts("DashboardController", "dashboard", "index", None),
        ),
        route(
            "settings.profiles.show",
            Get,
            SETTINGS_PROFILE,
            ts(
                PROFILES,
                "settingsProfiles",
                "show",
                Some("settingsProfile"),
            ),
        ),
        route(
            "settings.profiles.update",
            Patch,
            SETTINGS_PROFILE,
            ts(PROFILES, "settingsProfiles", "update", None),
        ),
        route(
            "settings.passwords.show",
            Get,
            SETTINGS_PASSWORD,
            ts(
                PASSWORDS,
                "settingsPasswords",
                "show",
                Some("settingsPassword"),
            ),
        ),
        route(
            "settings.passwords.update",
            Patch,
            SETTINGS_PASSWORD,
            ts(PASSWORDS, "settingsPasswords", "update", None),
        ),
        route(
            "settings.emails.show",
            Get,
            SETTINGS_EMAIL,
            ts(EMAILS, "settingsEmails", "show", Some("settingsEmail")),
        ),
        route(
            "settings.emails.update",
            Patch,
            SETTINGS_EMAIL,
            ts(EMAILS, "settingsEmails", "update", None),
        ),
        route(
            "settings.sessions.index",
            Get,
            SETTINGS_SESSIONS,
            ts(
                "Settings/SessionsController",
                "settingsSessions",
                "index",
                None,
            ),
        ),
        // `inertia :appearance` in Rails: a named route with no controller, which Typelizer
        // puts in `RoutesController` under the route's own name.
        route(
            "settings.appearance",
            Get,
            SETTINGS_APPEARANCE,
            ts(
                "RoutesController",
                "Routes",
                "settingsAppearance",
                Some("settingsAppearance"),
            ),
        ),
        route(
            "home.index",
            Get,
            ROOT,
            ts("HomeController", "home", "index", Some("root")),
        ),
        route("health.show", Get, UP, None),
        route("live.stream", Get, LIVE, None),
        route("live.perform", Post, LIVE_PERFORM, None),
        route("live.presence.create", Post, LIVE_PRESENCE, None),
        route("live.presence.destroy", Delete, LIVE_PRESENCE, None),
        route(
            "accounts.create",
            Post,
            ACCOUNTS,
            ts(ACCOUNTS_C, "accounts", "create", None),
        ),
        route(
            "accounts.new",
            Get,
            NEW_ACCOUNT,
            ts(ACCOUNTS_C, "accounts", "new", Some("newAccount")),
        ),
        route(
            "invitations.accept",
            Post,
            ACCEPT_INVITATION,
            ts(
                INVITATIONS_C,
                "invitations",
                "accept",
                Some("acceptInvitation"),
            ),
        ),
        route(
            "invitations.show",
            Get,
            INVITATION,
            ts(INVITATIONS_C, "invitations", "show", Some("invitation")),
        ),
        route(
            "accounts.show",
            Get,
            ACCOUNT,
            ts(ACCOUNTS_C, "accounts", "show", Some("account")),
        ),
        route(
            "accounts.edit",
            Get,
            ACCOUNT_SETTINGS,
            ts(ACCOUNTS_C, "accounts", "edit", Some("accountSettings")),
        ),
        route(
            "accounts.update",
            Patch,
            ACCOUNT_SETTINGS,
            ts(ACCOUNTS_C, "accounts", "update", None),
        ),
        route(
            "members.index",
            Get,
            ACCOUNT_MEMBERS,
            ts(MEMBERS_C, "members", "index", Some("accountMembers")),
        ),
        route(
            "members.update",
            Patch,
            ACCOUNT_MEMBER,
            ts(MEMBERS_C, "members", "update", Some("accountMember")),
        ),
        route(
            "members.destroy",
            Delete,
            ACCOUNT_MEMBER,
            ts(MEMBERS_C, "members", "destroy", None),
        ),
        route(
            "accounts.invitations.create",
            Post,
            ACCOUNT_INVITATIONS,
            ts(
                ACCOUNT_INVITATIONS_C,
                "accountsInvitations",
                "create",
                Some("accountInvitations"),
            ),
        ),
        route(
            "accounts.invitations.destroy",
            Delete,
            ACCOUNT_INVITATION,
            ts(
                ACCOUNT_INVITATIONS_C,
                "accountsInvitations",
                "destroy",
                Some("accountInvitation"),
            ),
        ),
        // scaffold:routes (`cargo loco generate scaffold` adds routes above this line)
    ]
};

/// The axum path of the route named `name` (see [`RouteDef::name`]).
///
/// # Panics
/// When no route has that name — a programming error, caught by the route table tests.
#[must_use]
pub fn path(name: &str) -> &'static str {
    find(name)
        .unwrap_or_else(|| panic!("no route named {name:?}"))
        .path
}

#[must_use]
pub fn find(name: &str) -> Option<&'static RouteDef> {
    ROUTES.iter().find(|r| r.name == name)
}

/// Directory (relative to the project root) the TS route files live in.
pub const TS_DIR: &str = "frontend/routes";

/// Hand-maintained runtime shipped alongside the generated files; never generated or deleted.
pub const TS_RUNTIME: &str = "runtime.ts";

/// Render `frontend/routes/*.ts` (except `runtime.ts`) as `(relative path, contents)` pairs,
/// in the same layout and format as Typelizer's route output in the Rails kit.
///
/// # Panics
/// When two routes of one TS module share an action key.
#[must_use]
pub fn generate_ts() -> Vec<(String, String)> {
    let mut modules: Vec<(&Ts, Vec<&RouteDef>)> = Vec::new();
    for r in ROUTES {
        let Some(t) = &r.ts else { continue };
        match modules.iter_mut().find(|(m, _)| m.module == t.module) {
            Some((m, routes)) => {
                assert_eq!(
                    m.namespace, t.namespace,
                    "{}: inconsistent namespace",
                    t.module
                );
                assert!(
                    routes
                        .iter()
                        .all(|o| o.ts.is_some_and(|ot| ot.action != t.action)),
                    "{}: duplicate action {}",
                    t.module,
                    t.action
                );
                routes.push(r);
            }
            None => modules.push((t, vec![r])),
        }
    }

    let mut files: Vec<(String, String)> = modules
        .iter()
        .map(|(t, routes)| {
            (
                format!("{}.ts", t.module),
                with_header(&render_module(t.module, routes)),
            )
        })
        .collect();
    files.push(("index.ts".to_string(), with_header(&render_index(&modules))));
    files
}

fn render_module(module: &str, routes: &[&RouteDef]) -> String {
    let depth = module.matches('/').count();
    let runtime = if depth == 0 {
        "./runtime".to_string()
    } else {
        format!("{}runtime", "../".repeat(depth))
    };
    let mut out = format!(
        "import type {{ RouteDefinition, RouteOptions }} from '{runtime}'\nimport {{ buildUrl }} from '{runtime}'\n\nexport default {{\n"
    );
    for (i, r) in routes.iter().enumerate() {
        let t = r.ts.expect("module routes have ts");
        let verb = r.method.as_str();
        let ts_path = rails_path(r.path);
        let params = path_params(r.path);
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("  /** {} {ts_path} */\n", verb.to_uppercase()));
        if params.is_empty() {
            out.push_str(&format!(
                "  {}: (options?: RouteOptions): RouteDefinition<'{verb}'> => ({{\n    url: buildUrl('{ts_path}', {{}}, options),\n",
                t.action
            ));
        } else {
            let fields = params
                .iter()
                .map(|p| format!("{}: string | number", camelize(p)))
                .collect::<Vec<_>>()
                .join("; ");
            let single = if params.len() == 1 {
                " | string | number"
            } else {
                ""
            };
            out.push_str(&format!(
                "  {}: (\n    params: {{ {fields} }}{single},\n    options?: RouteOptions,\n  ): RouteDefinition<'{verb}'> => ({{\n    url: buildUrl('{ts_path}', params, options),\n",
                t.action
            ));
        }
        out.push_str(&format!("    method: '{verb}',\n  }}),\n"));
    }
    out.push_str("}\n");
    out
}

fn render_index(modules: &[(&Ts, Vec<&RouteDef>)]) -> String {
    let mut entries: Vec<&Ts> = modules.iter().map(|(t, _)| *t).collect();
    entries.sort_by_key(|t| t.namespace);
    let mut out = String::new();
    for t in &entries {
        out.push_str(&format!(
            "export {{ default as {} }} from './{}'\n",
            t.namespace, t.module
        ));
    }

    let mut named: Vec<(&str, &Ts)> = ROUTES
        .iter()
        .filter_map(|r| r.ts.as_ref())
        .filter_map(|t| t.alias.filter(|a| *a != t.namespace).map(|a| (a, t)))
        .collect();
    named.sort_by_key(|(alias, _)| *alias);
    if named.is_empty() {
        return out;
    }

    out.push('\n');
    let mut imported: Vec<&str> = Vec::new();
    for (_, t) in &named {
        if !imported.contains(&t.module) {
            imported.push(t.module);
            out.push_str(&format!("import _{} from './{}'\n", t.namespace, t.module));
        }
    }
    out.push('\n');
    for (alias, t) in &named {
        out.push_str(&format!(
            "export const {alias} = _{}.{}\n",
            t.namespace, t.action
        ));
    }
    out
}

/// Typelizer's header: an MD5 of the body lets tools skip rewriting unchanged files.
fn with_header(body: &str) -> String {
    let digest = Md5::digest(body.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "// Typelizer digest {hex}\n//\n// DO NOT MODIFY: This file was automatically generated by `cargo loco task routes:generate` from src/route_table.rs.\n{body}"
    )
}

/// `/sessions/{id}` -> `/sessions/:id`
/// axum's `{id}` is Rails' `:id`; a glob `{*key}` is Rails' `*key` (runtime.ts keeps its
/// slashes).
fn rails_path(path: &str) -> String {
    let mut out = path.to_string();
    for p in path_params(path) {
        out = out
            .replace(&format!("{{*{p}}}"), &format!("*{p}"))
            .replace(&format!("{{{p}}}"), &format!(":{p}"));
    }
    out
}

/// The parameter names in a path, a glob's without its `*`.
fn path_params(path: &str) -> Vec<&str> {
    path.split('/')
        .filter_map(|seg| seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
        .map(|p| p.strip_prefix('*').unwrap_or(p))
        .collect()
}

fn camelize(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in snake.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn encode_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_resolvable() {
        for r in ROUTES {
            assert_eq!(
                ROUTES.iter().filter(|o| o.name == r.name).count(),
                1,
                "{}",
                r.name
            );
            assert_eq!(path(r.name), r.path);
        }
    }

    #[test]
    fn no_two_routes_share_method_and_path() {
        for r in ROUTES {
            let n = ROUTES
                .iter()
                .filter(|o| o.method == r.method && o.path == r.path)
                .count();
            assert_eq!(n, 1, "{} {}", r.method.as_str(), r.path);
        }
    }

    #[test]
    fn session_path_fills_and_encodes_id() {
        assert_eq!(session_path("abc_DEF-123"), "/sessions/abc_DEF-123");
        assert_eq!(session_path("a/b"), "/sessions/a%2Fb");
    }

    #[test]
    fn ts_uses_rails_param_syntax() {
        assert_eq!(rails_path("/sessions/{id}"), "/sessions/:id");
        let files = generate_ts();
        let (_, sessions) = files
            .iter()
            .find(|(f, _)| f == "SessionsController.ts")
            .unwrap();
        assert!(sessions.contains("buildUrl('/sessions/:id', params, options)"));
        assert!(sessions.contains("params: { id: string | number } | string | number,"));
    }

    /// A glob (`{*key}`, the rest of the path, slashes included) is Rails' `*key`, which
    /// runtime.ts fills without encoding the slashes; `:*key` would be a param named `*key`.
    #[test]
    fn a_glob_param_is_a_rails_glob_in_ts() {
        const FILE: &str = "/{account_slug}/files/{*key}";
        assert_eq!(rails_path(FILE), "/:account_slug/files/*key");
        assert_eq!(path_params(FILE), ["account_slug", "key"]);
        let file = route(
            "files.show",
            Method::Get,
            FILE,
            ts("FilesController", "files", "show", None),
        );
        let ts = render_module("FilesController", &[&file]);
        assert!(
            ts.contains("buildUrl('/:account_slug/files/*key', params, options)"),
            "{ts}"
        );
        assert!(
            ts.contains("params: { accountSlug: string | number; key: string | number },"),
            "{ts}"
        );
    }

    #[test]
    fn account_paths_fill_and_encode_the_slug() {
        assert_eq!(account_path("acme"), "/acme");
        assert_eq!(account_member_path("acme", 7), "/acme/members/7");
        assert_eq!(account_invitation_path("a/b", 1), "/a%2Fb/invitations/1");
        assert_eq!(accept_invitation_path("t0k"), "/invitations/t0k/accept");
        assert_eq!(rails_path(ACCOUNT_MEMBER), "/:account_slug/members/:id");
    }

    #[test]
    fn health_check_is_not_exposed_to_the_frontend() {
        assert!(generate_ts().iter().all(|(_, c)| !c.contains("'/up'")));
    }
}
