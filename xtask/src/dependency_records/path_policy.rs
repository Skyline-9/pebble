use std::fs;
use std::path::Path;

pub(super) fn check(
    name: &str,
    path: &str,
    member_dir: &Path,
    repository_root: &Path,
    violations: &mut Vec<String>,
) {
    let resolved = member_dir.join(path);
    let Ok(canonical_path) = fs::canonicalize(&resolved) else {
        violations.push(format!(
            "missing path dependency {name}: {}",
            resolved.display()
        ));
        return;
    };
    let Ok(canonical_root) = fs::canonicalize(repository_root) else {
        violations.push(format!(
            "cannot canonicalize repository root: {}",
            repository_root.display()
        ));
        return;
    };
    if !canonical_path.starts_with(canonical_root) {
        violations.push(format!("external path dependency: {name}"));
    }
}
