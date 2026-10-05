//! `SessionsController`: sign in, and log out one of your own sessions.

use axum::extract::Path;
use loco_rs::{model::ModelError, prelude::*};
use serde::Deserialize;
use serde_json::json;

use crate::{
    auth::{Authenticated, Details, RequireGuest},
    controllers::{
        members::home_path,
        rate_limit::{Action, RateLimited},
        render, settings, start_session, with_invitation, NoPrecognition, Params,
    },
    inertia::{config::SignUp, cookies, csrf::RotateCsrf, redirect::Redirect, render::Inertia},
    models::{sessions, users},
    route_table,
};

pub struct SignIn;
impl Action for SignIn {
    const NAME: &'static str = "sessions.create";
    const FALLBACK: &'static str = route_table::SIGN_IN;
}

#[derive(Debug, Default, Deserialize)]
struct Credentials {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    email: String,
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    password: String,
    /// An invitation token carried from `/invitations/{token}` (`?invitation=`).
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    invitation: String,
}

/// With `settings.sign_up: invitation_only` the page gets `invitation_only: true` and offers the
/// sign-up link only to someone carrying an invitation (`?invitation=`). Open (the default): no
/// props, as in the Rails kit.
async fn new(_: RequireGuest, State(ctx): State<AppContext>, inertia: Inertia) -> Result<Response> {
    let props = if settings(&ctx)?.sign_up == SignUp::InvitationOnly {
        json!({ "invitation_only": true })
    } else {
        json!({})
    };
    render(inertia, "sessions/new", props).await
}

async fn create(
    _: NoPrecognition,
    _: RequireGuest,
    _: RateLimited<SignIn>,
    State(ctx): State<AppContext>,
    Details(details): Details,
    Params(params): Params<Credentials>,
) -> Result<Response> {
    let session =
        match users::Model::authenticate_by(&ctx.db, &params.email, &params.password).await? {
            // `None` again when the password changed between the check and the insert.
            Some(user) => {
                sessions::Model::create_for_authenticated_user(&ctx.db, &user, &details).await?
            }
            None => None,
        };
    match session {
        Some(session) => {
            // Back to the invitation (the user then presses Accept), else the signed-in home.
            let to = if params.invitation.is_empty() {
                let user = users::Model::find_by_id(&ctx.db, session.user_id).await?;
                home_path(&ctx, user.id, user.last_account_id).await?
            } else {
                route_table::invitation_path(&params.invitation)
            };
            start_session(
                &ctx,
                &session,
                Redirect::to(to).notice("Signed in successfully"),
            )
        }
        None => Ok(
            Redirect::to(with_invitation(route_table::SIGN_IN, &params.invitation))
                .alert("That email or password is incorrect")
                .into_response(),
        ),
    }
}

async fn destroy(
    _: NoPrecognition,
    Authenticated(current): Authenticated,
    State(ctx): State<AppContext>,
    Path(id): Path<String>,
) -> Result<Response> {
    let destroyed = match sessions::Model::destroy_for_user(&ctx.db, current.user.id, &id).await {
        Ok(session) => session,
        Err(ModelError::EntityNotFound) => return Err(Error::NotFound),
        Err(err) => return Err(err.into()),
    };
    let mut res = Redirect::to(route_table::SETTINGS_SESSIONS)
        .notice("That session has been logged out")
        .clear_history()
        .into_response();
    // Logging out the session this browser is using signs it out here too.
    if destroyed.id == current.session.id {
        cookies::clear_session_token(res.headers_mut(), &*settings(&ctx)?);
        res.extensions_mut().insert(RotateCsrf);
    }
    Ok(res)
}

pub fn routes() -> Routes {
    Routes::new()
        .add(route_table::SIGN_IN, get(new).post(create))
        .add(route_table::SESSION, delete(destroy))
}
