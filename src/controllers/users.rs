//! `UsersController`: sign up (with precognition) and delete your account.
//!
//! `settings.sign_up` decides who can sign up. `open` (the default): anyone; an invitation for
//! the same email joins its account, otherwise the user gets a personal account.
//! `invitation_only`: only through a pending invitation, with the address it was sent to;
//! without one, both the page and the POST redirect to sign in with [`invitation_only_alert`].

use axum::http::HeaderMap;
use loco_rs::{model::ModelError, prelude::*};
use serde::Deserialize;
use serde_json::json;

use crate::{
    auth::{Authenticated, Details, RequireGuest},
    controllers::{
        clock,
        members::home_path,
        precognitive,
        rate_limit::{Action, RateLimited},
        render, settings, start_session, with_invitation, NoPrecognition, Params,
    },
    inertia::{
        config::SignUp as SignUpMode, cookies, csrf::RotateCsrf, redirect::Redirect,
        render::Inertia,
    },
    mailers::user_mailer::UserMailer,
    models::{
        invitations, memberships, sessions,
        users::{self, SaveError, SignUpParams, SignedUpInto},
    },
    route_table,
};

pub struct SignUp;
impl Action for SignUp {
    const NAME: &'static str = "users.create";
    const FALLBACK: &'static str = route_table::SIGN_UP;
}

/// The sign-up form, plus an invitation token carried from `/invitations/{token}`
/// (`?invitation=`).
#[derive(Debug, Default, Deserialize)]
struct SignUpForm {
    #[serde(flatten)]
    params: SignUpParams,
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    invitation: String,
}

#[derive(Debug, Default, Deserialize)]
struct InvitationQuery {
    #[serde(default)]
    invitation: String,
}

/// The flash when sign-up is invitation only and there is no pending invitation.
#[must_use]
pub fn invitation_only_alert(app_name: &str) -> String {
    format!("{app_name} is invitation only. Ask an admin to invite you")
}

/// The invitation for `token` (none for `""` or an unknown token).
async fn find_invitation(ctx: &AppContext, token: &str) -> Result<Option<invitations::Details>> {
    if token.is_empty() {
        return Ok(None);
    }
    match invitations::Model::find_by_token(&ctx.db, token).await {
        Ok(details) => Ok(Some(details)),
        Err(ModelError::EntityNotFound) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

/// `invitation_only` without a pending invitation: back to sign in, saying why.
fn refuse_sign_up(ctx: &AppContext) -> Result<Response> {
    Ok(Redirect::to(route_table::SIGN_IN)
        .alert(invitation_only_alert(&settings(ctx)?.app_name))
        .into_response())
}

pub struct DeleteAccount;
impl Action for DeleteAccount {
    const NAME: &'static str = "users.destroy";
    const FALLBACK: &'static str = route_table::SETTINGS_PROFILE;
}

#[derive(Debug, Default, Deserialize)]
struct Challenge {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    password_challenge: String,
}

async fn new(
    _: RequireGuest,
    State(ctx): State<AppContext>,
    Query(query): Query<InvitationQuery>,
    inertia: Inertia,
) -> Result<Response> {
    if settings(&ctx)?.sign_up == SignUpMode::InvitationOnly {
        let now = clock(&ctx).now();
        let invitation = find_invitation(&ctx, &query.invitation).await?;
        if !invitation.is_some_and(|d| d.invitation.is_pending(now)) {
            return refuse_sign_up(&ctx);
        }
    }
    render(inertia, "users/new", json!({})).await
}

async fn create(
    _: RequireGuest,
    _: RateLimited<SignUp>,
    State(ctx): State<AppContext>,
    headers: HeaderMap,
    Details(details): Details,
    Params(SignUpForm { params, invitation }): Params<SignUpForm>,
) -> Result<Response> {
    let invitation_only = settings(&ctx)?.sign_up == SignUpMode::InvitationOnly;
    // One `now` for the check and the sign-up, so an invitation can't expire in between.
    let now = clock(&ctx).now();
    let mut errors = users::Model::sign_up_errors(&ctx.db, &params).await?;
    let mut invitation_details = None;
    if invitation_only {
        // Checked before precognition, so it can't probe which emails are taken either.
        let Some(pending) = find_invitation(&ctx, &invitation)
            .await?
            .filter(|d| d.invitation.is_pending(now))
        else {
            return refuse_sign_up(&ctx);
        };
        // Another email would make a personal account, so it is an error on the form instead.
        if errors.get("email").is_none()
            && users::normalize_email(&params.email) != pending.invitation.email
        {
            errors.add(
                "email",
                format!("must be {}, the address invited", pending.invitation.email),
            );
        }
        invitation_details = Some(pending);
    }
    if let Some(res) = precognitive(&headers, &errors) {
        return Ok(res);
    }
    if invitation_only && !errors.is_empty() {
        return Ok(
            Redirect::to(with_invitation(route_table::SIGN_UP, &invitation))
                .errors(errors)
                .into_response(),
        );
    }
    if !invitation_only {
        // The invitation proves the address, so it only applies when the emails match.
        invitation_details = find_invitation(&ctx, &invitation).await?;
    }
    let signed_up = users::Model::sign_up_with_account(
        &ctx.db,
        &params,
        invitation_details.as_ref().map(|d| &d.invitation),
        now,
    )
    .await;
    match signed_up {
        // Unreachable: the invitation was pending and for this email (checked above). An error
        // rather than a silent personal account (sign-up is closed).
        Ok((_, SignedUpInto::PersonalAccount(_))) if invitation_only => Err(Error::string(
            "sign-up is invitation only, but it made a personal account",
        )),
        Ok((user, into)) => {
            let session = sessions::Model::create_for_user(&ctx.db, &user, &details).await?;
            match into {
                SignedUpInto::Invitation(account_id) => {
                    memberships::members_changed(account_id);
                    let account = &invitation_details
                        .expect("joined through an invitation")
                        .account;
                    start_session(
                        &ctx,
                        &session,
                        Redirect::to(route_table::account_path(&account.slug))
                            .notice(format!("Welcome to {}", account.name)),
                    )
                }
                SignedUpInto::PersonalAccount(_) => {
                    let res = start_session(
                        &ctx,
                        &session,
                        Redirect::to(home_path(&ctx, user.id, user.last_account_id).await?)
                            .notice("Welcome! You have signed up successfully"),
                    )?;
                    UserMailer::email_verification(&ctx, &user).await?;
                    Ok(res)
                }
            }
        }
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(with_invitation(
            route_table::SIGN_UP,
            &invitation,
        ))
        .errors(errors)
        .into_response()),
        Err(err) => Err(err.into()),
    }
}

async fn destroy(
    _: NoPrecognition,
    Authenticated(current): Authenticated,
    _: RateLimited<DeleteAccount>,
    State(ctx): State<AppContext>,
    Params(params): Params<Challenge>,
) -> Result<Response> {
    match current
        .user
        .destroy_with_challenge(&ctx.db, &params.password_challenge)
        .await
    {
        Ok(()) => {
            let mut res = Redirect::to(route_table::ROOT)
                .notice("Your account has been deleted")
                .clear_history()
                .into_response();
            cookies::clear_session_token(res.headers_mut(), &*settings(&ctx)?);
            res.extensions_mut().insert(RotateCsrf);
            Ok(res)
        }
        Err(SaveError::Invalid(errors)) => Ok(Redirect::to(route_table::SETTINGS_PROFILE)
            .errors(errors)
            .into_response()),
        Err(err) => Err(err.into()),
    }
}

pub fn routes() -> Routes {
    Routes::new()
        .add(route_table::SIGN_UP, get(new).post(create))
        .add(route_table::USERS, delete(destroy))
}
