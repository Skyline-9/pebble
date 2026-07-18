use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn manifest_paths(roots: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut manifests = Vec::new();
    for root in roots {
        collect_manifests(root, &mut manifests)?;
    }
    manifests.sort();
    manifests.dedup();
    Ok(manifests)
}

fn collect_manifests(path: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    if ignored(path) {
        return Ok(());
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("read metadata for {}: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "symlink is forbidden in Cargo manifest search roots: {}",
            path.display()
        ));
    }
    if metadata.is_file() {
        if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            output.push(path.to_path_buf());
        }
        return Ok(());
    }
    let entries = fs::read_dir(path)
        .map_err(|error| format!("read directory {}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("read directory entry in {}: {error}", path.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("read file type for {}: {error}", entry.path().display()))?;
        if file_type.is_symlink() {
            return Err(format!(
                "symlink is forbidden in Cargo manifest search roots: {}",
                entry.path().display()
            ));
        }
        collect_manifests(&entry.path(), output)?;
    }
    Ok(())
}

fn ignored(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(".git" | "node_modules" | "target" | ".superpowers")
        )
    })
}
