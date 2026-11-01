//! Serialize rwd writers, then replace a complete file within its directory.

use super::{OutputError, managed};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

pub(super) fn save(
    directory: &Path,
    date: chrono::NaiveDate,
    section: managed::Section,
    body: &str,
) -> Result<PathBuf, OutputError> {
    fs::create_dir_all(directory)?;
    let path = directory.join(format!("{date}.md"));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(format!(".{date}.rwd.lock")))?;
    lock.try_lock().map_err(|e| {
        format!(
            "{}: 다른 rwd 저장과 충돌했거나 잠금을 얻지 못했습니다: {e}",
            path.display()
        )
    })?;
    let existing = read_note(&path)?;
    let updated = managed::update(existing.as_deref().unwrap_or(""), section, body)?;
    if existing.as_deref() != Some(&updated) {
        replace(&path, existing.as_deref(), &updated)?;
    }
    Ok(path)
}

fn read_note(path: &Path) -> Result<Option<String>, OutputError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.is_file() => {
            return Err(format!(
                "{}: 일반 파일이 아니므로 저장하지 않았습니다",
                path.display()
            )
            .into());
        }
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    Ok(Some(fs::read_to_string(path).map_err(|e| {
        format!(
            "{}: {}",
            path.display(),
            crate::messages::error::file_read_failed(&e)
        )
    })?))
}

struct PendingFile(PathBuf);

impl Drop for PendingFile {
    fn drop(&mut self) {
        // Never remove a file we did not create. The lock file stays in place
        // so another process cannot lock a different inode under the same name.
        let _ = fs::remove_file(&self.0);
    }
}

fn temporary(path: &Path) -> Result<(PendingFile, File), OutputError> {
    for _ in 0..100 {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp = path.with_extension(format!("rwd-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => return Ok((PendingFile(temp), file)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err("안전한 임시 파일을 만들지 못해 저장하지 않았습니다".into())
}

fn replace(path: &Path, original: Option<&str>, updated: &str) -> Result<(), OutputError> {
    let (temp, mut file) = temporary(path)?;
    if original.is_some() {
        file.set_permissions(fs::metadata(path)?.permissions())?;
    }
    file.write_all(updated.as_bytes())?;
    file.sync_all()?;
    commit_file(&temp.0, path, original)
}

fn commit_file(temp: &Path, path: &Path, original: Option<&str>) -> Result<(), OutputError> {
    let current = read_note(path)?;
    if original.is_some() && current.is_none() {
        return Err(crate::messages::error::daily_markdown_not_found(&path.display()).into());
    }
    if current.as_deref() != original {
        return Err("저장 중 일일 노트가 변경되어 교체하지 않았습니다".into());
    }
    fs::rename(temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("rwd_vault_{}_{id}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_read_failure_and_lock_contention_preserve_original() {
        let dir = directory();
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        let path = dir.join(format!("{date}.md"));
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(save(&dir, date, managed::Section::Analysis, "new").is_err());
        assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe]);
        fs::write(&path, "original").unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join(format!(".{date}.rwd.lock")))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(save(&dir, date, managed::Section::Summary, "new").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        drop(lock);
        save(&dir, date, managed::Section::Analysis, "new").unwrap();
        assert!(fs::read_to_string(&path).unwrap().starts_with("original"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_failed_commit_keeps_external_edit_and_cleans_temporary_file() {
        let dir = directory();
        let path = dir.join("2099-01-01.md");
        fs::write(&path, "external edit").unwrap();
        assert!(replace(&path, Some("stale original"), "new").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert!(replace(&path, None, "new").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
        assert!(commit_file(&dir.join("missing.tmp"), &path, Some("external edit")).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
        fs::remove_file(&path).unwrap();
        assert!(replace(&path, Some("deleted original"), "new").is_err());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_invalid_output_directory_returns_error() {
        let dir = directory();
        let not_directory = dir.join("file");
        fs::write(&not_directory, "preserve").unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        assert!(save(&not_directory, date, managed::Section::Analysis, "new").is_err());
        assert_eq!(fs::read_to_string(&not_directory).unwrap(), "preserve");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_summary_can_create_note_and_both_regions_remain_unique() {
        let dir = directory();
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        let path = save(&dir, date, managed::Section::Summary, "progress").unwrap();
        for _ in 0..3 {
            save(&dir, date, managed::Section::Analysis, "analysis").unwrap();
            save(&dir, date, managed::Section::Summary, "progress").unwrap();
        }
        let saved = fs::read_to_string(path).unwrap();
        assert_eq!(saved.matches("<!-- rwd:analysis:v1:").count(), 1);
        assert_eq!(saved.matches("<!-- rwd:summary:v1:").count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn test_symlink_is_rejected_and_file_permissions_are_preserved() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = directory();
        let original = dir.join("original.md");
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        let path = dir.join(format!("{date}.md"));
        fs::write(&original, "keep").unwrap();
        symlink(&original, &path).unwrap();
        assert!(save(&dir, date, managed::Section::Analysis, "new").is_err());
        assert_eq!(fs::read_to_string(&original).unwrap(), "keep");
        assert!(
            fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_file(&path).unwrap();
        fs::write(&path, "user note").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        save(&dir, date, managed::Section::Analysis, "new").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
