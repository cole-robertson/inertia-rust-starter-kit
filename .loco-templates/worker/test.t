{#- The kit's worker test for `cargo loco generate worker`: Loco's builds `WorkerArgs {}`, which
    stops compiling at the first field. This one seeds (as model/test.t does) and fills the
    arguments with `WorkerArgs::default()` (derived by worker.t). See .loco-templates/README.md. -#}
{% set module_name = name |  snake_case -%}
{% set struct_name = module_name | pascal_case -%}
to: "tests/workers/{{module_name}}.rs"
skip_exists: true
message: "Test for worker `{{struct_name}}` was added successfully. Run `cargo test`."
injections:
- into: tests/workers/mod.rs
  append: true
  content: "pub mod {{ name |  snake_case }};"
---
use loco_rs::{bgworker::BackgroundWorker, testing::prelude::*};
use {{pkg_name}}::{
    app::App,
    workers::{{module_name}}::{Worker, WorkerArgs},
};
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_run_{{module_name}}_worker() {
    let boot = boot_test::<App>().await.unwrap();
    seed::<App>(&boot.app_context).await.unwrap();

    // From `Default`, so this compiles as `WorkerArgs` grows. Set what the job needs, e.g.
    // `WorkerArgs { account_id: 1, ..Default::default() }` (the seeds' Acme).
    let args = WorkerArgs::default();
    // Runs the job now: config/test.yaml has `workers.mode: ForegroundBlocking`.
    assert!(Worker::perform_later(&boot.app_context, args).await.is_ok());
    // Include additional assert validations after the execution of the worker
}
