# Recipe: accounts (organizations), roles, invitations

**When:** anything that belongs to a group of people rather than one user: projects, documents,
billing. Rails: Basecamp's `Account` + `Membership`, `Current.account`, routes under
`scope ":account_slug"`.

Accounts are **core** in this kit: every signed-in page that holds data lives under
`/{account_slug}/…`. Names follow 37signals: `Account` / `Membership` / `Invitation`, roles
`owner|admin|member`. To call them "Organization", "Team" or "Workspace" instead, see
[Renaming](#renaming) below.

## What exists

| Piece | Where |
|---|---|
| Tables | `accounts (name, slug^)`, `memberships (account, user, role)` unique per pair, `invitations (account, email, role, inviter_id → users, token_digest, expires_at, accepted_at)`, `users.last_account_id` |
| Models | `src/models/accounts.rs` (slug from the name, `-2` on collision, reserved top-level paths; `create_with_owner`, `create_personal`, `list_for_user`, `default_for_user`), `memberships.rs` (`Role`, `is_manager`, last-owner rule, `members_of`), `invitations.rs` (token stored as a SHA-256 digest, 7-day expiry, `accept`) |
| Scope extractor | `auth::CurrentAccount { session, account, membership }`: signed in **and** a member of the `{account_slug}` in the URL, else **404** (never 403: the response doesn't reveal that the account exists). It records `users.last_account_id`. |
| Slug constraint | `auth::slug_constraint` (a layer in `App::after_routes`): `/robots.txt` and other non-slug first segments never reach a `/{account_slug}` handler |
| Pages | `accounts/{new,show,settings}`, `members/index` (roles, remove, leave, invite, revoke), `invitations/show` (accept, or sign up / sign in carrying `?invitation=`) |
| Switcher | `frontend/components/account-switcher.tsx` in the sidebar, fed by the shared **once** prop `accounts [{name, slug}]` (`auth::register_shared_props`); its key is a digest of the user's accounts, so it is re-sent only when they change |
| Mail | `InvitationMailer::invite` enqueues `src/workers/invitation_delivery.rs`, which mints the token and sends |
| Sign-up / sign-in | `settings.sign_up` (`SIGN_UP`): `open` (default) or `invitation_only` ([below](#invitation-only-sign-up)). Open: sign-up without an invitation creates `"<name>'s account"` with the user as owner; with a matching invitation it joins that account instead (and the email counts as verified). Sign-in and `/` go to the last-used account (`controllers::members::home_path`), `/dashboard` redirects there too, and a user with no account goes to `/accounts/new`. |
| Seeds | `src/fixtures/{accounts,memberships}.yaml`: **Acme** (one@ owner, two@ member), **Globex** (two@ owner); a pending invitation to three@ in Acme (`App::seed`) |
| Tests | `tests/models/{accounts,memberships,invitations}.rs`, `tests/requests/{accounts,members,invitations}.rs`, `tests/workers/invitation_delivery.rs`, `e2e/invitations.spec.ts` |

## Invitation-only sign-up

Internal tools and B2B apps often close public sign-up: set `SIGN_UP=invitation_only`
(`settings.sign_up` in `config/<env>.yaml`; the default is `open`). Then:

- `GET`/`POST /sign_up` work only through a pending invitation (`?invitation=<token>`), for the
  address it was sent to. Without one they redirect to sign in with "<app name> is invitation
  only. Ask an admin to invite you"; another email is an error on the form
  (`must be <invited>, the address invited`). Precognition is refused the same way, so it
  can't probe which emails exist.
- The sign-in page gets `invitation_only: true` and offers "Sign up" only to someone carrying an
  invitation (from the invitation page's link).
- Nobody gets a personal account: an invited sign-up joins the invitation's account. The
  personal-account branch in `UsersController#create` is unreachable then, and fails loudly
  rather than making one.

The kit's tests pass in both modes: request tests that depend on it pin the mode with
`with_sign_up(SignUp::Open | SignUp::InvitationOnly, ..)` (`tests/requests/mod.rs`), and the
Playwright flows sign up through an invitation from two@ when `/sign_up` is closed
(`openSignUp` in `e2e/flows.spec.ts`). Run either with `SIGN_UP=invitation_only` to check.

## Add a resource to an account

The scaffold does it by default:

```sh
cargo loco generate scaffold projects name:string!     # + account:references, added for you
cargo loco task scaffold:pages resource:projects
```

What that writes (all of it is the pattern to follow by hand, too):

1. **Paths** under `/{account_slug}` in `src/route_table.rs`, with helpers that take the slug:
   `projects_path(slug)`, `project_path(slug, id)`. The TS helpers take
   `{ accountSlug, id }`: `projects.show({ accountSlug: account.slug, id: project.id })`.
2. **Controller:** every handler takes `current: CurrentAccount` (a non-member gets 404) and
   passes `current.account.id` to the model.
3. **Model finders** take the account, so a cross-account id is *not found*, a 404:

   ```rust
   pub async fn find_in_account(db: &DatabaseConnection, account_id: i64, id: i64) -> ModelResult<Self> {
       Entity::find_by_id(id)
           .filter(Column::AccountId.eq(account_id))
           .first(db)
           .await?
           .ok_or(ModelError::EntityNotFound)
   }
   ```

   `list(db, account_id)`, `create(db, account_id, params)` (the account is never a param) and
   the `*_options` lists for a `references` select whose parent belongs to accounts are scoped
   the same way.
4. **Sidebar:** the link goes in the account's nav (`projects.index(account.slug)`).
5. **Test:** the generated request test makes a project in Globex as two@ and proves one@ (Acme
   only) gets 404 on Globex's URLs and on that project's id through `/acme/...`.

Rails: `Current.account.projects.find(params[:id])`.

A resource outside accounts: `--global`. A controller under a resource
(`cargo loco generate controller projects/archives create destroy`) looks the project up with
`find_in_account` too.

## Check a role

```rust
async fn destroy(current: CurrentAccount, headers: HeaderMap, /* … */) -> Result<Response> {
    if !current.is_manager() {
        // redirect back with alert "You don't have permission to do that"
        return Ok(current.forbidden(&headers));
    }
    // …
}
```

- `current.membership.role()` is a `memberships::Role` (`Owner | Admin | Member`);
  `is_manager()` is owner-or-admin.
- Read-only pages render for everyone and send a `can_manage` prop the page uses to hide
  buttons (`members/index`). The server check is the one that counts.
- The last owner can't be demoted or removed: `Membership::change_role` / `remove` refuse with
  "An account needs at least one owner" (checked inside the transaction).

Rails: `before_action :require_admin` with `Current.membership.admin?`.

## Account-wide shared props

`auth::register_shared_props` adds `accounts` (the switcher list) for signed-in users. To share
something about the current account on every page (a plan, a feature flag), add it there and
read the slug from the path (`account_slug_of`), or render it from the account's own pages.

## Renaming

The names appear in tables, models, routes, pages and copy. Two ways:

- **Keep the code names, change the copy.** Users see "account" only in page text, flash
  messages and mail (`frontend/pages/accounts/*`, `members/index.tsx`, the account switcher,
  `src/mailers/invitation_mailer/invite/*`, the flash strings in `src/controllers/accounts.rs`).
  Changing those words to "workspace" is an afternoon and keeps every test name meaningful.
  This is what Basecamp does: the code says `Account`, the UI says whatever marketing likes.
- **Rename everything** (`Organization`, `organization_id`, `/{organization_slug}`): a new
  migration renaming the tables and columns, then `cargo loco db entities`, then a
  search-and-replace over `src/`, `tests/`, `frontend/` and `e2e/` for `account`/`Account`/
  `accounts` (check each hit: `users.last_account_id`, `account_path`, `useCurrentAccount`,
  `AccountSwitcher`, and the reserved-slug list in `models/accounts.rs`), then
  `cargo loco task routes:generate` and `bin/ci`. There is no script for it; most apps are
  better served by the first option.

## Single-user apps

Accounts stay in the kit; most apps keep them (they are cheap to keep and expensive to add to
an app that already has data). If an app really has no teams, flatten them in this order, and
keep the tables (every user still has exactly one personal account behind the scenes):

1. **Generate new resources with `--global`** (`cargo loco generate scaffold notes body:text
   --global`): plain signed-in routes, no `account_id`. Existing scoped resources can stay.
2. **Hide the team UI** in `frontend/components/app-sidebar.tsx`: the `<AccountSwitcher />` in the
   sidebar header and the "Members" entry in `mainNavItems`. Show them only when
   `usePage().props.accounts` has more than one entry, or remove them. Their routes can stay.
3. **Close the team entry points:** don't link `/accounts/new`, and leave invitations unused
   (`SIGN_UP` stays `open`).
4. **Optional:** send `/` and sign-in to a global page instead of `/{account_slug}`
   (`controllers::members::home_path`).

Run `bin/ci` after each step; the account tests keep passing because the model and routes are
unchanged. Don't delete the account tables, `CurrentAccount` or the migrations: the scaffolds,
`generate channel` and sign-up depend on them.

## Verify

```sh
cargo test --test mod requests::accounts requests::members requests::invitations
npx playwright test e2e/invitations.spec.ts
```
