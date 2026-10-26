use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

pub const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathEntry {
    pub name: String,
    pub directory: String,
}

pub fn cache_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SCROLL_CACHE_FILE") {
        return PathBuf::from(path);
    }
    let root = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    root.join("pixie/summon.list")
}

pub fn scan_path() -> Vec<PathEntry> {
    scan_path_value(&std::env::var_os("PATH").unwrap_or_default())
}

fn scan_path_value(path_value: &std::ffi::OsStr) -> Vec<PathEntry> {
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for directory in std::env::split_paths(path_value) {
        if directory.as_os_str().is_empty() || directory.to_string_lossy().contains(['\n', '\t']) {
            continue;
        }
        let Ok(items) = fs::read_dir(&directory) else {
            continue;
        };
        for item in items.flatten() {
            let name = item.file_name().to_string_lossy().into_owned();
            if name.starts_with('.')
                || name.contains('\n')
                || name.contains('\t')
                || seen.contains(&name)
            {
                continue;
            }
            let path = item.path();
            let Ok(metadata) = fs::metadata(&path) else {
                continue;
            };
            if !metadata.is_file() || !is_executable(&metadata) {
                continue;
            }
            seen.insert(name.clone());
            entries.push(PathEntry {
                name,
                directory: directory.to_string_lossy().into_owned(),
            });
        }
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    entries
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

pub fn read_cache(path: &Path) -> io::Result<Vec<PathEntry>> {
    let content = fs::read_to_string(path)?;
    let mut entries = Vec::new();
    for line in content.lines() {
        let Some((name, directory)) = line.split_once('\t') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PATH cache contains a malformed row",
            ));
        };
        if name.is_empty()
            || directory.is_empty()
            || name.contains(['\n', '\t', '/'])
            || directory.contains(['\n', '\t'])
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "PATH cache contains an invalid command entry",
            ));
        }
        entries.push(PathEntry {
            name: name.to_owned(),
            directory: directory.to_owned(),
        });
    }
    Ok(entries)
}

pub fn write_cache(path: &Path, entries: &[PathEntry]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut text = String::new();
    for entry in entries {
        if entry.name.is_empty()
            || entry.name.contains(['\n', '\t', '/'])
            || entry.directory.is_empty()
            || entry.directory.contains(['\n', '\t'])
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "PATH cache entry cannot be represented safely",
            ));
        }
        text.push_str(&entry.name);
        text.push('\t');
        text.push_str(&entry.directory);
        text.push('\n');
    }

    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "PATH cache path must name a file",
        )
    })?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));
    let mut temporary = None;
    for _ in 0..100 {
        let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = std::ffi::OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".{}.{}.tmp", std::process::id(), sequence));
        let candidate = parent.join(temporary_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (temporary_path, mut file) = temporary.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a temporary PATH cache file",
        )
    })?;

    let result = (|| {
        use std::io::Write;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary_path, path)?;
        if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
            directory.sync_all()?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary_path);
    }
    result
}

pub struct CacheLoad {
    pub entries: Vec<PathEntry>,
    pub refresh_error: Option<io::Error>,
}

pub fn entries_with_cache(
    path_value: &std::ffi::OsStr,
    cache: &Path,
    now: SystemTime,
) -> CacheLoad {
    let stale = fs::metadata(cache)
        .and_then(|metadata| metadata.modified())
        .map(|modified| {
            now.duration_since(modified)
                .map(|age| age > CACHE_TTL)
                .unwrap_or(true)
        })
        .unwrap_or(true);
    if stale {
        let entries = scan_path_value(path_value);
        let refresh_error = write_cache(cache, &entries).err();
        return CacheLoad {
            entries,
            refresh_error,
        };
    }
    match read_cache(cache) {
        Ok(entries) => CacheLoad {
            entries,
            refresh_error: None,
        },
        Err(_) => {
            let entries = scan_path_value(path_value);
            let refresh_error = write_cache(cache, &entries).err();
            CacheLoad {
                entries,
                refresh_error,
            }
        }
    }
}

pub fn entries() -> Vec<PathEntry> {
    let path_value = std::env::var_os("PATH").unwrap_or_default();
    let loaded = entries_with_cache(&path_value, &cache_path(), SystemTime::now());
    if let Some(error) = loaded.refresh_error {
        eprintln!("scroll: could not refresh PATH cache: {error}");
    }
    loaded.entries
}

pub fn matches_filter(query: &str, entry: &PathEntry) -> bool {
    let text = format!("{} {}", entry.name, entry.directory).to_lowercase();
    query
        .split_whitespace()
        .all(|term| text.contains(&term.to_lowercase()))
}

pub fn filtered<'a>(entries: &'a [PathEntry], query: &str) -> Vec<&'a PathEntry> {
    entries
        .iter()
        .filter(|entry| matches_filter(query, entry))
        .collect()
}

pub fn best_match<'a>(entries: &'a [PathEntry], query: &str) -> Option<&'a PathEntry> {
    let query_lower = query.to_lowercase();
    let mut hits: Vec<_> = entries
        .iter()
        .filter(|entry| matches_filter(query, entry))
        .collect();
    hits.sort_by(|left, right| {
        let left_prefix = left.name.to_lowercase().starts_with(&query_lower);
        let right_prefix = right.name.to_lowercase().starts_with(&query_lower);
        right_prefix
            .cmp(&left_prefix)
            .then_with(|| left.name.cmp(&right.name))
    });
    hits.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(label: &str) -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join(format!("scroll-test-{label}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn executable(path: &Path) {
        fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn path_scan_filters_hidden_non_executable_and_duplicate_names() {
        let fixture = Fixture::new("scan");
        let first = fixture.0.join("first");
        let second = fixture.0.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        executable(&first.join("alpha"));
        executable(&second.join("alpha"));
        executable(&second.join("zeta"));
        executable(&second.join(".hidden"));
        fs::write(second.join("plain"), "not executable").unwrap();
        let path_value = std::env::join_paths([&first, &second]).unwrap();

        let entries = scan_path_value(&path_value);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
        assert_eq!(entries[0].directory, first.to_string_lossy());
    }

    #[test]
    fn path_cache_refreshes_after_ttl_and_preserves_fresh_snapshot() {
        let fixture = Fixture::new("cache");
        let bin = fixture.0.join("bin");
        let cache = fixture.0.join("cache/summon.list");
        fs::create_dir_all(&bin).unwrap();
        executable(&bin.join("one"));
        let path_value = std::ffi::OsStr::new(bin.to_str().unwrap());
        let now = SystemTime::now();

        let first = entries_with_cache(path_value, &cache, now);
        assert_eq!(
            first
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["one"]
        );

        executable(&bin.join("two"));
        let fresh = entries_with_cache(path_value, &cache, now + Duration::from_secs(60));
        assert_eq!(
            fresh
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["one"]
        );

        let refreshed =
            entries_with_cache(path_value, &cache, now + CACHE_TTL + Duration::from_secs(2));
        assert_eq!(
            refreshed
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two"]
        );
    }

    #[test]
    fn failed_cache_write_keeps_scanned_commands_and_reports_error() {
        let fixture = Fixture::new("write-failure");
        let bin = fixture.0.join("bin");
        fs::create_dir_all(&bin).unwrap();
        executable(&bin.join("visible"));
        let blocker = fixture.0.join("not-a-directory");
        fs::write(&blocker, "file").unwrap();
        let path_value = std::ffi::OsStr::new(bin.to_str().unwrap());
        let cache = blocker.join("cache");

        let loaded = entries_with_cache(path_value, &cache, SystemTime::now());
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].name, "visible");
        assert!(loaded.refresh_error.is_some());
        assert!(!cache.exists());
    }

    #[test]
    fn cache_write_replaces_complete_snapshot_atomically_and_rejects_bad_rows() {
        let fixture = Fixture::new("atomic-write");
        let cache = fixture.0.join("cache/summon.list");
        let old = vec![PathEntry {
            name: "old".into(),
            directory: "/old".into(),
        }];
        write_cache(&cache, &old).unwrap();
        let new = vec![PathEntry {
            name: "new".into(),
            directory: "/new".into(),
        }];
        write_cache(&cache, &new).unwrap();
        assert_eq!(read_cache(&cache).unwrap(), new);

        let invalid = PathEntry {
            name: "bad\tentry".into(),
            directory: "/bad".into(),
        };
        assert_eq!(
            write_cache(&cache, &[invalid]).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(read_cache(&cache).unwrap(), new);
    }

    #[test]
    fn malformed_cache_is_rebuilt_from_path() {
        let fixture = Fixture::new("malformed-cache");
        let bin = fixture.0.join("bin");
        let cache = fixture.0.join("cache/summon.list");
        fs::create_dir_all(&bin).unwrap();
        executable(&bin.join("fresh"));
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, "truncated cache row\n").unwrap();
        let path_value = std::ffi::OsStr::new(bin.to_str().unwrap());

        let loaded = entries_with_cache(path_value, &cache, SystemTime::now());
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].name, "fresh");
        assert!(loaded.refresh_error.is_none());
        assert_eq!(read_cache(&cache).unwrap(), loaded.entries);
    }

    #[test]
    fn filtering_matches_all_terms_and_exec_prefers_prefix() {
        let entries = vec![
            PathEntry {
                name: "toolbox".into(),
                directory: "/opt/bin".into(),
            },
            PathEntry {
                name: "tool".into(),
                directory: "/usr/bin".into(),
            },
            PathEntry {
                name: "other".into(),
                directory: "/opt/tool-bin".into(),
            },
        ];
        let matches = filtered(&entries, "TOOL bin");
        assert_eq!(matches.len(), 3);
        assert_eq!(best_match(&entries, "tool").unwrap().name, "tool");
        assert_eq!(best_match(&entries, "opt").unwrap().name, "other");
    }
}
