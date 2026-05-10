//! `content-validator`: load every RON file under `content/` and report
//! schema errors.
//!
//! ## Usage
//!
//! ```sh
//! cargo run -p content-validator -- content/
//! ```
//!
//! The first positional argument is the path to the content root. If omitted,
//! it defaults to `content/` relative to the current working directory.
//!
//! Exits with code 0 if all content is valid, nonzero if any errors are found.

mod beats;

fn main() {
    let content_root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "content".to_string());

    let content_path = std::path::Path::new(&content_root);

    if !content_path.exists() {
        eprintln!(
            "content-validator: content root '{}' does not exist",
            content_path.display()
        );
        std::process::exit(1);
    }

    let mut total_errors: usize = 0;

    // ── Beat Charts ───────────────────────────────────────────────────────────
    println!("--- Beat Charts (content/gigs/) ---");
    total_errors += beats::validate_beats(content_path);

    // ── Summary ───────────────────────────────────────────────────────────────
    if total_errors == 0 {
        println!("\ncontent-validator: all checks passed");
    } else {
        eprintln!("\ncontent-validator: {total_errors} error(s) found");
        std::process::exit(1);
    }
}
