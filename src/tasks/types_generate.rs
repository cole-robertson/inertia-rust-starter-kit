//! `cargo loco task types:generate`: write `frontend/types/generated/*.ts` from the props
//! structs listed in `src/page_types.rs`.

use std::path::Path;

use loco_rs::prelude::*;

use crate::{page_types, tasks::routes_generate};

pub struct TypesGenerate;

#[async_trait]
impl Task for TypesGenerate {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "types:generate".to_string(),
            detail: "Generate frontend/types/generated/*.ts from the props structs in \
                     src/page_types.rs"
                .to_string(),
        }
    }

    async fn run(&self, _ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
        let report = write(Path::new(env!("CARGO_MANIFEST_DIR"))).map_err(Error::wrap)?;
        println!("{report}");
        Ok(())
    }
}

/// Write the generated files under `root/frontend/types/generated`, delete `.ts` files there
/// that no listed type generates, and return a summary of what changed.
///
/// # Errors
/// On any filesystem error.
pub fn write(root: &Path) -> std::io::Result<String> {
    let dir = root.join(page_types::TS_DIR);
    std::fs::create_dir_all(&dir)?;
    let files = page_types::generate_ts();
    let mut written = 0;
    for (name, contents) in &files {
        let target = dir.join(name);
        if std::fs::read_to_string(&target).ok().as_deref() == Some(contents.as_str()) {
            continue;
        }
        std::fs::write(&target, contents)?;
        written += 1;
    }

    let mut removed = Vec::new();
    for existing in routes_generate::ts_files(&dir)? {
        let name = existing
            .strip_prefix(&dir)
            .expect("ts_files returns paths under dir")
            .to_string_lossy()
            .replace('\\', "/");
        if !files.iter().any(|(f, _)| *f == name) {
            std::fs::remove_file(&existing)?;
            removed.push(name);
        }
    }

    Ok(format!(
        "types:generate: {} files, {written} written, {} stale removed{}",
        files.len(),
        removed.len(),
        if removed.is_empty() {
            String::new()
        } else {
            format!(" ({})", removed.join(", "))
        }
    ))
}
