//! `cargo loco task routes:generate` — write `frontend/routes/*.ts` from `src/route_table.rs`.

use std::path::Path;

use loco_rs::prelude::*;

use crate::route_table;

pub struct RoutesGenerate;

#[async_trait]
impl Task for RoutesGenerate {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "routes:generate".to_string(),
            detail: "Generate frontend/routes/*.ts from src/route_table.rs".to_string(),
        }
    }

    async fn run(&self, _ctx: &AppContext, _vars: &task::Vars) -> Result<()> {
        let report = write(Path::new(env!("CARGO_MANIFEST_DIR"))).map_err(Error::wrap)?;
        println!("{report}");
        Ok(())
    }
}

/// Write the generated files under `root/frontend/routes`, delete stale generated `.ts`
/// files (everything except `runtime.ts`), and return a summary of what changed.
///
/// # Errors
/// On any filesystem error.
pub fn write(root: &Path) -> std::io::Result<String> {
    let dir = root.join(route_table::TS_DIR);
    let files = route_table::generate_ts();
    let mut written = 0;
    for (rel, contents) in &files {
        let target = dir.join(rel);
        if std::fs::read_to_string(&target).ok().as_deref() == Some(contents.as_str()) {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, contents)?;
        written += 1;
    }

    let mut removed = Vec::new();
    for existing in ts_files(&dir)? {
        let rel = existing
            .strip_prefix(&dir)
            .expect("ts_files returns paths under dir")
            .to_string_lossy()
            .replace('\\', "/");
        if rel != route_table::TS_RUNTIME && !files.iter().any(|(f, _)| *f == rel) {
            std::fs::remove_file(&existing)?;
            removed.push(rel);
        }
    }

    Ok(format!(
        "routes:generate: {} files, {written} written, {} stale removed{}",
        files.len(),
        removed.len(),
        if removed.is_empty() {
            String::new()
        } else {
            format!(" ({})", removed.join(", "))
        }
    ))
}

/// Every `.ts` file under `dir`, recursively.
///
/// # Errors
/// On any filesystem error.
pub fn ts_files(dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "ts") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}
