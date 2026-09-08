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
    fn in_memory_source_modes_require_a_regular_file_and_one_link() {
        use crate::plugin::discovery::safe_fs::is_single_link_regular_file;
        use rustix::fs::FileType;
        let regular = FileType::RegularFile.as_raw_mode() | 0o600;
        let character_device = FileType::CharacterDevice.as_raw_mode() | 0o600;
        for (mode, links, accepted) in [
            (regular, 1u64, true),
            (character_device, 1, false),
            (regular, 0, false),
            (regular, 2, false),
        ] {
            assert_eq!(is_single_link_regular_file(mode, links), accepted);
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::plugin::discovery::safe_fs::read_manifest_source_at_parent_boundary;
    use std::os::windows::fs::{symlink_file, OpenOptionsExt};
    use std::path::PathBuf;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    fn reparse_write_handle(path: &Path) -> std::io::Result<fs::File> {
        // FSCTL_SET_REPARSE_POINT requires write access. GENERIC_WRITE supplies
        // FILE_WRITE_DATA and FILE_WRITE_ATTRIBUTES; allow every sharing mode
        // here so failure comes from the reader's held ancestor restrictions.
        fs::OpenOptions::new()
            .access_mode(FILE_GENERIC_WRITE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
    }

    #[test]
    fn parent_boundary_rejects_in_place_attribute_only_junction_retarget() {
        use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
        use windows_sys::Win32::System::IO::DeviceIoControl;
        // Keep test-only Win32 control codes local; no extra production feature.
        const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xa0000003;
        const FSCTL_SET_REPARSE_POINT: u32 = 0x900a4;
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let parent = root.join("parent");
        let target = root.join("target");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&target).unwrap();
        let source = parent.join("source.json");
        fs::write(&source, VALID).unwrap();
        fs::write(target.join("source.json"), VALID).unwrap();
        let substitute: Vec<u16> = format!(r"\??\{}", disk_path(&target).display())
            .encode_utf16()
            .collect();
        let print: Vec<u16> = disk_path(&target).as_os_str().encode_wide().collect();
        let mut data = Vec::new();
        data.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        data.extend((8u16 + ((substitute.len() + print.len() + 2) * 2) as u16).to_le_bytes());
        data.extend(0u16.to_le_bytes());
        data.extend(0u16.to_le_bytes());
        data.extend(((substitute.len() * 2) as u16).to_le_bytes());
        data.extend(((substitute.len() * 2 + 2) as u16).to_le_bytes());
        data.extend(((print.len() * 2) as u16).to_le_bytes());
        for unit in substitute
            .iter()
            .chain([0u16].iter())
            .chain(print.iter())
            .chain([0u16].iter())
        {
            data.extend(unit.to_le_bytes());
        }
        let captured = read_manifest_source_at_parent_boundary(&source, || {
            let result = fs::OpenOptions::new()
                .access_mode(windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&parent);
            let handle = result.unwrap();
            fs::remove_file(&source).unwrap();
            let mut returned = 0;
            // SAFETY: live handle and initialized input bytes; no output buffer.
            let set = unsafe {
                DeviceIoControl(
                    handle.as_raw_handle(),
                    FSCTL_SET_REPARSE_POINT,
                    data.as_ptr().cast(),
                    data.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    &mut returned,
                    std::ptr::null_mut(),
                )
            };
            assert_ne!(
                set,
                0,
                "fixture junction update failed: {:?}",
                std::io::Error::last_os_error().raw_os_error()
            );
        });
        fs::remove_dir(parent).unwrap();
        assert!(
            captured.is_err(),
            "a retargeted ancestor exposed the replacement source"
        );
    }

    #[test]
    fn parent_boundary_pins_every_ancestor_against_rename_until_capture_finishes() {
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let ancestor = root.join("ancestor");
        let parent = ancestor.join("parent");
        fs::create_dir_all(&parent).unwrap();
        let source = parent.join("source.json");
        fs::write(&source, VALID).unwrap();
        let mut reached_boundary = false;
        let bytes = read_manifest_source_at_parent_boundary(&source, || {
            reached_boundary = true;
            for path in [&ancestor, &parent] {
                let error = fs::rename(path, root.join("replacement-slot")).unwrap_err();
                assert!(matches!(error.raw_os_error(), Some(5 | 32)));
            }
        })
        .unwrap();
        assert!(reached_boundary);
        assert_eq!(bytes, VALID);
        for path in [&ancestor, &parent] {
            fs::rename(path, root.join("replacement-slot")).unwrap();
            fs::rename(root.join("replacement-slot"), path).unwrap();
        }
    }

    #[test]
    fn parent_boundary_denies_reparse_write_handles_until_capture_finishes() {
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let ancestor = root.join("ancestor");
        let parent = ancestor.join("parent");
        fs::create_dir_all(&parent).unwrap();
        let source = parent.join("source.json");
        fs::write(&source, VALID).unwrap();
        let mut reached_boundary = false;
        let bytes = read_manifest_source_at_parent_boundary(&source, || {
            reached_boundary = true;
            for path in [&ancestor, &parent] {
                let error = reparse_write_handle(path).unwrap_err();
                assert_eq!(error.raw_os_error(), Some(32));
            }
        })
        .unwrap();
        assert!(reached_boundary);
        assert_eq!(bytes, VALID);
        for path in [&ancestor, &parent] {
            drop(reparse_write_handle(path).unwrap());
        }
    }

    #[test]
    fn an_existing_ancestor_writer_rejects_source_before_capture() {
        let temp = fixture();
        let root = temp.path().canonicalize().unwrap();
        let parent = root.join("parent");
        fs::create_dir(&parent).unwrap();
        let source = parent.join("source.json");
        fs::write(&source, VALID).unwrap();
        let writer = reparse_write_handle(&parent).unwrap();
        assert_source_rejected(SystemLocalManifestReader.read(&source));
        drop(writer);
        assert!(SystemLocalManifestReader.read(&source).is_ok());
    }

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
