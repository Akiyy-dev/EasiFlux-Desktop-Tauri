use super::{LocalManifestReader, SystemLocalManifestReader};
use crate::error::{AppError, AppResult};
use crate::plugin::import::{test_support::VALID, PreparedManifest};
use crate::plugin::manifest::PluginManifestV1;
use std::fs;
use std::path::Path;

fn fixture() -> tempfile::TempDir {
    // Owned fixture roots avoid sandboxed user-home handles and macOS /var links.
    let base = std::env::current_dir().unwrap().join("target");
    fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base.canonicalize().unwrap()).unwrap()
}

fn assert_source_rejected(result: AppResult<PreparedManifest>) {
    let Err(error) = result else {
        panic!("unsafe source was accepted");
    };
    assert!(matches!(
        error,
        AppError::Plugin {
            code: "plugin_import_source_rejected",
            diagnostic: None,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::json!({
            "code": "plugin_import_source_rejected",
            "message": "无法安全读取所选文件，请选择普通本地 JSON 文件。"
        })
    );
}

#[test]
fn captures_selected_content_without_reopening_the_source() {
    let temp = fixture();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("user-chosen-name.json");
    fs::write(&source, VALID).unwrap();
    let reader: &(dyn LocalManifestReader + Send + Sync) = &SystemLocalManifestReader;
    let captured = reader.read(&source).unwrap();
    fs::write(&source, b"now invalid").unwrap();
    fs::remove_file(&source).unwrap();
    let reread: PluginManifestV1 = serde_json::from_slice(captured.bytes()).unwrap();
    assert_eq!(reread.id.as_str(), "com.example.notes");
    assert_eq!(reread.description, "Metadata only");
}

#[test]
fn rejects_an_external_hard_link() {
    let temp = fixture();
    let outside = fixture();
    let first = outside.path().canonicalize().unwrap().join("a.json");
    let second = temp.path().canonicalize().unwrap().join("b.json");
    fs::write(&first, VALID).unwrap();
    fs::hard_link(&first, &second).unwrap();
    assert_source_rejected(SystemLocalManifestReader.read(&second));
    assert_eq!(fs::read(first).unwrap(), VALID);
}

#[test]
fn source_limit_accepts_16384_rejects_16385() {
    let temp = fixture();
    let source = temp.path().canonicalize().unwrap().join("padded.json");
    for (length, accepted) in [(16_384, true), (16_385, false), (1_000_000, false)] {
        let mut bytes = VALID.to_vec();
        bytes.resize(length, b' ');
        fs::write(&source, bytes).unwrap();
        let result = SystemLocalManifestReader.read(&source);
        if accepted {
            assert_eq!(
                result.unwrap().record().manifest().id.as_str(),
                "com.example.notes"
            );
        } else {
            assert_source_rejected(result);
        }
    }
}

#[test]
fn relative_and_parent_paths_rejected() {
    let temp = fixture();
    let root = temp.path().canonicalize().unwrap();
    fs::create_dir(root.join("child")).unwrap();
    fs::write(root.join("source.json"), VALID).unwrap();
    for path in ["source.json", "./source.json", "../source.json"] {
        assert_source_rejected(SystemLocalManifestReader.read(Path::new(path)));
    }
    // Append raw text: PathBuf::push normalizes verbatim Windows dot components.
    for suffix in [
        "/child/../source.json",
        "/./source.json",
        "/child/./../source.json",
    ] {
        let suffix = suffix.replace('/', std::path::MAIN_SEPARATOR_STR);
        let mut path = root.as_os_str().to_os_string();
        path.push(suffix);
        assert_source_rejected(SystemLocalManifestReader.read(Path::new(&path)));
    }
}

#[test]
fn source_directory_rejected() {
    let temp = fixture();
    assert_source_rejected(SystemLocalManifestReader.read(&temp.path().canonicalize().unwrap()));
}

#[test]
fn trailing_separator_does_not_silently_select_a_regular_file() {
    let temp = fixture();
    let source = temp.path().canonicalize().unwrap().join("source.json");
    fs::write(&source, VALID).unwrap();
    let mut path = source.into_os_string();
    path.push(std::path::MAIN_SEPARATOR_STR);
    assert_source_rejected(SystemLocalManifestReader.read(Path::new(&path)));
}

#[test]
fn missing_source_and_missing_ancestor_have_sanitized_errors() {
    let temp = fixture();
    let root = temp.path().canonicalize().unwrap();
    for source in [
        root.join("private-missing.json"),
        root.join("missing/private.json"),
    ] {
        assert_source_rejected(SystemLocalManifestReader.read(&source));
    }
}

#[test]
fn safely_read_invalid_json_keeps_the_manifest_invalid_error() {
    let temp = fixture();
    let source = temp.path().canonicalize().unwrap().join("invalid.json");
    fs::write(&source, b"private invalid bytes").unwrap();
    assert!(matches!(
        SystemLocalManifestReader.read(&source),
        Err(AppError::Plugin {
            code: "plugin_import_manifest_invalid",
            diagnostic: None,
            ..
        })
    ));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{symlink, FileTypeExt};

    #[test]
    fn source_and_ancestor_symlinks_are_rejected() {
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let target = root.join("real");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("source.json"), VALID).unwrap();
        symlink(target.join("source.json"), root.join("source-link.json")).unwrap();
        symlink(&target, root.join("ancestor-link")).unwrap();
        for path in [
            root.join("source-link.json"),
            root.join("ancestor-link/source.json"),
        ] {
            assert_source_rejected(SystemLocalManifestReader.read(&path));
        }
    }

    #[test]
    fn fifo_without_writer_does_not_block() {
        let temp = fixture();
        let source = temp.path().canonicalize().unwrap().join("fifo.json");
        let output = std::process::Command::new("mkfifo")
            .args(["-m", "600"])
            .arg(&source)
            .output()
            .expect("POSIX mkfifo must be available for Unix security tests");
        assert!(output.status.success(), "mkfifo failed: {output:?}");
        assert!(fs::symlink_metadata(&source).unwrap().file_type().is_fifo());
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            tx.send(SystemLocalManifestReader.read(&source)).unwrap();
        });
        let result = rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("FIFO must not wait for a writer");
        assert_source_rejected(result);
        worker.join().unwrap();
    }

    #[test]
    fn device_source_is_rejected() {
        assert_source_rejected(SystemLocalManifestReader.read(Path::new("/dev/null")));
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::fs::{symlink_file, OpenOptionsExt};
    use std::path::PathBuf;
    use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

    fn disk_path(path: &Path) -> PathBuf {
        PathBuf::from(path.to_str().unwrap().strip_prefix(r"\\?\").unwrap())
    }

    #[test]
    fn normal_disk_current_components_are_not_normalized_away() {
        let temp = fixture();
        let root = disk_path(&temp.path().canonicalize().unwrap());
        fs::write(root.join("source.json"), VALID).unwrap();
        for suffix in [r"\.\source.json", "/./source.json"] {
            let mut path = root.as_os_str().to_os_string();
            path.push(suffix);
            assert_source_rejected(SystemLocalManifestReader.read(Path::new(&path)));
        }
    }

    #[test]
    fn normal_and_verbatim_disk_paths_are_accepted_and_ancestor_handles_released() {
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let parent = root.join("目录");
        fs::create_dir(&parent).unwrap();
        let source = parent.join("清单.json");
        fs::write(&source, VALID).unwrap();
        for path in [&source, &disk_path(&source)] {
            let captured = SystemLocalManifestReader.read(path).unwrap();
            assert_eq!(
                captured.record().manifest().id.as_str(),
                "com.example.notes"
            );
        }
        fs::rename(&parent, root.join("moved")).unwrap();
    }

    #[test]
    fn unc_device_and_ads_paths_are_rejected() {
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("source.json");
        fs::write(&source, VALID).unwrap();
        let stream = root.join("source.json:private");
        fs::write(&stream, VALID).unwrap();
        for path in [
            PathBuf::from(r"\\localhost\c$\source.json"),
            PathBuf::from(r"\\?\UNC\localhost\c$\source.json"),
            PathBuf::from(r"\\.\NUL"),
            PathBuf::from(r"\\?\GLOBALROOT\Device\Null"),
            PathBuf::from(r"C:\NUL"),
            PathBuf::from(r"C:source.json"),
            stream.clone(),
            disk_path(&stream),
            root.join("source.json:private/child.json"),
        ] {
            assert_source_rejected(SystemLocalManifestReader.read(&path));
        }
    }

    #[test]
    fn source_and_ancestor_junctions_are_rejected() {
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let target = root.join("real");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("source.json"), VALID).unwrap();
        let link = root.join("junction");
        let output = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "junction creation failed: {output:?}"
        );
        assert_source_rejected(SystemLocalManifestReader.read(&link));
        assert_source_rejected(SystemLocalManifestReader.read(&link.join("source.json")));
        fs::remove_dir(link).unwrap();
    }

    #[test]
    fn source_file_symlink_is_rejected() {
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("source.json");
        let link = root.join("link.json");
        fs::write(&source, VALID).unwrap();
        if let Err(error) = symlink_file(&source, &link) {
            if error.raw_os_error() == Some(1314) {
                eprintln!("SKIP source_file_symlink_is_rejected: ERROR_PRIVILEGE_NOT_HELD");
                return;
            }
            panic!("symlink creation failed: {error}");
        }
        assert_source_rejected(SystemLocalManifestReader.read(&link));
    }

    #[test]
    fn source_with_an_active_writer_is_rejected() {
        let temp = fixture();
        let source = temp.path().canonicalize().unwrap().join("source.json");
        fs::write(&source, VALID).unwrap();
        let writer = fs::OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ)
            .open(&source)
            .unwrap();
        assert_source_rejected(SystemLocalManifestReader.read(&source));
        drop(writer);
        assert!(SystemLocalManifestReader.read(&source).is_ok());
    }
}
