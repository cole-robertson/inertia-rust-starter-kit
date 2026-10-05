# Recipe: an admin area (sketch)

> **Sketch; not implemented in this kit.** There is no admin flag, route or page here. The
> Rails kit has none either.

**When:** staff need to see users, fix data, or impersonate for support. Rails: an `admin`
namespace with a `before_action :require_admin`, or a gem like Avo / Administrate.

## Shape

1. **Who is an admin:** `cargo loco generate migration AddAdminToUsers admin:bool!` (default
   false), then `cargo loco db migrate && cargo loco db entities`. Grant it with a task
   (`cargo loco generate task grant_admin`), never through a web form.
2. **Guard:** a `RequireAdmin` extractor in `src/auth.rs`, built like `Authenticated` (which it
   wraps), that answers **404** for non-admins so the area isn't discoverable.
3. **Routes:** `/admin/...` constants in `src/route_table.rs`, handlers in
   `src/controllers/admin/` (a module like `controllers/settings/`), pages in
   `frontend/pages/admin/`, a separate layout if it should look different.
4. **CRUD:** `cargo loco generate scaffold` then move the controller under `admin/` and swap
   `Authenticated` for `RequireAdmin`; the scaffold's pages work as they are.
5. **Audit:** log every admin write (who, what, before/after) to an `admin_events` table.

Impersonation, if needed: create a session for the target user flagged with the admin's id, and
show a banner from a shared prop; never reuse the admin's session.

## Verify (when you build it)

Request tests: a normal user gets 404 on every `/admin` route; an admin gets 200; writes create
an audit row.
