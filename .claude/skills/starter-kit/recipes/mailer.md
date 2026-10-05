# Recipe: sending email

**When:** a notification, digest, invitation. Rails: `rails g mailer` + `deliver_later`.
Generic detail: `.claude/skills/loco/recipes/mailer.md`.

## Commands

```sh
cargo loco generate mailer project_mailer
```

Writes `src/mailers/project_mailer.rs`, `src/mailers/project_mailer/welcome/{subject,html,text}.t`,
and `src/mailers/shared/` (a base layout, used only through `mail_template_with_shared`; the
kit's own mailers don't, so delete it unless you want it). Rename `welcome` to your message. The kit's
`.loco-templates/mailer/mailer.t` makes it send from `settings.mail_from`; a mailer written by
hand needs `from: Some(settings(ctx)?.mail_from.clone())` in its `mailer::Args`, or it goes out
as Loco's `System <system@example.com>`.

## This kit's pattern: deliver later

The kit never sends mail inside a request. Like Rails' `deliver_later`, the mailer method
enqueues a job and the job sends. The job is a **generated worker**, never written by hand
(`.claude/skills/loco/workflow.md`):

```sh
cargo loco generate worker project_delivery
```

1. **The worker** (`src/workers/project_delivery.rs`, registered in `App::connect_workers` and
   tested in `tests/workers/project_delivery.rs` by the generator): put **ids** in `WorkerArgs`;
   `perform` reloads the records (skip a deleted one with `Ok(())`, like Active Job discarding
   it), builds anything time-sensitive (tokens) *now*, and calls the mailer's `deliver_now`.
   Add `fn queue() -> Option<String> { Some("mailer".to_string()) }`.
2. **The mailer** (`src/mailers/project_mailer.rs`): the public method enqueues
   (`Worker::perform_later(ctx, WorkerArgs { project_id: project.id })`); `deliver_now` renders
   the Tera templates and sends with `Self::mail_template_now(ctx, &dir, mailer::Args { .. })`.

The kit's own pair is the model to copy: `src/mailers/user_mailer.rs` (`UserMailer::password_reset`
enqueues, `UserMailer::deliver_now` sends) and `src/workers/user_mailer_delivery.rs`.

Template rules (`.t` files are not autoescaped): pipe every user-controlled value through
`| escape` in `html.t`. Subject is `subject.t`, plain text `text.t`.

From address: `settings.mail_from` (`MAIL_FROM` in production). Links: build them from
`settings.app_url` plus a `route_table` path, never from the request's Host header.

## Delivery by environment

| Env | Where mail goes |
|---|---|
| development | SMTP `localhost:1025`: run Mailpit (`docker run -p 1025:1025 -p 8025:8025 axllent/mailpit`) |
| test | `mailer.stub: true`, recorded; assert with `deliveries(&ctx)` |
| Playwright | SMTP to `e2e/mail-sink.ts` (`bin/e2e-server` sets `MAILER_STUB=false`); read with `lastMailTo(page, email, subject)` from `e2e/mail.ts` (`e2e/flows.spec.ts` follows the verification and reset links) |
| production | `MAILER_HOST` / `MAILER_USER` / `MAILER_PASSWORD`; without `MAILER_HOST` the app boots, warns, and drops mail |

## Verify

```rust
let mails = deliveries(&ctx);
assert_eq!(mails.len(), 1);
assert!(mails[0].contains("To: one@example.com"));
assert!(mails[0].contains("Subject: Your project"));
```

`tests/requests/mailers.rs` has more (decoding quoted-printable bodies with `decode_qp`).
