//! `UserMailer` from the Rails kit: password reset and email verification. The public methods
//! enqueue a [`crate::workers::user_mailer_delivery`] job (`deliver_later`); the job calls
//! [`UserMailer::deliver_now`].
//!
//! Templates live in `src/mailers/user_mailer/<message>/{subject,html,text}.t`. Their `.t`
//! suffix means Tera does NOT autoescape them, so the HTML templates pipe every value
//! through `| escape` explicitly (the user controls `email`; the URL carries a token).
#![allow(non_upper_case_globals)]

use loco_rs::prelude::*;
use serde_json::json;

use crate::{
    controllers::{clock, settings},
    models::{tokens::Purpose, users},
    route_table,
    workers::user_mailer_delivery::{Worker, WorkerArgs},
};

static password_reset: Dir<'_> = include_dir!("src/mailers/user_mailer/password_reset");
static email_verification: Dir<'_> = include_dir!("src/mailers/user_mailer/email_verification");

pub struct UserMailer;
impl Mailer for UserMailer {}

impl UserMailer {
    /// `UserMailer.with(user:).password_reset.deliver_later`: enqueues a
    /// [`crate::workers::user_mailer_delivery`] job; the token is generated when the job runs.
    ///
    /// # Errors
    /// Enqueueing failures (or, with `ForegroundBlocking` workers, delivery failures).
    pub async fn password_reset(ctx: &AppContext, user: &users::Model) -> Result<()> {
        Self::deliver_later(ctx, user, Purpose::PasswordReset).await
    }

    /// `UserMailer.with(user:).email_verification.deliver_later`: enqueues a
    /// [`crate::workers::user_mailer_delivery`] job; the token is generated when the job runs.
    ///
    /// # Errors
    /// Enqueueing failures (or, with `ForegroundBlocking` workers, delivery failures).
    pub async fn email_verification(ctx: &AppContext, user: &users::Model) -> Result<()> {
        Self::deliver_later(ctx, user, Purpose::EmailVerification).await
    }

    async fn deliver_later(ctx: &AppContext, user: &users::Model, purpose: Purpose) -> Result<()> {
        Worker::perform_later(
            ctx,
            WorkerArgs {
                user_id: user.id,
                purpose,
            },
        )
        .await?;
        Ok(())
    }

    /// Render and send `purpose`'s message to `user` now, with a token minted now
    /// (the body of Rails' mailer action, run by the job).
    ///
    /// # Errors
    /// Template rendering or delivery failures.
    pub async fn deliver_now(
        ctx: &AppContext,
        user: &users::Model,
        purpose: Purpose,
    ) -> Result<()> {
        let (dir, path) = match purpose {
            Purpose::PasswordReset => (&password_reset, route_table::EDIT_IDENTITY_PASSWORD_RESET),
            Purpose::EmailVerification => (
                &email_verification,
                route_table::IDENTITY_EMAIL_VERIFICATION,
            ),
        };
        let settings = settings(ctx)?;
        if settings.production && !smtp_configured(ctx) {
            tracing::warn!(
                purpose = purpose.as_str(),
                user_id = user.id,
                "mail not sent: no SMTP configured (set MAILER_HOST)"
            );
        }
        let clock = clock(ctx);
        let sid = user.generate_token_for(purpose, settings.secret_key_base.as_bytes(), &*clock);
        let url = format!(
            "{}{path}?{}",
            settings.app_url.trim_end_matches('/'),
            serde_urlencoded::to_string([("sid", sid.as_str())])
                .map_err(|e| Error::Message(e.to_string()))?
        );
        Self::mail_template_now(
            ctx,
            dir,
            mailer::Args {
                from: Some(settings.mail_from.clone()),
                to: user.email.clone(),
                locals: json!({ "email": user.email, "url": url }),
                ..Default::default()
            },
        )
        .await
    }
}

/// Whether mail really leaves the process. Production without `MAILER_HOST` runs on Loco's
/// stub transport (config/production.yaml), which accepts every message and sends nothing.
#[must_use]
pub fn smtp_configured(ctx: &AppContext) -> bool {
    ctx.config
        .mailer
        .as_ref()
        .is_some_and(|m| !m.stub && m.smtp.as_ref().is_some_and(|s| s.enable))
}
