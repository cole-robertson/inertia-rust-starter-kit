//! Background workers (`cargo loco generate worker <name>` adds them here and registers them in
//! `App::connect_workers`). The kit's own job is mail delivery, `user_mailer_delivery`.

pub mod invitation_delivery;
pub mod user_mailer_delivery;
