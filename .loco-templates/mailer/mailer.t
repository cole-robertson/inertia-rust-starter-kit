{#- Replaces Loco's mailer template only to send from `settings.mail_from` (MAIL_FROM in
    production), like `src/mailers/user_mailer.rs`; Loco's own sends from its built-in
    `System <system@example.com>`. Read .loco-templates/README.md first. -#}
{% set module_name = name | snake_case -%}
{% set struct_name = module_name | pascal_case -%}
to: "src/mailers/{{module_name}}.rs"
skip_exists: true
message: "A mailer `{{struct_name}}` was added successfully."
injections:
- into: "src/mailers/mod.rs"
  append: true
  content: "pub mod {{ module_name }};"
---
#![allow(non_upper_case_globals)]

use loco_rs::prelude::*;
use serde_json::json;

use crate::controllers::settings;

static shared: Dir<'_> = include_dir!("src/mailers/shared");
static welcome: Dir<'_> = include_dir!("src/mailers/{{module_name}}/welcome");

#[allow(clippy::module_name_repetitions)]
pub struct {{struct_name}} {}
impl Mailer for {{struct_name}} {}
impl {{struct_name}} {
    /// Send an email from `settings.mail_from`. To keep requests off SMTP, call this from a
    /// worker, as `src/mailers/user_mailer.rs` does.
    ///
    /// # Errors
    /// When email sending is failed
    pub async fn send_welcome(ctx: &AppContext, to: &str, msg: &str) -> Result<()> {
        Self::mail_template_with_shared(
            ctx,
            &welcome,
            &[&shared],
            mailer::Args {
                from: Some(settings(ctx)?.mail_from.clone()),
                to: to.to_string(),
                locals: json!({
                  "message": msg,
                  "domain": ctx.config.server.full_url()
                }),
                ..Default::default()
            },
        )
        .await?;

        Ok(())
    }
}
