#![forbid(unsafe_code)]

//! Repository configuration, identity, and registry tests.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use pebble_core::domain::RepositoryId;
use pebble_core::repository::{RepositoryConfig, RepositoryRegistry, SystemGit};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> std::io::Result<Self> {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pebble-repository-{label}-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        let path = path.canonicalize()?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(repository: &Path, arguments: &[&str]) -> std::io::Result<()> {
    let status = Command::new("git")
        .args(["--no-optional-locks", "-C"])
        .arg(repository)
        .args(arguments)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("test Git command failed"))
    }
}

fn init_remote(repository: &Path, remote: &str) -> std::io::Result<()> {
    git(repository, &["init", "-q"])?;
    git(repository, &["remote", "add", "origin", remote])
}

#[test]
fn https_and_ssh_remotes_normalize_to_the_same_identity() -> Result<(), Box<dyn std::error::Error>>
{
    let https = TempDir::new("https")?;
    let ssh = TempDir::new("ssh")?;
    init_remote(https.path(), "https://GitHub.COM/Acme/Pebble.git/")?;
    init_remote(ssh.path(), "git@github.com:Acme/Pebble.git")?;
    let system_git = SystemGit::discover()?;

    let https_config = RepositoryConfig::initialize(https.path(), &system_git)?;
    let ssh_config = RepositoryConfig::initialize(ssh.path(), &system_git)?;

    assert_eq!(https_config.repository_id(), ssh_config.repository_id());
    assert_eq!(
        https_config.repository_id().as_str(),
        "git.10.github.com.4.Acme.6.Pebble"
    );
    Ok(())
}

#[test]
fn repository_without_remote_gets_a_valid_ulid_identity() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = TempDir::new("ulid")?;
    git(repository.path(), &["init", "-q"])?;

    let config = RepositoryConfig::initialize(repository.path(), &SystemGit::discover()?)?;
    let id = config.repository_id().as_str();

    assert_eq!(id.len(), 26);
    assert!(id.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    assert_eq!(config.schema(), 1);
    Ok(())
}

#[test]
fn persisted_identity_remains_canonical_after_remote_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let repository = TempDir::new("canonical")?;
    init_remote(repository.path(), "https://github.com/acme/original.git")?;
    let system_git = SystemGit::discover()?;
    let initialized = RepositoryConfig::initialize(repository.path(), &system_git)?;

    git(
        repository.path(),
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/replacement.git",
        ],
    )?;
    let loaded = RepositoryConfig::initialize(repository.path(), &system_git)?;

    assert_eq!(loaded.repository_id(), initialized.repository_id());
    assert_eq!(loaded, RepositoryConfig::load(repository.path())?);
    Ok(())
}

#[test]
fn config_round_trips_and_rejects_unsafe_content() -> Result<(), Box<dyn std::error::Error>> {
    let repository = TempDir::new("config")?;
    fs::create_dir(repository.path().join(".pebble"))?;
    fs::write(
        repository.path().join(".pebble/pebble.toml"),
        concat!(
            "schema = 1\n",
            "repository_id = \"acme.repo\"\n",
            "include = [\"src/**\", \".pebble/knowledge/**\"]\n",
            "exclude = [\"target/**\"]\n",
            "[language_overrides]\n",
            "\"src/generated/**\" = \"rust\"\n",
        ),
    )?;

    let loaded = RepositoryConfig::load(repository.path())?;
    assert_eq!(loaded.schema(), 1);
    assert_eq!(loaded.include(), &["src/**", ".pebble/knowledge/**"]);
    assert_eq!(loaded.exclude(), &["target/**"]);
    assert_eq!(
        loaded.language_overrides(),
        &BTreeMap::from([("src/generated/**".to_owned(), "rust".to_owned())])
    );

    for (label, contents) in [
        (
            "secret",
            "schema=1\nrepository_id=\"repo\"\ntoken=\"secret\"\n",
        ),
        (
            "absolute",
            "schema=1\nrepository_id=\"repo\"\ninclude=[\"/etc/**\"]\n",
        ),
        (
            "windows absolute",
            "schema=1\nrepository_id=\"repo\"\ninclude=[\"C:/secret/**\"]\n",
        ),
        (
            "traversal",
            "schema=1\nrepository_id=\"repo\"\nexclude=[\"src/../secret\"]\n",
        ),
        ("duplicate", "schema=1\nschema=1\nrepository_id=\"repo\"\n"),
    ] {
        fs::write(repository.path().join(".pebble/pebble.toml"), contents)?;
        assert!(
            RepositoryConfig::load(repository.path()).is_err(),
            "{label} config must fail"
        );
    }

    fs::write(
        repository.path().join(".pebble/pebble.toml"),
        vec![b'x'; 1_048_577],
    )?;
    assert!(RepositoryConfig::load(repository.path()).is_err());
    Ok(())
}

#[test]
fn registry_rejects_duplicate_checkouts_but_allows_alternate_worktrees()
-> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("registry")?;
    let first = TempDir::new("checkout-a")?;
    let second = TempDir::new("checkout-b")?;
    let repository_id = RepositoryId::try_from("acme.repo".to_owned())?;
    let registry = RepositoryRegistry::new(state.path());

    registry.register(&repository_id, first.path(), false)?;
    assert!(
        registry
            .register(&repository_id, second.path(), false)
            .is_err()
    );
    registry.register(&repository_id, second.path(), true)?;

    let registrations = registry.registrations()?;
    assert_eq!(registrations.len(), 2);
    assert_eq!(registrations[0].repository_id(), &repository_id);
    assert_eq!(registrations[0].checkout(), first.path());
    assert!(!registrations[0].alternate_worktree());
    assert!(registrations[1].alternate_worktree());
    assert!(state.path().join("registry.json").is_file());
    assert!(!state.path().join("registry.json.tmp").exists());
    Ok(())
}

#[test]
fn remote_identity_framing_prevents_separator_collisions() -> Result<(), Box<dyn std::error::Error>>
{
    let first = TempDir::new("remote-components-a")?;
    let second = TempDir::new("remote-components-b")?;
    init_remote(first.path(), "https://example.com/a.b/c.git")?;
    init_remote(second.path(), "git@example.com:a/b.c.git")?;
    let system_git = SystemGit::discover()?;

    let first_id = RepositoryConfig::initialize(first.path(), &system_git)?;
    let second_id = RepositoryConfig::initialize(second.path(), &system_git)?;

    assert_ne!(first_id.repository_id(), second_id.repository_id());
    assert_eq!(
        first_id.repository_id().as_str(),
        "git.11.example.com.3.a.b.1.c"
    );
    assert_eq!(
        second_id.repository_id().as_str(),
        "git.11.example.com.1.a.3.b.c"
    );
    Ok(())
}

#[test]
fn concurrent_registry_processes_preserve_every_registration()
-> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("registry-concurrent")?;
    let barrier = state.path().join("start");
    let mut checkouts = Vec::new();
    let mut workers = Vec::new();
    for index in 0..16 {
        let checkout = TempDir::new(&format!("concurrent-{index}"))?;
        workers.push(registry_worker(
            state.path(),
            checkout.path(),
            &format!("repo.{index}"),
            &barrier,
        )?);
        checkouts.push(checkout);
    }
    fs::write(&barrier, b"go")?;
    for mut worker in workers {
        assert!(worker.wait()?.success());
    }

    let registrations = RepositoryRegistry::new(state.path()).registrations()?;

    assert_eq!(registrations.len(), checkouts.len());
    Ok(())
}

#[test]
fn crashed_registry_lock_owner_does_not_leave_a_stale_lock()
-> Result<(), Box<dyn std::error::Error>> {
    let state = TempDir::new("registry-crash")?;
    let checkout = TempDir::new("registry-crash-checkout")?;
    let ready = state.path().join("ready");
    let barrier = state.path().join("start");
    fs::write(&barrier, b"go")?;
    let mut holder = Command::new(std::env::current_exe()?)
        .args(["--exact", "registry_lock_holder", "--nocapture"])
        .env("PEBBLE_REGISTRY_STATE", state.path())
        .env("PEBBLE_REGISTRY_READY", &ready)
        .stdout(Stdio::null())
        .spawn()?;
    wait_for(&ready)?;
    let mut worker = registry_worker(state.path(), checkout.path(), "repo.crash", &barrier)?;
    thread::sleep(Duration::from_millis(100));
    assert!(
        worker.try_wait()?.is_none(),
        "registry lock was not honored"
    );

    holder.kill()?;
    assert!(!holder.wait()?.success());
    assert!(worker.wait()?.success());
    assert_eq!(
        RepositoryRegistry::new(state.path()).registrations()?.len(),
        1
    );
    Ok(())
}

fn registry_worker(
    state: &Path,
    checkout: &Path,
    repository_id: &str,
    barrier: &Path,
) -> std::io::Result<Child> {
    Command::new(std::env::current_exe()?)
        .args(["--exact", "registry_process_worker", "--nocapture"])
        .env("PEBBLE_REGISTRY_STATE", state)
        .env("PEBBLE_REGISTRY_CHECKOUT", checkout)
        .env("PEBBLE_REGISTRY_ID", repository_id)
        .env("PEBBLE_REGISTRY_BARRIER", barrier)
        .stdout(Stdio::null())
        .spawn()
}

fn wait_for(path: &Path) -> std::io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(std::io::Error::other("worker did not become ready"));
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[test]
fn registry_process_worker() -> Result<(), Box<dyn std::error::Error>> {
    let Some(state) = std::env::var_os("PEBBLE_REGISTRY_STATE") else {
        return Ok(());
    };
    let checkout = PathBuf::from(
        std::env::var_os("PEBBLE_REGISTRY_CHECKOUT").ok_or("missing worker checkout")?,
    );
    let repository_id = RepositoryId::try_from(std::env::var("PEBBLE_REGISTRY_ID")?)?;
    let barrier =
        PathBuf::from(std::env::var_os("PEBBLE_REGISTRY_BARRIER").ok_or("missing barrier")?);
    wait_for(&barrier)?;
    RepositoryRegistry::new(Path::new(&state)).register(&repository_id, &checkout, false)?;
    Ok(())
}

#[test]
fn registry_lock_holder() -> Result<(), Box<dyn std::error::Error>> {
    let Some(state) = std::env::var_os("PEBBLE_REGISTRY_STATE") else {
        return Ok(());
    };
    let ready = PathBuf::from(std::env::var_os("PEBBLE_REGISTRY_READY").ok_or("missing ready")?);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(Path::new(&state).join(".registry.lock"))?;
    lock.lock()?;
    fs::write(ready, b"ready")?;
    thread::sleep(Duration::from_secs(30));
    Ok(())
}
