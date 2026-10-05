//! `cargo loco task seed:demo email:admin@example.com password:'…' [name:Admin] [account:acme]`:
//! create (or update) a verified user to sign in with on a demo deploy, and with `account:` make
//! them an owner of that account. Without it, a user in no account gets a personal one, as on
//! sign-up, so the demo login lands on an account overview. Production-safe: it goes through the User model (argon2 hash,
//! the sign-up validations) and is idempotent. The Docker image runs it at boot only when
//! `DEMO_ADMIN_EMAIL` and `DEMO_ADMIN_PASSWORD` are set.

use loco_rs::prelude::*;

use crate::models::{accounts, memberships, users};

pub struct SeedDemo;

#[async_trait]
impl Task for SeedDemo {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "seed:demo".to_string(),
            detail: "Create or update a verified demo user: email:<email> password:<password> \
                     [name:<name>] [account:<slug>]"
                .to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, vars: &task::Vars) -> Result<()> {
        let email = vars.cli_arg("email")?;
        let password = vars.cli_arg("password")?;
        let name = vars.cli_arg("name").unwrap_or("Admin");
        let user = users::Model::upsert_verified(&ctx.db, name, email, password).await?;
        println!("seed:demo: {} (id {}) is verified", user.email, user.id);
        if let Ok(slug) = vars.cli_arg("account") {
            let account = accounts::Model::find_by_slug(&ctx.db, slug).await?;
            memberships::Model::find_or_create(
                &ctx.db,
                account.id,
                user.id,
                memberships::Role::Owner,
            )
            .await?;
            println!("seed:demo: {} is a member of {slug}", user.email);
        } else if accounts::Model::list_for_user(&ctx.db, user.id)
            .await?
            .is_empty()
        {
            let account = accounts::Model::create_personal(&ctx.db, &user).await?;
            println!("seed:demo: {} owns {}", user.email, account.slug);
        }
        Ok(())
    }
}
