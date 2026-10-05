use std::{collections::BTreeMap, sync::LazyLock};

use loco_rs::{hash, prelude::*};
use regex::Regex;
use sea_orm::ActiveValue;
use serde::{Deserialize, Serialize};

pub use super::_entities::users::{self, ActiveModel, Column, Entity, Model};
use super::{
    _entities::sessions,
    tokens::{self, Clock, Purpose},
};
use crate::db::First;

/// Minimum password length (Rails kit: `validates :password, length: { minimum: 12 }`).
pub const PASSWORD_MIN_LENGTH: usize = 12;

/// `has_secure_password`'s limit, in bytes (`ActiveModel::SecurePassword::MAX_PASSWORD_LENGTH_ALLOWED`):
/// bcrypt ignores anything past 72 bytes, so Rails rejects longer passwords with "is too long".
/// Argon2 has no such limit, but the rule is part of the validation behaviour we match.
pub const PASSWORD_MAX_BYTES: usize = 72;

/// Rails' `URI::MailTo::EMAIL_REGEXP`, translated to the `regex` crate
/// (`\A`/`\z` become `^`/`$`, which anchor the whole input without the `m` flag).
static EMAIL_REGEXP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[a-zA-Z0-9.!\#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*$",
    )
    .expect("EMAIL_REGEXP is a valid regex")
});

/// An argon2id hash (default params, same as `loco_rs::hash`) of a random
/// throwaway password. `authenticate_by` verifies against it when no user
/// matches the email, so "unknown email" and "wrong password" cost the same.
static DUMMY_PASSWORD_DIGEST: LazyLock<String> =
    LazyLock::new(|| hash::hash_password(&hash::random_string(32)).unwrap_or_default());

// ---------------------------------------------------------------------------
// Validation errors
// ---------------------------------------------------------------------------

/// Rails-style `errors.to_hash`: attribute → list of messages, in a stable
/// order. This is exactly the shape Inertia's `errors` prop carries.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Errors(BTreeMap<String, Vec<String>>);

impl Errors {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, attribute: &str, message: impl Into<String>) {
        self.0
            .entry(attribute.to_string())
            .or_default()
            .push(message.into());
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn get(&self, attribute: &str) -> Option<&[String]> {
        self.0.get(attribute).map(Vec::as_slice)
    }

    #[must_use]
    pub fn into_inner(self) -> BTreeMap<String, Vec<String>> {
        self.0
    }

    fn into_result(self) -> Result<(), SaveError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(SaveError::Invalid(self))
        }
    }
}

impl From<BTreeMap<String, Vec<String>>> for Errors {
    fn from(map: BTreeMap<String, Vec<String>>) -> Self {
        Self(map)
    }
}

/// Outcome of a save that can fail validation (`user.update(...)` returning
/// false in Rails) or fail for an infrastructure reason.
#[derive(Debug)]
pub enum SaveError {
    /// Validation failed; show these to the user.
    Invalid(Errors),
    /// The credential this change was authorized against (the password digest
    /// that was checked) is no longer current: a concurrent change won.
    Stale,
    /// Anything else (database, hashing). Propagate as a 500.
    Model(ModelError),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(errors) => write!(f, "validation failed: {errors:?}"),
            Self::Stale => f.write_str("the password changed concurrently"),
            Self::Model(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<ModelError> for SaveError {
    fn from(err: ModelError) -> Self {
        Self::Model(err)
    }
}

impl From<DbErr> for SaveError {
    fn from(err: DbErr) -> Self {
        Self::Model(ModelError::DbErr(err))
    }
}

impl From<SaveError> for loco_rs::Error {
    fn from(err: SaveError) -> Self {
        match err {
            SaveError::Invalid(errors) => Self::BadRequest(format!("{errors:?}")),
            SaveError::Stale => Self::BadRequest(SaveError::Stale.to_string()),
            SaveError::Model(err) => Self::Model(err),
        }
    }
}

// ---------------------------------------------------------------------------
// Attribute rules (the `validates` lines of app/models/user.rb)
// ---------------------------------------------------------------------------

/// `normalizes :email, with: -> { _1.strip.downcase }`. Ruby's `String#strip` removes only
/// ASCII whitespace and NUL (not U+00A0 or other Unicode spaces, which Rust's `trim` would);
/// `downcase` is full Unicode, like `to_lowercase`.
#[must_use]
pub fn normalize_email(email: &str) -> String {
    email
        .trim_matches(['\0', '\t', '\n', '\x0b', '\x0c', '\r', ' '])
        .to_lowercase()
}

fn validate_name(name: &str, errors: &mut Errors) {
    if name.trim().is_empty() {
        errors.add("name", "can't be blank");
    }
}

pub(crate) fn validate_email_format(email: &str, errors: &mut Errors) {
    if email.is_empty() {
        errors.add("email", "can't be blank");
    }
    if !EMAIL_REGEXP.is_match(email) {
        errors.add("email", "is invalid");
    }
}

/// The password a save would actually assign. `has_secure_password`'s `password=` ignores
/// `""` (and a missing param never calls it), so both leave the digest untouched.
fn new_password(password: Option<&str>) -> Option<&str> {
    password.filter(|p| !p.is_empty())
}

/// `has_secure_password` + `validates :password, allow_nil: true, length: { minimum: 12 }`.
/// With no new password (see [`new_password`]) only a record without a digest (create) is
/// invalid, and `validates_confirmation_of … allow_nil: true` is skipped. `confirmation` is
/// only checked when supplied, exactly like Rails.
fn validate_password(
    password: Option<&str>,
    confirmation: Option<&str>,
    on_create: bool,
    errors: &mut Errors,
) {
    let Some(password) = new_password(password) else {
        if on_create {
            errors.add("password", "can't be blank");
        }
        return;
    };
    if password.len() > PASSWORD_MAX_BYTES {
        errors.add("password", "is too long");
    }
    if password.chars().count() < PASSWORD_MIN_LENGTH {
        errors.add(
            "password",
            format!("is too short (minimum is {PASSWORD_MIN_LENGTH} characters)"),
        );
    }
    if let Some(confirmation) = confirmation {
        if confirmation != password {
            errors.add("password_confirmation", "doesn't match Password");
        }
    }
}

/// The challenge was checked against a password digest that is no longer current (a
/// concurrent reset or change won): report it like a wrong challenge, with `message`.
fn stale_challenge(message: &str) -> SaveError {
    let mut errors = Errors::new();
    errors.add("password_challenge", message);
    SaveError::Invalid(errors)
}

fn hash(password: &str) -> Result<String, SaveError> {
    hash::hash_password(password).map_err(|e| SaveError::Model(ModelError::Any(e.into())))
}

/// Uniqueness is checked inside the caller's transaction; the unique index on
/// `users.email` is the final backstop against a race.
async fn email_taken<C: ConnectionTrait>(
    db: &C,
    email: &str,
    except_id: Option<i64>,
) -> ModelResult<bool> {
    let mut query = Entity::find().filter(Column::Email.eq(email));
    if let Some(id) = except_id {
        query = query.filter(Column::Id.ne(id));
    }
    Ok(query.first(db).await?.is_some())
}

/// Validator used by `before_save` so an invalid row can never be persisted,
/// even by code that bypasses the named model methods (invariants live on the
/// model). The named methods below produce the user-facing Rails messages.
#[derive(Debug, Validate)]
pub struct Validator {
    #[validate(length(min = 1, message = "can't be blank"))]
    pub name: String,
    #[validate(regex(path = *EMAIL_REGEXP, message = "is invalid"))]
    pub email: String,
}

impl Validatable for ActiveModel {
    fn validator(&self) -> Box<dyn Validate> {
        Box::new(Validator {
            name: self.name.as_ref().trim().to_string(),
            email: self.email.as_ref().clone(),
        })
    }
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
        this.validate()?;
        if !insert && this.updated_at.is_unchanged() {
            this.updated_at = ActiveValue::Set(chrono::Utc::now().into());
        }
        Ok(this)
    }
}

// ---------------------------------------------------------------------------
// Params
// ---------------------------------------------------------------------------

/// `params.permit(:email, :name, :password, :password_confirmation)`. An explicit `null`
/// fails validation exactly like `""` ("can't be blank").
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SignUpParams {
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    pub name: String,
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    pub email: String,
    #[serde(default, deserialize_with = "crate::controllers::null_default")]
    pub password: String,
    #[serde(default)]
    pub password_confirmation: Option<String>,
}

/// Where [`Model::sign_up_with_account`] put the new user: the id of the account it joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedUpInto {
    /// Accepted the invitation into this account.
    Invitation(i64),
    /// Owns this new personal account.
    PersonalAccount(i64),
}

/// `params.permit(:password, :password_confirmation)`. A missing (or `""`) password leaves
/// the current one in place, like `user.update` with `has_secure_password`; an explicit
/// `null` is `password = nil`, which clears the digest and fails with "can't be blank".
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PasswordParams {
    #[serde(default, deserialize_with = "crate::controllers::nullable")]
    pub password: Option<Option<String>>,
    #[serde(default)]
    pub password_confirmation: Option<String>,
}

impl PasswordParams {
    /// The new password, if one was given (missing, `""` and `null` are not).
    fn new_password(&self) -> Option<&str> {
        new_password(self.password.as_ref().and_then(Option::as_deref))
    }

    /// `password: null` (`password = nil`): the digest would be cleared.
    fn is_nil(&self) -> bool {
        matches!(self.password, Some(None))
    }
}

// ---------------------------------------------------------------------------
// Finders, creation, authentication, state transitions
// ---------------------------------------------------------------------------

impl Model {
    /// # Errors
    /// `ModelError::EntityNotFound` when no such user.
    pub async fn find_by_id<C: ConnectionTrait>(db: &C, id: i64) -> ModelResult<Self> {
        Entity::find_by_id(id)
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// Looks up by the normalized email (Rails normalizes `find_by` args too).
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when no such user.
    pub async fn find_by_email<C: ConnectionTrait>(db: &C, email: &str) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::Email.eq(normalize_email(email)))
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// `User.find_by(email:, verified: true)` — password resets are only
    /// offered to verified addresses.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when no verified user has this email.
    pub async fn find_verified_by_email<C: ConnectionTrait>(
        db: &C,
        email: &str,
    ) -> ModelResult<Self> {
        Entity::find()
            .filter(Column::Email.eq(normalize_email(email)))
            .filter(Column::Verified.eq(true))
            .first(db)
            .await?
            .ok_or(ModelError::EntityNotFound)
    }

    /// The validation errors [`Self::sign_up`] would report, without writing anything
    /// (`User.new(params).valid?` — used for precognition).
    ///
    /// # Errors
    /// Database errors.
    pub async fn sign_up_errors<C: ConnectionTrait>(
        db: &C,
        params: &SignUpParams,
    ) -> ModelResult<Errors> {
        let email = normalize_email(&params.email);
        let mut errors = Errors::new();
        validate_name(&params.name, &mut errors);
        validate_email_format(&email, &mut errors);
        validate_password(
            Some(&params.password),
            params.password_confirmation.as_deref(),
            true,
            &mut errors,
        );
        if !email.is_empty() && email_taken(db, &email, None).await? {
            errors.add("email", "has already been taken");
        }
        Ok(errors)
    }

    /// Validate and create a user (`User.new(params).save`).
    ///
    /// # Errors
    /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
    pub async fn sign_up(
        db: &DatabaseConnection,
        params: &SignUpParams,
    ) -> Result<Self, SaveError> {
        let txn = crate::db::begin_write(db).await?;
        let user = Self::insert_signed_up(&txn, params, false).await?;
        txn.commit().await?;
        Ok(user)
    }

    /// `UsersController#create`: the user, and in the same transaction either the membership
    /// `invitation` grants (when it is pending and was sent to this address: the invite proves
    /// the address, so the user starts verified) or a personal account
    /// `"<name>'s account"` with the user as owner.
    ///
    /// # Errors
    /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
    pub async fn sign_up_with_account(
        db: &DatabaseConnection,
        params: &SignUpParams,
        invitation: Option<&super::invitations::Model>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(Self, SignedUpInto), SaveError> {
        let email = normalize_email(&params.email);
        let invitation =
            invitation.filter(|invitation| invitation.email == email && invitation.is_pending(now));
        let txn = crate::db::begin_write(db).await?;
        let user = Self::insert_signed_up(&txn, params, invitation.is_some()).await?;
        let into = match invitation {
            Some(invitation) => {
                invitation.accept(&txn, user.id, now).await?;
                SignedUpInto::Invitation(invitation.account_id)
            }
            None => {
                let account = super::accounts::Model::create_personal(&txn, &user).await?;
                SignedUpInto::PersonalAccount(account.id)
            }
        };
        txn.commit().await?;
        Ok((user, into))
    }

    async fn insert_signed_up<C: ConnectionTrait>(
        db: &C,
        params: &SignUpParams,
        verified: bool,
    ) -> Result<Self, SaveError> {
        Self::sign_up_errors(db, params).await?.into_result()?;
        Ok(ActiveModel {
            name: ActiveValue::Set(params.name.clone()),
            email: ActiveValue::Set(normalize_email(&params.email)),
            password_digest: ActiveValue::Set(hash(&params.password)?),
            verified: ActiveValue::Set(verified),
            ..Default::default()
        }
        .insert(db)
        .await?)
    }

    /// Remember the account this user visited last (`update_column(:last_account_id, …)`:
    /// no validations, `updated_at` untouched).
    ///
    /// # Errors
    /// Database errors.
    pub async fn remember_account<C: ConnectionTrait>(
        &self,
        db: &C,
        account_id: i64,
    ) -> ModelResult<()> {
        if self.last_account_id != Some(account_id) {
            Entity::update_many()
                .col_expr(Column::LastAccountId, Expr::value(account_id))
                .filter(Column::Id.eq(self.id))
                .exec(db)
                .await?;
        }
        Ok(())
    }

    /// The demo admin (`task seed:demo`): create a verified user, or, when the email is
    /// taken, mark that user verified and give it `password` (signing out its sessions when
    /// the password changes). Idempotent. Same validations as sign-up, minus uniqueness.
    ///
    /// # Errors
    /// `SaveError::Invalid` with Rails-worded messages, or `SaveError::Model`.
    pub async fn upsert_verified(
        db: &DatabaseConnection,
        name: &str,
        email: &str,
        password: &str,
    ) -> Result<Self, SaveError> {
        let email = normalize_email(email);
        let mut errors = Errors::new();
        validate_name(name, &mut errors);
        validate_email_format(&email, &mut errors);
        validate_password(Some(password), None, true, &mut errors);
        errors.into_result()?;

        let txn = crate::db::begin_write(db).await?;
        let existing = Entity::find()
            .filter(Column::Email.eq(email.as_str()))
            .first(&txn)
            .await?;
        let user = match existing {
            None => {
                ActiveModel {
                    name: ActiveValue::Set(name.to_string()),
                    email: ActiveValue::Set(email),
                    password_digest: ActiveValue::Set(hash(password)?),
                    verified: ActiveValue::Set(true),
                    ..Default::default()
                }
                .insert(&txn)
                .await?
            }
            Some(user) if user.verified && user.authenticate(password) => user,
            Some(user) => {
                let password_changed = !user.authenticate(password);
                if password_changed {
                    user.delete_other_sessions(&txn, None).await?;
                }
                let mut active: ActiveModel = user.into();
                if password_changed {
                    active.password_digest = ActiveValue::Set(hash(password)?);
                }
                active.verified = ActiveValue::Set(true);
                active.update(&txn).await?
            }
        };
        txn.commit().await?;
        Ok(user)
    }

    /// `User.authenticate_by(email:, password:)`.
    ///
    /// Always performs exactly one argon2 verification — against a dummy hash
    /// when the email is unknown — so response time does not reveal whether an
    /// account exists.
    ///
    /// # Errors
    /// Database errors only; bad credentials are `Ok(None)`.
    pub async fn authenticate_by<C: ConnectionTrait>(
        db: &C,
        email: &str,
        password: &str,
    ) -> ModelResult<Option<Self>> {
        let user = match Self::find_by_email(db, email).await {
            Ok(user) => Some(user),
            Err(ModelError::EntityNotFound) => None,
            Err(err) => return Err(err),
        };
        match user {
            Some(user) if user.authenticate(password) => Ok(Some(user)),
            Some(_) => Ok(None),
            None => {
                let _ = hash::verify_password(password, &DUMMY_PASSWORD_DIGEST);
                Ok(None)
            }
        }
    }

    /// `user.authenticate(password)`.
    #[must_use]
    pub fn authenticate(&self, password: &str) -> bool {
        hash::verify_password(password, &self.password_digest)
    }

    /// The fingerprint a token for `purpose` is bound to.
    fn token_fingerprint(&self, purpose: Purpose) -> String {
        match purpose {
            Purpose::EmailVerification => self.email.clone(),
            // `password_salt.last(10)`: the tail of the PHC string lives in the
            // hash segment, which changes whenever the password (and salt) does.
            Purpose::PasswordReset => {
                let digest = &self.password_digest;
                let start = digest.len().saturating_sub(10);
                digest.get(start..).unwrap_or_default().to_string()
            }
        }
    }

    /// `user.generate_token_for(purpose)`.
    #[must_use]
    pub fn generate_token_for(
        &self,
        purpose: Purpose,
        secret_key_base: &[u8],
        clock: &dyn Clock,
    ) -> String {
        tokens::generate(
            secret_key_base,
            purpose,
            self.id,
            &self.token_fingerprint(purpose),
            clock,
        )
    }

    /// `User.find_by_token_for!(purpose, token)`.
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` for any invalid, expired, repurposed or
    /// stale token, and for a token whose user no longer exists.
    pub async fn find_by_token_for<C: ConnectionTrait>(
        db: &C,
        purpose: Purpose,
        token: &str,
        secret_key_base: &[u8],
        clock: &dyn Clock,
    ) -> ModelResult<Self> {
        let claims = tokens::verify(secret_key_base, purpose, token, clock).map_err(|err| {
            tracing::debug!(purpose = purpose.as_str(), %err, "rejected token");
            ModelError::EntityNotFound
        })?;
        let user = Self::find_by_id(db, claims.id).await?;
        claims
            .check_fingerprint(&user.token_fingerprint(purpose))
            .map_err(|err| {
                tracing::debug!(purpose = purpose.as_str(), %err, "rejected token");
                ModelError::EntityNotFound
            })?;
        Ok(user)
    }

    /// Delete every session of this user except `keep_session_id` (the
    /// `after_update if: :password_digest_previously_changed?` callback).
    async fn delete_other_sessions<C: ConnectionTrait>(
        &self,
        db: &C,
        keep_session_id: Option<i64>,
    ) -> ModelResult<u64> {
        let mut query =
            sessions::Entity::delete_many().filter(sessions::Column::UserId.eq(self.id));
        if let Some(id) = keep_session_id {
            query = query.filter(sessions::Column::Id.ne(id));
        }
        Ok(query.exec(db).await?.rows_affected)
    }

    /// Checks `password_challenge` against the stored digest. Rails treats a
    /// missing challenge as `""` in these controllers (`with_defaults`).
    fn check_challenge(&self, challenge: &str, errors: &mut Errors) {
        if !self.authenticate(challenge) {
            errors.add("password_challenge", "is invalid");
        }
    }

    /// The errors [`Self::update_profile`] would report, without writing (precognition).
    #[must_use]
    pub fn profile_errors(name: Option<&str>) -> Errors {
        let mut errors = Errors::new();
        if let Some(name) = name {
            validate_name(name, &mut errors);
        }
        errors
    }

    /// `Settings::ProfilesController#update` — `user.update(params.permit(:name))`. A
    /// missing `name` keeps the current one; the name is stored as given (Rails only
    /// normalizes email).
    ///
    /// # Errors
    /// `SaveError::Invalid` when the name is blank.
    pub async fn update_profile(
        self,
        db: &DatabaseConnection,
        name: Option<&str>,
    ) -> Result<Self, SaveError> {
        Self::profile_errors(name).into_result()?;
        let Some(name) = name else {
            return Ok(self);
        };
        let mut user = self.into_active_model();
        user.name = ActiveValue::Set(name.to_string());
        Ok(user.update(db).await?)
    }

    /// The errors [`Self::change_email`] would report, without writing (precognition).
    /// A missing `email` keeps the current one; the challenge is always checked.
    ///
    /// # Errors
    /// Database errors.
    pub async fn email_change_errors<C: ConnectionTrait>(
        &self,
        db: &C,
        email: Option<&str>,
        password_challenge: &str,
    ) -> ModelResult<Errors> {
        let mut errors = Errors::new();
        if let Some(email) = email.map(normalize_email) {
            validate_email_format(&email, &mut errors);
            if !email.is_empty() && email_taken(db, &email, Some(self.id)).await? {
                errors.add("email", "has already been taken");
            }
        }
        self.check_challenge(password_challenge, &mut errors);
        Ok(errors)
    }

    /// `Settings::EmailsController#update`. Returns the saved user and whether
    /// the email actually changed (`email_previously_changed?`). A changed
    /// email resets `verified` to false; the caller then re-sends verification.
    ///
    /// # Errors
    /// `SaveError::Invalid` for a wrong challenge, bad format or a taken email — also when
    /// the password changed after `self` was loaded (the challenge was checked against a
    /// digest that is no longer current).
    pub async fn change_email(
        self,
        db: &DatabaseConnection,
        email: Option<&str>,
        password_challenge: &str,
    ) -> Result<(Self, bool), SaveError> {
        let txn = crate::db::begin_write(db).await?;
        self.email_change_errors(&txn, email, password_challenge)
            .await?
            .into_result()?;
        // Every step below only applies while the digest the challenge was checked against
        // is still current: a password reset after `self` was loaded makes it stale.
        let Some(email) = email.map(normalize_email).filter(|e| *e != self.email) else {
            let current = Entity::find()
                .filter(Column::Id.eq(self.id))
                .filter(Column::PasswordDigest.eq(self.password_digest.as_str()))
                .count(&txn)
                .await?;
            txn.commit().await?;
            if current == 0 {
                return Err(stale_challenge("is invalid"));
            }
            return Ok((self, false));
        };
        let updated = Entity::update_many()
            .col_expr(Column::Email, Expr::value(email))
            .col_expr(Column::Verified, Expr::value(false))
            .col_expr(
                Column::UpdatedAt,
                Expr::value(DateTimeWithTimeZone::from(chrono::Utc::now())),
            )
            .filter(Column::Id.eq(self.id))
            .filter(Column::PasswordDigest.eq(self.password_digest.as_str()))
            .exec(&txn)
            .await?;
        if updated.rows_affected == 0 {
            txn.rollback().await?;
            return Err(stale_challenge("is invalid"));
        }
        let user = Self::find_by_id(&txn, self.id).await?;
        txn.commit().await?;
        Ok((user, true))
    }

    /// The errors [`Self::change_password`] would report, without writing (precognition).
    #[must_use]
    pub fn password_change_errors(
        &self,
        params: &PasswordParams,
        password_challenge: &str,
    ) -> Errors {
        let mut errors = Self::password_reset_errors(params);
        self.check_challenge(password_challenge, &mut errors);
        errors
    }

    /// `Settings::PasswordsController#update`: requires the current password,
    /// then deletes every other session of the user. Without a new password (missing or
    /// `""`) nothing is written, like Rails' `update` with an unchanged `has_secure_password`.
    ///
    /// # Errors
    /// `SaveError::Invalid` for a wrong challenge, a short password or a
    /// confirmation mismatch — also when the password changed concurrently after the
    /// challenge was checked (the challenge no longer matches the current password).
    pub async fn change_password(
        self,
        db: &DatabaseConnection,
        params: &PasswordParams,
        password_challenge: &str,
        current_session_id: Option<i64>,
    ) -> Result<Self, SaveError> {
        self.password_change_errors(params, password_challenge)
            .into_result()?;
        let Some(password) = params.new_password() else {
            return Ok(self);
        };
        match self.set_password(db, password, current_session_id).await {
            Err(SaveError::Stale) => {
                let mut errors = Errors::new();
                errors.add("password_challenge", "is invalid");
                Err(SaveError::Invalid(errors))
            }
            other => other,
        }
    }

    /// The errors [`Self::reset_password`] would report, without writing (precognition).
    #[must_use]
    pub fn password_reset_errors(params: &PasswordParams) -> Errors {
        let mut errors = Errors::new();
        if params.is_nil() {
            // `has_secure_password`: the digest is gone, "can't be blank".
            errors.add("password", "can't be blank");
            return errors;
        }
        validate_password(
            params.new_password(),
            params.password_confirmation.as_deref(),
            false,
            &mut errors,
        );
        errors
    }

    /// `Identity::PasswordResetsController#update`: no challenge (the signed
    /// token is the proof). The reset routes skip authentication in the Rails kit, so
    /// `Current.session` is nil and every session of the user is deleted, including the
    /// one of a browser that happens to be signed in.
    ///
    /// `self` must be the user as loaded when the token was checked: the write only
    /// applies while its password digest is still current, so of two concurrent resets
    /// with the same token exactly one wins.
    ///
    /// # Errors
    /// `SaveError::Invalid` for a short password or a confirmation mismatch;
    /// `SaveError::Stale` when the password changed after the token was checked.
    pub async fn reset_password(
        self,
        db: &DatabaseConnection,
        params: &PasswordParams,
    ) -> Result<Self, SaveError> {
        Self::password_reset_errors(params).into_result()?;
        let Some(password) = params.new_password() else {
            return Ok(self);
        };
        self.set_password(db, password, None).await
    }

    /// Store a new digest — only while `self.password_digest` (the one that was checked)
    /// is still current — and delete the other sessions, in one transaction.
    async fn set_password(
        self,
        db: &DatabaseConnection,
        password: &str,
        keep_session_id: Option<i64>,
    ) -> Result<Self, SaveError> {
        let digest = hash(password)?;
        let txn = crate::db::begin_write(db).await?;
        let updated = Entity::update_many()
            .col_expr(Column::PasswordDigest, Expr::value(digest))
            .col_expr(
                Column::UpdatedAt,
                Expr::value(DateTimeWithTimeZone::from(chrono::Utc::now())),
            )
            .filter(Column::Id.eq(self.id))
            .filter(Column::PasswordDigest.eq(self.password_digest.as_str()))
            .exec(&txn)
            .await?;
        if updated.rows_affected == 0 {
            txn.rollback().await?;
            return Err(SaveError::Stale);
        }
        self.delete_other_sessions(&txn, keep_session_id).await?;
        let user = Self::find_by_id(&txn, self.id).await?;
        txn.commit().await?;
        Ok(user)
    }

    /// `Identity::EmailVerificationsController#show` — `update!(verified: true)`, but only
    /// while the user's email is still the one the token was issued for (`self.email`, as
    /// loaded when the token was checked).
    ///
    /// # Errors
    /// `ModelError::EntityNotFound` when the email changed, the user is already verified
    /// (the link was used), or the user is gone since the token was checked; database errors.
    pub async fn verify_email(self, db: &DatabaseConnection) -> ModelResult<Self> {
        let updated = Entity::update_many()
            .col_expr(Column::Verified, Expr::value(true))
            .col_expr(
                Column::UpdatedAt,
                Expr::value(DateTimeWithTimeZone::from(chrono::Utc::now())),
            )
            .filter(Column::Id.eq(self.id))
            .filter(Column::Email.eq(self.email.as_str()))
            // Single use: once verified, the same link no longer matches. (Rails binds the token to
            // the email only, so a used link keeps working until it expires; docs/PARITY.md.)
            .filter(Column::Verified.eq(false))
            .exec(db)
            .await?;
        if updated.rows_affected == 0 {
            return Err(ModelError::EntityNotFound);
        }
        Self::find_by_id(db, self.id).await
    }

    /// `UsersController#destroy`: delete the account (and its sessions) if the
    /// password challenge matches.
    ///
    /// # Errors
    /// `SaveError::Invalid` with `password_challenge: ["Password challenge is
    /// invalid"]` (the Rails kit's wording for this action) — also when the password
    /// changed after `self` was loaded; nothing is deleted then.
    pub async fn destroy_with_challenge(
        self,
        db: &DatabaseConnection,
        password_challenge: &str,
    ) -> Result<(), SaveError> {
        if !self.authenticate(password_challenge) {
            let mut errors = Errors::new();
            errors.add("password_challenge", "Password challenge is invalid");
            return Err(SaveError::Invalid(errors));
        }
        let txn = crate::db::begin_write(db).await?;
        // Claim the row while the checked digest is still current (the write also takes the
        // lock), before any session is deleted; a reset since `self` was loaded fails here.
        let claimed = Entity::update_many()
            .col_expr(
                Column::UpdatedAt,
                Expr::value(DateTimeWithTimeZone::from(chrono::Utc::now())),
            )
            .filter(Column::Id.eq(self.id))
            .filter(Column::PasswordDigest.eq(self.password_digest.as_str()))
            .exec(&txn)
            .await?;
        if claimed.rows_affected == 0 {
            txn.rollback().await?;
            return Err(stale_challenge("Password challenge is invalid"));
        }
        self.delete_other_sessions(&txn, None).await?;
        Entity::delete_many()
            .filter(Column::Id.eq(self.id))
            .filter(Column::PasswordDigest.eq(self.password_digest.as_str()))
            .exec(&txn)
            .await?;
        txn.commit().await?;
        Ok(())
    }
}
