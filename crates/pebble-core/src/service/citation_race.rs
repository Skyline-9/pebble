//! Deterministic, test-only citation snapshot race hooks.

use std::path::Path;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum RacePoint {
    BeforeSnapshotOpen,
    AfterSnapshotOpen,
}

#[cfg(test)]
type Hook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
std::thread_local! {
    static INJECTION: std::cell::RefCell<Option<(RacePoint, Hook)>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn inject(point: RacePoint, hook: impl FnOnce(&Path) + 'static) {
    INJECTION.with(|injection| {
        *injection.borrow_mut() = Some((point, Box::new(hook)));
    });
}

#[cfg(test)]
pub(super) fn run(point: RacePoint, repository: &Path) {
    INJECTION.with(|injection| {
        let mut injection = injection.borrow_mut();
        if injection
            .as_ref()
            .is_some_and(|(target, _)| *target == point)
            && let Some((_, hook)) = injection.take()
        {
            drop(injection);
            hook(repository);
        }
    });
}

#[cfg(not(test))]
pub(super) const fn run(_point: RacePoint, _repository: &Path) {}
