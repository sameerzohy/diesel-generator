//! Post-generation verification (Commit 8): format the generated Rust with
//! `rustfmt`, then prove it compiles with `cargo check`. A failed check is a
//! hard error carrying the compiler's output — invalid code never lands silently.

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, bail, Result};

/// Run `rustfmt` over each generated `.rs` file in place. Formatting is cosmetic,
/// so a missing or unhappy rustfmt warns rather than failing the whole run.
pub fn rustfmt_files(paths: &[&Path]) -> Result<()> {
    for path in paths {
        match Command::new("rustfmt")
            .arg("--edition")
            .arg("2021")
            .arg(path)
            .output()
        {
            Ok(out) if out.status.success() => {}
            Ok(out) => eprintln!(
                "warning: rustfmt failed on {}: {}",
                path.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            // Not installed — warn once and stop trying.
            Err(e) if e.kind() == ErrorKind::NotFound => {
                eprintln!("warning: rustfmt not found; skipping formatting");
                return Ok(());
            }
            Err(e) => return Err(anyhow!("failed to run rustfmt: {e}")),
        }
    }
    Ok(())
}

/// Run `cargo check` in the generated crate. Non-zero exit ⇒ hard error with the
/// captured compiler output. A missing `cargo` is also an error (use `--no-verify`).
pub fn cargo_check(out_dir: &Path) -> Result<()> {
    let output = match Command::new("cargo")
        .arg("check")
        .current_dir(out_dir)
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            bail!("cargo not found; cannot verify the generated crate (pass --no-verify to skip)")
        }
        Err(e) => return Err(anyhow!("failed to run cargo check: {e}")),
    };

    if output.status.success() {
        Ok(())
    } else {
        bail!(
            "generated crate failed `cargo check`:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
    }
}
