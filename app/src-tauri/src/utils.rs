// Cross-platform small utilities

#[cfg(target_os = "windows")]
pub fn ps_escape_single_quoted(s: &str) -> String {
  // In PowerShell single-quoted strings, escape ' by doubling it
  s.replace('\'', "''")
}

#[cfg(not(target_os = "windows"))]
pub fn ps_escape_single_quoted(s: &str) -> String { s.to_string() }

use std::path::PathBuf;
use std::fs;

// Utility: Copy a file to destination (used by Save As flow)
pub fn copy_file_to_path(src: String, dest: String, overwrite: Option<bool>) -> Result<String, String> {
  let overwrite = overwrite.unwrap_or(true);
  let dest_path = PathBuf::from(&dest);
  if let Some(dir) = dest_path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create destination dir: {e}"))?;
  }
  if dest_path.exists() && !overwrite {
    return Err("Destination file already exists".into());
  }
  fs::copy(&src, &dest_path).map_err(|e| format!("Copy failed: {e}"))?;
  Ok(dest_path.to_string_lossy().to_string())
}
