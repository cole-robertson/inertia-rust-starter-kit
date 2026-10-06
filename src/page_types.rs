//! The TypeScript types of the page props, generated from the Rust structs that build them.
//!
//! A props struct derives `Serialize` (what the controller sends) and `ts_rs::TS` (its
//! TypeScript type), and is listed in [`generate_ts`]. `cargo loco task types:generate` writes
//! one file per type to [`TS_DIR`], and React pages import them
//! (`import type { AccountProps } from "@/types/generated/AccountProps"`), so a renamed or
//! retyped field fails `tsc` instead of rendering `undefined`. `tests/types_fresh.rs` fails when
//! the committed files differ from what the structs generate, as `tests/routes_fresh.rs` does
//! for `src/route_table.rs`.
//!
//! Types a listed struct uses (`Role` in `MemberProps`) are generated with it; list only the
//! roots. `cargo loco generate scaffold` adds its resource's `<Singular>Props` above
//! `// scaffold:types`.

use std::{collections::BTreeMap, path::PathBuf};

use ts_rs::{Config, TypeVisitor, TS};

/// Where the generated files go, relative to the app root.
pub const TS_DIR: &str = "frontend/types/generated";

/// Every generated file as `(file name, contents)`, sorted by name.
#[must_use]
pub fn generate_ts() -> Vec<(String, String)> {
    let mut types = Collector::default();
    types.add::<crate::auth::Auth>();
    types.add::<crate::models::accounts::AccountProps>();
    types.add::<crate::models::accounts::AccountSummary>();
    types.add::<crate::models::memberships::MemberProps>();
    types.add::<crate::models::invitations::PendingInvitationProps>();
    types.add::<crate::models::sessions::SessionProps>();
    types.add::<crate::models::SelectOption>();
    // scaffold:types (`cargo loco generate scaffold` adds its props struct above this line)
    types.files.into_iter().collect()
}

/// i64 ids are `number` in the pages (they are well under 2^53), not ts-rs's default `bigint`.
fn config() -> Config {
    Config::new().with_large_int("number")
}

/// Each type's file, plus the files of every exported type it uses.
#[derive(Default)]
struct Collector {
    files: BTreeMap<String, String>,
}

impl Collector {
    fn add<T: TS + 'static>(&mut self) {
        self.visit::<T>();
    }
}

impl TypeVisitor for Collector {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        let Some(name) = T::output_path().map(|p: PathBuf| p.to_string_lossy().replace('\\', "/"))
        else {
            return; // a built-in (`string`, `Array<..>`): nothing to write
        };
        if self.files.contains_key(&name) {
            return;
        }
        let ts = T::export_to_string(&config())
            .unwrap_or_else(|e| panic!("ts-rs could not export {name}: {e}"));
        self.files.insert(name, ts);
        T::visit_dependencies(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> String {
        generate_ts()
            .into_iter()
            .find(|(f, _)| f == name)
            .unwrap_or_else(|| panic!("{name} is not generated"))
            .1
    }

    #[test]
    fn ids_are_numbers_and_options_are_nullable() {
        let auth = file("AuthUser.ts");
        assert!(auth.contains("id: number"), "{auth}");
        let invitation = file("PendingInvitationProps.ts");
        assert!(
            invitation.contains("inviter_name: string | null"),
            "{invitation}"
        );
    }

    #[test]
    fn types_used_by_a_listed_struct_are_generated_with_it() {
        let member = file("MemberProps.ts");
        assert!(
            member.contains(r#"import type { Role } from "./Role";"#),
            "{member}"
        );
        assert!(file("Role.ts").contains(r#""owner" | "admin" | "member""#));
    }
}
