{#- The kit's task test for `cargo loco generate task`: Loco's, with the seeds loaded first (as
    model/test.t does), since a task usually works on existing rows. See
    .loco-templates/README.md. -#}
{% set file_name = name |  snake_case -%}
{% set module_name = file_name | pascal_case -%}
to: tests/tasks/{{ file_name }}.rs
skip_exists: true
message: "Tests for task `{{module_name}}` was added successfully. Run `cargo test`."
injections:
- into: tests/tasks/mod.rs
  append: true
  content: "pub mod {{ file_name }};"
---
use {{pkg_name}}::app::App;
use loco_rs::{task, testing::prelude::*};

use loco_rs::boot::run_task;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_can_run_{{name | snake_case}}() {
    let boot = boot_test::<App>().await.unwrap();
    // The seeds (src/fixtures/: Acme, Globex and their users), so a task that reads them works.
    seed::<App>(&boot.app_context).await.unwrap();

    // Pass the task's `key:value` args here, e.g. `task::Vars::from_cli_args(vec![("account".into(), "acme".into())])`.
    assert!(
        run_task::<App>(&boot.app_context, Some(&"{{name}}".to_string()), &task::Vars::default())
            .await
            .is_ok()
    );
}
