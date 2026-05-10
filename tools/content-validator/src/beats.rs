//! Beat Chart validator for the content-validator CLI.
//!
//! Loads every `*.ron` file under `content/gigs/` using [`cpr_gm::beats::load_gig`],
//! then runs [`cpr_gm::beats::validate_gig`] against each one.
//!
//! NPC and location cross-reference validation is **skipped** (empty sets passed)
//! because the NPC template catalog and location registry loaders are downstream
//! work packages (WP-606, WP-610). Structural Beat Chart checks are fully applied.
//!
//! Returns the total number of diagnostic errors encountered across all files.

use cpr_gm::beats::{load_gig, validate_gig, NpcTemplateId};
use cpr_gm::LocationId;
use std::collections::HashSet;
use std::path::Path;

/// Validate all `*.ron` Beat Charts under `<content_root>/gigs/`.
///
/// Prints one diagnostic line per error in the format:
/// ```text
/// [ERROR] <file>: <error message>
/// ```
///
/// Returns the total count of errors found. A return value of `0` means all
/// Beat Charts are structurally valid.
pub fn validate_beats(content_root: &Path) -> usize {
    let gigs_dir = content_root.join("gigs");

    if !gigs_dir.exists() {
        // No gigs directory — nothing to validate.
        println!("[beats] no content/gigs/ directory found, skipping");
        return 0;
    }

    let entries = match std::fs::read_dir(&gigs_dir) {
        Ok(e) => e,
        Err(err) => {
            println!(
                "[ERROR] cannot read gigs directory {}: {err}",
                gigs_dir.display()
            );
            return 1;
        }
    };

    // Empty sets: NPC and location cross-ref validation is deferred.
    let npc_ids: HashSet<NpcTemplateId> = HashSet::new();
    let location_ids: HashSet<LocationId> = HashSet::new();

    let mut total_errors: usize = 0;

    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(err) => {
                println!(
                    "[ERROR] directory entry error in {}: {err}",
                    gigs_dir.display()
                );
                total_errors += 1;
                continue;
            }
        };

        let path = entry.path();

        // Only process *.ron files.
        if path.extension().and_then(|e| e.to_str()) != Some("ron") {
            continue;
        }
        if !path.is_file() {
            continue;
        }

        // Load the gig.
        let gig = match load_gig(&path) {
            Ok(g) => g,
            Err(err) => {
                println!("[ERROR] {}: {err}", path.display());
                total_errors += 1;
                continue;
            }
        };

        // Run structural validation.
        if let Err(errors) = validate_gig(&gig, &npc_ids, &location_ids) {
            for err in &errors {
                println!("[ERROR] {}: {err}", path.display());
            }
            total_errors += errors.len();
        } else {
            println!("[OK]    {}", path.display());
        }
    }

    total_errors
}
