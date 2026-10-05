//! `Membership`: a user in an account, with a role. An account always keeps an owner.

use loco_rs::model::{ModelError, ModelResult};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, QueryOrder, QuerySelect};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub use super::_entities::memberships::{ActiveModel, Column, Entity, Model};
use super::{
    _entities::users,
    users::{Errors, SaveError},
};
use crate::db::First;
pub type Memberships = Entity;

/// "An account needs at least one owner" (`Membership::LAST_OWNER_MESSAGE`).
pub const LAST_OWNER_MESSAGE: &str = "An account needs at least one owner";

/// `enum :role, %w[owner admin member]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Self::Owner),
            "admin" => Some(Self::Admin),
            "member" => Some(Self::Member),
            _ => None,
        }
    }
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if let ActiveValue::Set(role) = &self.role {
            if Role::parse(role).is_none() {
                return Err(DbErr::Custom(format!("invalid membership role {role:?}")));
            }
        }
        if !insert && self.updated_at.is_unchanged() {
            let mut this = self;
            this.updated_at = sea_orm::ActiveValue::Set(chrono::Utc::now().into());
            Ok(this)
        } else {
            Ok(self)
        }
    }
}

/// A membership with its user, for the members page.
#[derive(Debug, Clone)]
pub struct Member {
    pub membership: Model,
    pub user: users::Model,
}

impl Member {
    /// `{id, user: {name, email}, role, joined_at}`.
    #[must_use]
    pub fn to_props(&self) -> serde_json::Value {
        json!({
            "id": self.membership.id,
            "user": { "name": self.user.name, "email": self.user.email },
            "role": self.membership.role,
            "joined_at": super::as_json_time(&self.membership.created_at),
        })
    }
}

/// Tell the account's open pages that its members changed (`AccountChannel`, after commit):
/// the members page reloads them.
pub fn members_changed(account_id: i64) {
    crate::channels::account::AccountChannel::broadcast_to(
        account_id,
        serde_json::json!({ "type": "members" }),
    );
}

/// The base error a role rule failed with: shown as the alert flash.
fn invalid(message: &str) -> SaveError {
    let mut errors = Errors::new();
    errors.add("base", message);
    SaveError::Invalid(errors)
}

impl Model {
    #[must_use]
    pub fn role(&self) -> Role {
        Role::parse(&self.role).unwrap_or(Role::Member)
    }

    /// Owners and admins manage members and invitations.
    #[must_use]
    pub fn is_manager(&self) -> bool {
        matches!(self.role(), Role::Owner | Role::Admin)
    }

    /// `Current.user.memberships.find_by(account:)`.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when the user is not a member.
    pub async fn find_for<C: ConnectionTrait>(
        db: &C,
        account_id: i64,
        user_id: i64,
    ) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::AccountId.eq(account_id))
            .filter(Column::UserId.eq(user_id))
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// `Current.account.memberships.find(id)`: another account's id is not found.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when no such membership in the account.
    pub async fn find_in_account<C: ConnectionTrait>(
        db: &C,
        account_id: i64,
        id: i64,
    ) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .filter(Column::AccountId.eq(account_id))
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// `account.memberships.includes(:user).order(:created_at, :id)`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn members_of<C: ConnectionTrait>(
        db: &C,
        account_id: i64,
    ) -> ModelResult<Vec<Member>> {
        Ok(Entity::find()
            .filter(Column::AccountId.eq(account_id))
            .order_by_asc(Column::CreatedAt)
            .order_by_asc(Column::Id)
            .find_also_related(users::Entity)
            .all(db)
            .await?
            .into_iter()
            .filter_map(|(membership, user)| user.map(|user| Member { membership, user }))
            .collect())
    }

    /// `account.memberships.count`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn count_in<C: ConnectionTrait>(db: &C, account_id: i64) -> ModelResult<u64> {
        Ok(Entity::find()
            .filter(Column::AccountId.eq(account_id))
            .count(db)
            .await?)
    }

    /// Whether `email` belongs to a member of the account (`account.users.exists?(email:)`).
    ///
    /// # Errors
    /// Database errors.
    pub async fn email_is_member<C: ConnectionTrait>(
        db: &C,
        account_id: i64,
        email: &str,
    ) -> ModelResult<bool> {
        Ok(Entity::find()
            .inner_join(users::Entity)
            .filter(Column::AccountId.eq(account_id))
            .filter(users::Column::Email.eq(email))
            .select_only()
            .column(Column::Id)
            .into_tuple::<i64>()
            .one(db)
            .await?
            .is_some())
    }

    /// `account.memberships.find_or_create_by!(user:) { _1.role = role }`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn find_or_create<C: ConnectionTrait>(
        db: &C,
        account_id: i64,
        user_id: i64,
        role: Role,
    ) -> ModelResult<Self> {
        match Self::find_for(db, account_id, user_id).await {
            Ok(existing) => Ok(existing),
            Err(ModelError::EntityNotFound) => Ok(ActiveModel {
                account_id: ActiveValue::Set(account_id),
                user_id: ActiveValue::Set(user_id),
                role: ActiveValue::Set(role.as_str().to_owned()),
                ..Default::default()
            }
            .insert(db)
            .await?),
            Err(err) => Err(err),
        }
    }

    /// Whether another owner than this membership exists. Runs inside the caller's
    /// transaction so two demotions can't both see the other owner.
    async fn other_owner_exists<C: ConnectionTrait>(&self, db: &C) -> ModelResult<bool> {
        Ok(Entity::find()
            .filter(Column::AccountId.eq(self.account_id))
            .filter(Column::Role.eq(Role::Owner.as_str()))
            .filter(Column::Id.ne(self.id))
            .first(db)
            .await?
            .is_some())
    }

    /// `membership.update(role:)`: `role` must be one of the roles, and the last owner can't
    /// be demoted.
    ///
    /// # Errors
    /// `SaveError::Invalid` with `base: ["Role is not included in the list"]` or
    /// [`LAST_OWNER_MESSAGE`], or `SaveError::Model`.
    pub async fn change_role(self, db: &DatabaseConnection, role: &str) -> Result<Self, SaveError> {
        let Some(role) = Role::parse(role) else {
            return Err(invalid("Role is not included in the list"));
        };
        let txn = crate::db::begin_write(db).await?;
        if self.role() == Role::Owner
            && role != Role::Owner
            && !self.other_owner_exists(&txn).await?
        {
            return Err(invalid(LAST_OWNER_MESSAGE));
        }
        let mut item: ActiveModel = self.into();
        item.role = ActiveValue::Set(role.as_str().to_owned());
        let updated = item.update(&txn).await?;
        txn.commit().await?;
        members_changed(updated.account_id);
        Ok(updated)
    }

    /// `membership.destroy`: refused for the last owner.
    ///
    /// # Errors
    /// `SaveError::Invalid` with [`LAST_OWNER_MESSAGE`], or `SaveError::Model`.
    pub async fn remove(self, db: &DatabaseConnection) -> Result<(), SaveError> {
        let txn = crate::db::begin_write(db).await?;
        if self.role() == Role::Owner && !self.other_owner_exists(&txn).await? {
            return Err(invalid(LAST_OWNER_MESSAGE));
        }
        let account_id = self.account_id;
        self.delete(&txn).await?;
        txn.commit().await?;
        members_changed(account_id);
        Ok(())
    }
}
