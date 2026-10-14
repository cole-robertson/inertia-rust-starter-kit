use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Hooks},
    bgworker::{BackgroundWorker, Queue},
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::AppRoutes,
    db::{self, truncate_table},
    environment::Environment,
    task::Tasks,
    Result,
};
use migration::Migrator;
use std::path::Path;

use axum::Router as AxumRouter;

use crate::{
    controllers, inertia,
    models::_entities::{accounts, invitations, memberships, sessions, users},
    tasks,
};

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }
    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }
    /// Validates `settings:` (refusing to boot on a bad production config),
    /// loads the Vite manifest and stores `Arc<Settings>` in `shared_store`, then gives every
    /// SQLite connection the WAL/`synchronous=NORMAL`/busy-timeout PRAGMAs (src/db.rs).
    async fn after_context(ctx: AppContext) -> Result<AppContext> {
        crate::inertia::install(&ctx)?;
        crate::db::configure_sqlite_queue(&ctx).await?;
        crate::db::configure_sqlite_pool(ctx).await
    }
    async fn before_run(ctx: &AppContext) -> Result<()> {
        if ctx.environment == Environment::Production
            && !crate::mailers::user_mailer::smtp_configured(ctx)
        {
            tracing::warn!(
                "MAILER_HOST is not set: no mail will be sent (verification and password-reset \
                 emails are logged and dropped). Set MAILER_HOST, MAILER_USER and \
                 MAILER_PASSWORD to send mail."
            );
        }
        Ok(())
    }
    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn loco_rs::app::Initializer>>> {
        Ok(vec![Box::new(crate::inertia::ssr::SsrSupervisor)])
    }
    async fn on_shutdown(ctx: &AppContext) {
        crate::inertia::ssr::shutdown(ctx);
    }
    /// Loco's stack with a request logger that redacts `?sid=`/tokens.
    fn middlewares(
        ctx: &AppContext,
    ) -> Vec<Box<dyn loco_rs::controller::middleware::MiddlewareLayer>> {
        inertia::middlewares(ctx)
    }
    /// Unmatched GET/HEAD requests fall back to files in `public/`.
    async fn before_routes(_ctx: &AppContext) -> Result<AxumRouter<AppContext>> {
        Ok(inertia::base_router())
    }
    fn routes(ctx: &AppContext) -> AppRoutes {
        controllers::rate_limit::install(ctx);
        crate::auth::register_shared_props(ctx);
        // `cargo loco generate scaffold` adds its `.add_route(..)` line right after the
        // `empty()` line below, so keep the chain starting there.
        let routes = AppRoutes::empty()
            .add_route(controllers::account_invitations::routes())
            .add_route(crate::live::routes())
            .add_route(controllers::invitations::routes())
            .add_route(controllers::members::routes())
            .add_route(controllers::accounts::routes())
            .add_route(controllers::home::routes())
            .add_route(controllers::sessions::routes())
            .add_route(controllers::users::routes())
            .add_route(controllers::identity::email_verifications::routes())
            .add_route(controllers::identity::password_resets::routes())
            .add_route(controllers::dashboard::routes())
            .add_route(controllers::settings::profiles::routes())
            .add_route(controllers::settings::passwords::routes())
            .add_route(controllers::settings::emails::routes())
            .add_route(controllers::settings::sessions::routes())
            .add_route(controllers::settings::appearance::routes())
            .add_route(controllers::health::routes());
        #[cfg(feature = "bench")]
        let routes = {
            controllers::bench::install(ctx);
            routes.add_route(controllers::bench::routes())
        };
        routes
    }
    /// Layers, outermost first: timing → exceptions → headers → csrf → flash → inertia (omega:
    /// renders, the asset version 409, the redirect rules) → auth → live (the request's
    /// `X-Tab-Id`), then (routed requests only) the account-slug constraint and the browser check.
    /// (axum applies the last `.layer` outermost, hence the reverse order below.)
    async fn after_routes(router: AxumRouter, ctx: &AppContext) -> Result<AxumRouter> {
        let settings = controllers::settings(ctx)?;
        let router = controllers::browser::layer(router);
        let router = crate::auth::slug_constraint(router);
        let router = crate::live::layer(router);
        let router = crate::auth::layer(router, ctx, settings.clone());
        let router = inertia::render::layer(router, settings.clone());
        let router = inertia::flash::layer(router, settings.clone());
        let router = inertia::csrf::layer(router, settings.clone());
        let router = inertia::headers::layer(router, settings);
        let router = inertia::exceptions::layer(router);
        Ok(inertia::timing::layer(router))
    }
    /// Loco's `serve`, with the router behind [`inertia::service`] (trailing slashes).
    async fn serve(
        app: AxumRouter,
        ctx: &AppContext,
        serve_params: &loco_rs::boot::ServeParams,
    ) -> Result<()> {
        let listener = tokio::net::TcpListener::bind(&format!(
            "{}:{}",
            serve_params.binding, serve_params.port
        ))
        .await?;
        let service =
            axum::ServiceExt::<axum::extract::Request>::into_make_service_with_connect_info::<
                std::net::SocketAddr,
            >(inertia::service(app));
        let shutdown_ctx = ctx.clone();
        axum::serve(listener, service)
            .with_graceful_shutdown(async move {
                loco_rs::boot::shutdown_signal().await;
                tracing::info!("shutting down...");
                Self::on_shutdown(&shutdown_ctx).await;
            })
            .await?;
        Ok(())
    }
    async fn connect_workers(ctx: &AppContext, queue: &Queue) -> Result<()> {
        queue
            .register(crate::workers::invitation_delivery::Worker::build(ctx))
            .await?;
        queue
            .register(crate::workers::user_mailer_delivery::Worker::build(ctx))
            .await?;
        Ok(())
    }
    fn register_tasks(tasks: &mut Tasks) {
        tasks.register(tasks::routes_generate::RoutesGenerate);
        tasks.register(tasks::scaffold_pages::ScaffoldPages);
        tasks.register(tasks::seed_demo::SeedDemo);
        tasks.register(tasks::types_generate::TypesGenerate);
        // tasks-inject (do not remove this comment: `cargo loco generate task` adds above it)
    }
    async fn truncate(ctx: &AppContext) -> Result<()> {
        // Children first: sessions, memberships and invitations reference users and accounts.
        truncate_table(&ctx.db, invitations::Entity).await?;
        truncate_table(&ctx.db, memberships::Entity).await?;
        truncate_table(&ctx.db, sessions::Entity).await?;
        truncate_table(&ctx.db, accounts::Entity).await?;
        truncate_table(&ctx.db, users::Entity).await?;
        Ok(())
    }
    async fn seed(ctx: &AppContext, base: &Path) -> Result<()> {
        db::seed::<users::ActiveModel>(&ctx.db, &base.join("users.yaml").display().to_string())
            .await?;
        db::seed::<sessions::ActiveModel>(
            &ctx.db,
            &base.join("sessions.yaml").display().to_string(),
        )
        .await?;
        db::seed::<accounts::ActiveModel>(
            &ctx.db,
            &base.join("accounts.yaml").display().to_string(),
        )
        .await?;
        db::seed::<memberships::ActiveModel>(
            &ctx.db,
            &base.join("memberships.yaml").display().to_string(),
        )
        .await?;
        crate::models::invitations::Model::seed_pending(
            &ctx.db,
            1,
            1,
            "three@example.com",
            crate::models::memberships::Role::Admin,
            crate::models::invitations::SEED_TOKEN,
        )
        .await?;
        Ok(())
    }
}
