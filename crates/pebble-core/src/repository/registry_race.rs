//! Deterministic, test-only registry read replacement hook.

use std::path::Path;

#[cfg(test)]
type Hook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
std::thread_local! {
    static INJECTION: std::cell::RefCell<Option<Hook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn inject(hook: impl FnOnce(&Path) + 'static) {
    INJECTION.with(|injection| *injection.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
pub(super) fn run(path: &Path) {
    INJECTION.with(|injection| {
        if let Some(hook) = injection.borrow_mut().take() {
            hook(path);
        }
    });
}

#[cfg(not(test))]
pub(super) const fn run(_path: &Path) {}
