//! `Account`: the organization. Its slug is the first URL segment (`/{account_slug}/…`),
//! generated from the name on create and never changed afterwards (Rails' `attr_readonly`).

use std::sync::LazyLock;

use loco_rs::model::{ModelError, ModelResult};
use regex::Regex;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue, QueryOrder};
use serde::Deserialize;
use serde_json::json;

pub use super::_entities::accounts::{ActiveModel, Column, Entity, Model};
use super::{
    _entities::memberships,
    cast,
    memberships::Role,
    users::{Errors, SaveError},
};
use crate::db::First;
pub type Accounts = Entity;

/// `validates :name, length: { maximum: 60 }`.
pub const NAME_MAX: usize = 60;
/// The longest slug (`[a-z0-9-]{3,40}`).
pub const SLUG_MAX: usize = 40;

/// The slug format, also the route constraint on `/{account_slug}` (`Account::SLUG_FORMAT`).
static SLUG_FORMAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9-]{3,40}$").expect("SLUG_FORMAT is a valid regex"));

/// Top-level paths a slug would shadow; a name that parameterizes to one gets a `-2` suffix.
/// The same list as the Rails app, so both pick the same slug for the same name.
pub const RESERVED_SLUGS: &[&str] = &[
    "accounts",
    "live",
    "invitations",
    "dashboard",
    "settings",
    "identity",
    "sessions",
    "users",
    "rails",
    "assets",
    "vite",
];

#[must_use]
pub fn is_slug(s: &str) -> bool {
    SLUG_FORMAT.is_match(s)
}

/// Rails' `String#parameterize`: transliterate to ASCII, lowercase, every run of other
/// characters becomes one `-`, no leading or trailing `-`. Unlike Rails, `_` is a separator
/// too, so the result always fits the slug format (SPEC-C1.md, Divergences).
#[must_use]
pub fn parameterize(name: &str) -> String {
    let ascii = deunicode::deunicode(name).to_lowercase();
    let mut out = String::with_capacity(ascii.len());
    for c in ascii.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// The slug part before the collision suffix: parameterized, cut to 40, padded with
/// `-account` when shorter than 3 (`"X"` is `x-account`, `""` is `account`).
fn slug_base(name: &str) -> String {
    let base: String = parameterize(name).chars().take(SLUG_MAX).collect();
    let base = base.trim_end_matches('-').to_owned();
    if base.len() >= 3 {
        base
    } else if base.is_empty() {
        "account".to_owned()
    } else {
        format!("{base}-account")
    }
}

/// `before_validation :assign_slug, on: :create`: the base, then `-2`, `-3`, … while it is
/// taken or reserved.
async fn unique_slug<C: ConnectionTrait>(db: &C, name: &str) -> ModelResult<String> {
    let base = slug_base(name);
    let mut slug = base.clone();
    let mut suffix = 1;
    while RESERVED_SLUGS.contains(&slug.as_str()) || slug_taken(db, &slug).await? {
        suffix += 1;
        let tail = format!("-{suffix}");
        let head: String = base.chars().take(SLUG_MAX - tail.len()).collect();
        slug = format!("{}{tail}", head.trim_end_matches('-'));
    }
    Ok(slug)
}

async fn slug_taken<C: ConnectionTrait>(db: &C, slug: &str) -> ModelResult<bool> {
    Ok(Entity::find()
        .filter(Column::Slug.eq(slug))
        .first(db)
        .await?
        .is_some())
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    async fn before_save<C>(self, _db: &C, insert: bool) -> std::result::Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if !insert && self.updated_at.is_unchanged() {
            let mut this = self;
            this.updated_at = sea_orm::ActiveValue::Set(chrono::Utc::now().into());
            Ok(this)
        } else {
            Ok(self)
        }
    }
}

/// `params.permit(:name)`. The slug is not a param: it is generated, and not editable.
#[derive(Debug, Default, Deserialize)]
pub struct AccountParams {
    #[serde(default, deserialize_with = "cast::form_value")]
    pub name: Option<String>,
}

impl AccountParams {
    /// Cast each attribute to its column type and assign it to `item`, returning the
    /// validation errors (Rails' `errors` after `valid?`).
    fn assign(&self, item: &mut ActiveModel) -> Errors {
        let mut errors = Errors::new();
        let name = cast::string(&mut errors, "name", self.name.as_deref());
        if name.chars().count() > NAME_MAX {
            errors.add(
                "name",
                format!("is too long (maximum is {NAME_MAX} characters)"),
            );
        }
        item.name = ActiveValue::Set(name);
        errors
    }

    /// The validation errors, without writing (Precognition).
    #[must_use]
    pub fn errors(&self) -> Errors {
        self.assign(&mut <ActiveModel as Default>::default())
    }
}

impl Model {
    /// # Errors
    /// `ModelError::EntityNotFound` (a 404) when there is no such account.
    pub async fn find_by_slug<C: ConnectionTrait>(db: &C, slug: &str) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::Slug.eq(slug))
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// `user.accounts.order(:name)`: every account `user_id` is a member of.
    ///
    /// # Errors
    /// Database errors.
    pub async fn list_for_user<C: ConnectionTrait>(db: &C, user_id: i64) -> ModelResult<Vec<Self>> {
        Ok(Entity::find()
            .inner_join(memberships::Entity)
            .filter(memberships::Column::UserId.eq(user_id))
            .order_by_asc(Column::Name)
            .order_by_asc(Column::Id)
            .all(db)
            .await?)
    }

    /// `user.default_account`: the account visited last if the user is still in it, else the
    /// first one joined, else `None`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn default_for_user<C: ConnectionTrait>(
        db: &C,
        user_id: i64,
        last_account_id: Option<i64>,
    ) -> ModelResult<Option<Self>> {
        let mine = || {
            Entity::find()
                .inner_join(memberships::Entity)
                .filter(memberships::Column::UserId.eq(user_id))
        };
        if let Some(id) = last_account_id {
            if let Some(account) = mine().filter(Column::Id.eq(id)).first(db).await? {
                return Ok(Some(account));
            }
        }
        Ok(mine()
            .order_by_asc(memberships::Column::CreatedAt)
            .order_by_asc(memberships::Column::Id)
            .first(db)
            .await?)
    }

    /// `Account.create(params.merge(owner: user))`: the account and its owner membership, in
    /// one transaction.
    ///
    /// # Errors
    /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
    pub async fn create_with_owner(
        db: &DatabaseConnection,
        params: &AccountParams,
        owner_id: i64,
    ) -> Result<Self, SaveError> {
        let txn = crate::db::begin_write(db).await?;
        let account = Self::create_in(&txn, params, owner_id).await?;
        txn.commit().await?;
        Ok(account)
    }

    /// [`Self::create_with_owner`] inside the caller's transaction (sign-up creates the user
    /// and the personal account together).
    ///
    /// # Errors
    /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
    pub async fn create_in<C: ConnectionTrait>(
        db: &C,
        params: &AccountParams,
        owner_id: i64,
    ) -> Result<Self, SaveError> {
        let mut item = <ActiveModel as Default>::default();
        let errors = params.assign(&mut item);
        if !errors.is_empty() {
            return Err(SaveError::Invalid(errors));
        }
        item.slug = ActiveValue::Set(unique_slug(db, item.name.as_ref()).await?);
        let account = item.insert(db).await?;
        memberships::ActiveModel {
            account_id: ActiveValue::Set(account.id),
            user_id: ActiveValue::Set(owner_id),
            role: ActiveValue::Set(Role::Owner.as_str().to_owned()),
            ..Default::default()
        }
        .insert(db)
        .await?;
        Ok(account)
    }

    /// The personal account a user gets on sign-up without an invitation:
    /// `"<name>'s account"` (the name cut to 50 characters so it fits 60), with the user as
    /// owner.
    ///
    /// # Errors
    /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
    pub async fn create_personal<C: ConnectionTrait>(
        db: &C,
        user: &super::users::Model,
    ) -> Result<Self, SaveError> {
        let name: String = user.name.chars().take(50).collect();
        Self::create_in(
            db,
            &AccountParams {
                name: Some(format!("{name}'s account")),
            },
            user.id,
        )
        .await
    }

    /// `account.update(params)`: the name only.
    ///
    /// # Errors
    /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
    pub async fn update(
        self,
        db: &DatabaseConnection,
        params: &AccountParams,
    ) -> Result<Self, SaveError> {
        let mut item: ActiveModel = self.into();
        let errors = params.assign(&mut item);
        if !errors.is_empty() {
            return Err(SaveError::Invalid(errors));
        }
        Ok(item.update(db).await?)
    }

    /// The page props for this account: `{id, name, slug}` (the frontend's `Account` type).
    #[must_use]
    pub fn to_props(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "name": self.name,
            "slug": self.slug,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameterize_matches_rails_for_ordinary_names() {
        assert_eq!(parameterize("Acme"), "acme");
        assert_eq!(parameterize("  Acme, Inc.  "), "acme-inc");
        assert_eq!(parameterize("Café Crème"), "cafe-creme");
        assert_eq!(parameterize("Ann's account"), "ann-s-account");
        assert_eq!(parameterize("a_b"), "a-b");
    }

    #[test]
    fn slug_base_pads_short_names_and_cuts_long_ones() {
        assert_eq!(slug_base("X"), "x-account");
        assert_eq!(slug_base("!!"), "account");
        let long = "a".repeat(39) + " b";
        assert_eq!(slug_base(&long), "a".repeat(39));
        assert!(is_slug(&slug_base(&"word ".repeat(20))));
    }
}
