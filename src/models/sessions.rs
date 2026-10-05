//! A signed-in browser. The Rails kit's `Session` model.
//!
//! The browser holds `token` (a random UUIDv4), never the integer `id`. The
//! token is what goes into the signed `session_token` cookie and into URLs such
//! as `DELETE /sessions/:id`, so session ids cannot be enumerated even though
//! the cookie is also HMAC-signed.

use loco_rs::prelude::*;
use sea_orm::{ActiveValue, QueryOrder};

pub use super::_entities::sessions::{self, ActiveModel, Column, Entity, Model};
use super::_entities::users;
use crate::db::First;

/// Longest `User-Agent` we keep; anything beyond is truncated before storage.
const MAX_USER_AGENT_LEN: usize = 512;

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let mut this = self;
        if insert && this.token.is_not_set() {
            this.token = ActiveValue::Set(Uuid::new_v4().to_string());
        }
        if !insert && this.updated_at.is_unchanged() {
            this.updated_at = ActiveValue::Set(chrono::Utc::now().into());
        }
        Ok(this)
    }
}

/// Request details recorded on a new session (`Current.user_agent` /
/// `Current.ip_address` in the Rails kit).
#[derive(Debug, Clone, Default)]
pub struct RequestDetails {
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
}

impl Model {
    /// `user.sessions.create!` with the current request's user agent and IP.
    ///
    /// # Errors
    /// Database errors.
    pub async fn create_for_user<C: ConnectionTrait>(
        db: &C,
        user: &users::Model,
        details: &RequestDetails,
    ) -> ModelResult<Self> {
        let user_agent = details
            .user_agent
            .as_ref()
            .map(|ua| ua.chars().take(MAX_USER_AGENT_LEN).collect::<String>());
        Ok(ActiveModel {
            user_id: ActiveValue::Set(user.id),
            token: ActiveValue::Set(Uuid::new_v4().to_string()),
            user_agent: ActiveValue::Set(user_agent),
            ip_address: ActiveValue::Set(details.ip_address.clone()),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// `user.sessions.create!` after `User.authenticate_by`, atomically with a re-check of
    /// the credential: `user` is the record whose password was just verified, and the
    /// session is only created while its `password_digest` is still current. The check
    /// is a conditional write on the user row, so it also takes the row's write lock
    /// until the insert commits; a concurrent password change/reset (which deletes every
    /// session) is ordered entirely before or after this, never in between.
    ///
    /// `Ok(None)` when the password changed since `user` was loaded (nothing is written).
    ///
    /// # Errors
    /// Database errors.
    pub async fn create_for_authenticated_user(
        db: &DatabaseConnection,
        user: &users::Model,
        details: &RequestDetails,
    ) -> ModelResult<Option<Self>> {
        let txn = crate::db::begin_write(db).await?;
        let current = users::Entity::update_many()
            .col_expr(users::Column::Id, Expr::col(users::Column::Id))
            .filter(users::Column::Id.eq(user.id))
            .filter(users::Column::PasswordDigest.eq(user.password_digest.as_str()))
            .exec(&txn)
            .await?;
        if current.rows_affected == 0 {
            txn.rollback().await?;
            return Ok(None);
        }
        let session = Self::create_for_user(&txn, user, details).await?;
        txn.commit().await?;
        Ok(Some(session))
    }

    /// Resolve the session behind a cookie together with its user.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when the token matches nothing.
    pub async fn find_by_token_with_user<C: ConnectionTrait>(
        db: &C,
        token: &str,
    ) -> ModelResult<(Self, users::Model)> {
        let (session, user) = Entity::find()
            .filter(Column::Token.eq(token))
            .find_also_related(users::Entity)
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;
        let user = user.ok_or(ModelError::EntityNotFound)?;
        Ok((session, user))
    }

    /// `Current.user.sessions.order(created_at: :desc)`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn list_for_user<C: ConnectionTrait>(db: &C, user_id: i64) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .filter(Column::UserId.eq(user_id))
            .order_by_desc(Column::CreatedAt)
            .order_by_desc(Column::Id)
            .all(db)
            .await?)
    }

    /// `Current.user.sessions.find(params[:id]).destroy!` — scoped to the user,
    /// so another user's session token is a 404, not a deletion.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when the user owns no session with `token`.
    pub async fn destroy_for_user<C: ConnectionTrait>(
        db: &C,
        user_id: i64,
        token: &str,
    ) -> ModelResult<Self> {
        let session = Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::Token.eq(token))
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;
        Entity::delete_by_id(session.id).exec(db).await?;
        Ok(session)
    }
}
