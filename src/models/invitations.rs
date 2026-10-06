//! `Invitation`: an email invited into an account with a role. Only a SHA-256 digest of the
//! token is stored; the plain token exists in memory just long enough to be mailed.

use chrono::{DateTime, Duration, Utc};
use loco_rs::model::{ModelError, ModelResult};
use rand::Rng;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, QueryOrder, TransactionSession, TransactionTrait};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

pub use super::_entities::invitations::{ActiveModel, Column, Entity, Model};
use super::{
    _entities::{accounts, users},
    memberships::{self, Role},
    users::{normalize_email, validate_email_format, Errors, SaveError},
};
use crate::db::First;
pub type Invitations = Entity;

/// `VALID_FOR = 7.days`.
#[must_use]
pub fn valid_for() -> Duration {
    Duration::days(7)
}

/// A new random token: 24 random bytes as 48 hex characters (URL-safe, survives mail encoding).
#[must_use]
pub fn generate_token() -> String {
    let mut bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// The plain token of the seeded invitation to `three@example.com` (the Rails fixture's
/// `"three-token"`): only its digest is stored.
pub const SEED_TOKEN: &str = "three-token";

/// `Invitation.digest(token)`: lowercase hex SHA-256.
#[must_use]
pub fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        let mut this = self;
        if let ActiveValue::Set(email) = &this.email {
            let normalized = normalize_email(email);
            if &normalized != email {
                this.email = ActiveValue::Set(normalized);
            }
        }
        if !insert && this.updated_at.is_unchanged() {
            this.updated_at = sea_orm::ActiveValue::Set(chrono::Utc::now().into());
        }
        Ok(this)
    }
}

/// `params.permit(:email, :role)`.
#[derive(Debug, Default, Deserialize)]
pub struct InvitationParams {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    pub email: String,
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    pub role: String,
}

impl InvitationParams {
    /// The submitted role. Blank is invalid, as in Rails, where the enum casts `""` to nil.
    fn role(&self) -> &str {
        self.role.as_str()
    }

    /// `invitation.valid?` for `account_id` at `now`: format, role, "has already been
    /// invited", "is already a member". Reads only, so precognition can use it.
    ///
    /// # Errors
    /// Database errors.
    pub async fn errors<C: ConnectionTrait>(
        &self,
        db: &C,
        account_id: i64,
        now: DateTime<Utc>,
    ) -> ModelResult<Errors> {
        let email = normalize_email(&self.email);
        let mut errors = Errors::new();
        validate_email_format(&email, &mut errors);
        if !matches!(Role::parse(self.role()), Some(Role::Admin | Role::Member)) {
            errors.add("role", "is not included in the list");
        }
        if errors.get("email").is_none() {
            if Model::pending_for_email(db, account_id, &email, now)
                .await?
                .is_some()
            {
                errors.add("email", "has already been invited");
            }
            if memberships::Model::email_is_member(db, account_id, &email).await? {
                errors.add("email", "is already a member");
            }
        }
        Ok(errors)
    }
}

/// An invitation with the names its pages and mail show.
#[derive(Debug, Clone)]
pub struct Details {
    pub invitation: Model,
    pub account: accounts::Model,
    pub inviter: users::Model,
}

impl Model {
    /// Not accepted and not expired at `now` (`pending?`).
    #[must_use]
    pub fn is_pending(&self, now: DateTime<Utc>) -> bool {
        self.accepted_at.is_none() && self.expires_at > now
    }

    #[must_use]
    pub fn role(&self) -> Role {
        Role::parse(&self.role).unwrap_or(Role::Member)
    }

    /// `account.invitations.pending.find_by(email:)`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn pending_for_email<C: ConnectionTrait>(
        db: &C,
        account_id: i64,
        email: &str,
        now: DateTime<Utc>,
    ) -> ModelResult<Option<Self>> {
        Ok(Entity::find()
            .filter(Column::AccountId.eq(account_id))
            .filter(Column::Email.eq(normalize_email(email)))
            .filter(Column::AcceptedAt.is_null())
            .filter(Column::ExpiresAt.gt(now))
            .first(db)
            .await?)
    }

    /// `account.invitations.pending.includes(:inviter).order(:created_at)`, as page props
    /// `{id, email, role, expires_at, inviter_name}`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn pending_props<C: ConnectionTrait>(
        db: &C,
        account_id: i64,
        now: DateTime<Utc>,
    ) -> ModelResult<Vec<PendingInvitationProps>> {
        Ok(Entity::find()
            .filter(Column::AccountId.eq(account_id))
            .filter(Column::AcceptedAt.is_null())
            .filter(Column::ExpiresAt.gt(now))
            .order_by_asc(Column::CreatedAt)
            .order_by_asc(Column::Id)
            .find_also_related(users::Entity)
            .all(db)
            .await?
            .into_iter()
            .map(|(invitation, inviter)| PendingInvitationProps {
                id: invitation.id,
                role: invitation.role(),
                expires_at: super::as_json_time(&invitation.expires_at),
                email: invitation.email,
                inviter_name: inviter.map(|u| u.name),
            })
            .collect())
    }

    /// `Current.account.invitations.pending.find(id)`.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when there is no such pending invitation in the account.
    pub async fn find_pending_in_account<C: ConnectionTrait>(
        db: &C,
        account_id: i64,
        id: i64,
        now: DateTime<Utc>,
    ) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .filter(Column::AccountId.eq(account_id))
            .filter(Column::AcceptedAt.is_null())
            .filter(Column::ExpiresAt.gt(now))
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// `Invitation.find_by_token!(token)`, with its account and inviter.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` (a 404) for an unknown token.
    pub async fn find_by_token<C: ConnectionTrait>(db: &C, token: &str) -> ModelResult<Details> {
        let invitation = Entity::find()
            .filter(Column::TokenDigest.eq(digest(token)))
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;
        Self::details(db, invitation).await
    }

    /// # Errors
    /// `ModelError::EntityNotFound` when the invitation is gone.
    pub async fn find_details<C: ConnectionTrait>(db: &C, id: i64) -> ModelResult<Details> {
        let invitation = Entity::find_by_id(id)
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;
        Self::details(db, invitation).await
    }

    async fn details<C: ConnectionTrait>(db: &C, invitation: Self) -> ModelResult<Details> {
        let account = accounts::Entity::find_by_id(invitation.account_id)
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;
        let inviter = users::Entity::find_by_id(invitation.inviter_id)
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)?;
        Ok(Details {
            invitation,
            account,
            inviter,
        })
    }

    /// `account.invitations.create(email:, role:, inviter:)`, valid for 7 days from `now`.
    /// The stored digest is of a token nobody holds: the delivery job mints the one it mails
    /// ([`Self::rotate_token`]).
    ///
    /// # Errors
    /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
    pub async fn create(
        db: &DatabaseConnection,
        account_id: i64,
        inviter_id: i64,
        params: &InvitationParams,
        now: DateTime<Utc>,
    ) -> Result<Self, SaveError> {
        let txn = crate::db::begin_write(db).await?;
        let errors = params.errors(&txn, account_id, now).await?;
        if !errors.is_empty() {
            return Err(SaveError::Invalid(errors));
        }
        let invitation = ActiveModel {
            account_id: ActiveValue::Set(account_id),
            inviter_id: ActiveValue::Set(inviter_id),
            email: ActiveValue::Set(normalize_email(&params.email)),
            role: ActiveValue::Set(params.role().to_owned()),
            token_digest: ActiveValue::Set(digest(&generate_token())),
            expires_at: ActiveValue::Set((now + valid_for()).into()),
            ..Default::default()
        }
        .insert(&txn)
        .await?;
        txn.commit().await?;
        Ok(invitation)
    }

    /// Store the digest of a fresh token and return the plain one, for the mail. A link
    /// mailed earlier for this invitation stops working.
    ///
    /// # Errors
    /// Database errors.
    pub async fn rotate_token<C: ConnectionTrait>(self, db: &C) -> ModelResult<(Self, String)> {
        let token = generate_token();
        let mut item: ActiveModel = self.into();
        item.token_digest = ActiveValue::Set(digest(&token));
        Ok((item.update(db).await?, token))
    }

    /// `invitation.accept!(user)`: mark it accepted and add the membership with its role, in
    /// one transaction. Only a pending invitation can be accepted (a second accept of the same
    /// row finds `accepted_at` set and changes nothing).
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when it is no longer pending; database errors.
    pub async fn accept<C: TransactionTrait + ConnectionTrait>(
        &self,
        db: &C,
        user_id: i64,
        now: DateTime<Utc>,
    ) -> ModelResult<memberships::Model> {
        let txn = crate::db::begin_write(db).await?;
        let updated = Entity::update_many()
            .col_expr(
                Column::AcceptedAt,
                Expr::value(DateTimeWithTimeZone::from(now)),
            )
            .col_expr(
                Column::UpdatedAt,
                Expr::value(DateTimeWithTimeZone::from(now)),
            )
            .filter(Column::Id.eq(self.id))
            .filter(Column::AcceptedAt.is_null())
            .filter(Column::ExpiresAt.gt(now))
            .exec(&txn)
            .await?;
        if updated.rows_affected == 0 {
            return Err(ModelError::EntityNotFound);
        }
        let membership =
            memberships::Model::find_or_create(&txn, self.account_id, user_id, self.role()).await?;
        txn.commit().await?;
        Ok(membership)
    }

    /// Seeds: a pending invitation with a known `token`, valid for 7 days from now, unless
    /// one is already pending for that address.
    ///
    /// # Errors
    /// Database errors.
    pub async fn seed_pending(
        db: &DatabaseConnection,
        account_id: i64,
        inviter_id: i64,
        email: &str,
        role: Role,
        token: &str,
    ) -> ModelResult<Self> {
        let now = Utc::now();
        if let Some(existing) = Self::pending_for_email(db, account_id, email, now).await? {
            return Ok(existing);
        }
        Ok(ActiveModel {
            account_id: ActiveValue::Set(account_id),
            inviter_id: ActiveValue::Set(inviter_id),
            email: ActiveValue::Set(normalize_email(email)),
            role: ActiveValue::Set(role.as_str().to_owned()),
            token_digest: ActiveValue::Set(digest(token)),
            expires_at: ActiveValue::Set((now + valid_for()).into()),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// `invitation.destroy!`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn revoke(self, db: &DatabaseConnection) -> ModelResult<()> {
        self.delete(db).await?;
        Ok(())
    }
}

/// A pending invitation on the members page (managers only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct PendingInvitationProps {
    pub id: i64,
    pub email: String,
    pub role: Role,
    // ISO 8601, UTC.
    pub expires_at: String,
    // `None` (null) when the inviter's user is gone.
    pub inviter_name: Option<String>,
}
