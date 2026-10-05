//! `spec/mailers/user_mailer_spec.rb`, plus the copy, the link and HTML escaping.

use inertia_rust_starter_kit::{
    mailers::user_mailer::UserMailer,
    models::{tokens::Purpose, users},
    route_table,
};
use sea_orm::{ActiveModelTrait, ActiveValue, IntoActiveModel};
use serial_test::serial;

use super::*;

#[tokio::test]
#[serial]
async fn email_verification_goes_to_the_user_with_the_right_subject_copy_and_link() {
    with_app(|_server, ctx| async move {
        let one = user(&ctx, ONE).await;
        UserMailer::email_verification(&ctx, &one).await.unwrap();
        let mails = deliveries(&ctx);
        assert_eq!(mails.len(), 1);
        let mail = decode_qp(&mails[0]);
        assert!(mail.contains("To: one@example.com"), "{mail}");
        assert!(mail.contains("From: from@example.com"), "{mail}");
        assert!(mail.contains("Subject: Verify your email"), "{mail}");
        assert!(mail.contains("Yes, use this email for my account"));
        assert!(mail.contains("This is to confirm that one@example.com is the email you want"));
        let link = format!(
            "{}{}?sid=",
            settings(&ctx).base_url(),
            route_table::IDENTITY_EMAIL_VERIFICATION
        );
        assert!(mail.contains(&link), "{mail}");
        let sid = sid_from_mail(&mails[0]);
        let found = users::Model::find_by_token_for(
            &ctx.db,
            Purpose::EmailVerification,
            &sid,
            settings(&ctx).secret_key_base.as_bytes(),
            &*inertia_rust_starter_kit::controllers::clock(&ctx),
        )
        .await
        .unwrap();
        assert_eq!(found.id, one.id);
    })
    .await;
}

#[tokio::test]
#[serial]
async fn password_reset_goes_to_the_user_with_the_right_subject_copy_and_link() {
    with_app(|_server, ctx| async move {
        let one = user(&ctx, ONE).await;
        UserMailer::password_reset(&ctx, &one).await.unwrap();
        let mails = deliveries(&ctx);
        assert_eq!(mails.len(), 1);
        let mail = decode_qp(&mails[0]);
        assert!(mail.contains("To: one@example.com"));
        assert!(mail.contains("Subject: Reset your password"));
        assert!(mail.contains("Reset my password"));
        assert!(mail.contains("it expires in 20 minutes"));
        assert!(mail.contains("Can't remember your password for <strong>one@example.com</strong>?"));
        let link = format!(
            "{}{}?sid=",
            settings(&ctx).base_url(),
            route_table::EDIT_IDENTITY_PASSWORD_RESET
        );
        assert!(mail.contains(&link), "{mail}");
    })
    .await;
}

#[tokio::test]
#[serial]
async fn user_data_is_html_escaped_in_the_html_part() {
    with_app(|_server, ctx| async move {
        // `&` and `'` are legal in an email address (URI::MailTo::EMAIL_REGEXP) and must be
        // escaped in HTML; `<`/`>` can never be stored.
        let mut one = user(&ctx, ONE).await.into_active_model();
        one.email = ActiveValue::Set("o'brien&co@example.com".into());
        let one = one.update(&ctx.db).await.unwrap();
        UserMailer::password_reset(&ctx, &one).await.unwrap();
        let mail = decode_qp(&deliveries(&ctx)[0]);
        let html = &mail[mail.find("text/html").expect("has an html part")..];
        assert!(
            html.contains("<strong>o&#39;brien&amp;co@example.com</strong>"),
            "{html}"
        );
        // The link's query string is escaped too (`&` would start a new parameter).
        assert!(!html.contains("<strong>o'brien&co"), "{html}");
    })
    .await;
}
