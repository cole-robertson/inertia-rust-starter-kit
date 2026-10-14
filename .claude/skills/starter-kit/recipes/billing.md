# Recipe: billing with Stripe (sketch)

> **Sketch; not implemented in this kit.** There is no Stripe dependency, table, route or page
> here. This is the shape to build, the way Rails apps using the `pay` gem or plain Stripe
> Checkout do it.

**When:** paid plans. Bill the thing that owns the data: the account (`accounts.md`), not the
user.

## Pieces

| Piece | Where it would go |
|---|---|
| `subscriptions` table: owner id, `stripe_customer_id`, `stripe_subscription_id`, `status`, `price_id`, `current_period_end` | `cargo loco generate model subscriptions ...` |
| Stripe HTTP calls | a small module over `reqwest` (already a dependency) against Stripe's REST API, or the `async-stripe` crate; keys in `settings:` via `get_env(name="STRIPE_SECRET_KEY")` |
| Checkout | `POST /billing/checkout` creates a Checkout Session, then redirect to Stripe's URL (an external redirect: extract `omega::Inertia` and return `inertia.location(url)`, which answers `409` + `X-Inertia-Location` to an Inertia visit; a plain `Redirect` to another origin gets the same 409 from the Inertia layer) |
| Customer portal | `POST /billing/portal`, same external-redirect pattern |
| Webhook | `POST /webhooks/stripe`: verify the `Stripe-Signature` HMAC over the **raw body** (`axum::body::Bytes`, not `Params`), then enqueue a worker and return 200 fast |
| Plan checks | `subscriptions::Model::active_for(owner)` used by an extractor or in handlers; share `billing.plan` as a prop |

## Kit-specific notes

- The webhook route has no browser and no CSRF token. The CSRF layer (`src/inertia/csrf.rs`)
  must skip that path: add an explicit allowlist for it, and nothing else.
- Webhooks arrive out of order and more than once: store Stripe's event id and ignore repeats;
  treat `customer.subscription.updated` as the source of truth for status.
- Trust the webhook, not the success redirect, to grant access.

## Verify (when you build it)

Stripe CLI (`stripe listen --forward-to localhost:5150/webhooks/stripe`) in development; request
tests that post signed fixture events and assert the subscription row and access checks.
