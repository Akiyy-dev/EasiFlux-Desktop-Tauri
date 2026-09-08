use super::*;
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture(PathBuf);

#[test]
fn post_read_shape_and_identity_replacements_fail_closed_with_handles_held() {
    for swap in [false, true] {
        let f = Fixture::new();
        let package = f.package(0, b"{}");
        fs::write(package.join("ownership-receipt.json"), b"{}").unwrap();
        POST_READ.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                if swap {
                    let result = fs::rename(package.join("manifest.json"), package.join("old"));
                    #[cfg(windows)]
                    assert_eq!(result.unwrap_err().raw_os_error(), Some(32));
                    #[cfg(unix)]
                    {
                        result.unwrap();
                        fs::write(package.join("manifest.json"), b"{}").unwrap();
                        fs::remove_file(package.join("old")).unwrap();
                    }
                } else {
                    fs::write(package.join("extra"), b"keep").unwrap();
                }
            }))
        });
        let scan = f.scan().unwrap();
        if cfg!(windows) && swap {
            assert_eq!(scan.packages.len(), 1);
        } else {
            assert!(scan.packages.is_empty());
        }
    }
}

#[test]
fn rejected_manifest_does_not_mask_receipt_aggregate_overflow() {
    let f = Fixture::new();
    let package = f.package(0, &vec![b'x'; 16_385]);
    fs::write(package.join("ownership-receipt.json"), vec![b'x'; 4097]).unwrap();
    let limits = ScanLimits {
        max_total_bytes: 20_000,
        ..ScanLimits::production()
    };
    assert!(read_package_candidates(f.root(), limits).is_err());
}

#[test]
fn two_file_probe_bytes_and_wrong_case_extra_hardlink_are_bounded() {
    let f = Fixture::new();
    let package = f.package(0, b"{}");
    fs::write(package.join("ownership-receipt.json"), vec![b'x'; 4097]).unwrap();
    let scan = f.scan().unwrap();
    assert!(scan.packages.is_empty());
    assert_eq!(scan.usage.bytes_read, 4099);
    fs::remove_file(package.join("ownership-receipt.json")).unwrap();
    fs::write(package.join("Ownership-Receipt.json"), b"{}").unwrap();
    assert!(f.scan().unwrap().packages.is_empty());
    fs::remove_file(package.join("Ownership-Receipt.json")).unwrap();
    fs::write(f.root().join("receipt-source"), b"{}").unwrap();
    fs::hard_link(
        f.root().join("receipt-source"),
        package.join("ownership-receipt.json"),
    )
    .unwrap();
    assert!(f.scan().unwrap().packages.is_empty());
}
impl Fixture {
    fn new() -> Self {
        // Keep fixtures beneath the checkout: sandboxed Windows processes may
        // create temp files but cannot hold a handle to the user-home ancestor.
        let base = std::env::current_dir().unwrap().join("target");
        fs::create_dir_all(&base).unwrap();
        let base = base.canonicalize().unwrap();
        let path = base.join(format!("easiflux-safe-fs-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn root(&self) -> &Path {
        &self.0
    }
    fn package(&self, n: usize, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(format!("pkg-{n:032x}"));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("manifest.json"), bytes).unwrap();
        path
    }
    fn scan(&self) -> Result<PackageScan, RootReadError> {
        read_package_candidates(self.root(), ScanLimits::production())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn slot_parser_accepts_only_pkg_prefix_and_32_lower_hex() {
    assert!(
        parse_slot_name(std::ffi::OsStr::new("pkg-550e8400e29b41d4a716446655440000")).is_some()
    );
    for invalid in [
        "con.plugin",
        "com1.foo",
        "nul.anything",
        "pkg-550E8400e29b41d4a716446655440000",
        "550e8400-e29b-41d4-a716-446655440000",
        "pkg-550e8400e29b41d4a71644665544000",
        "pkg-550e8400e29b41d4a7164466554400000",
        "pkg-550e8400e29b41d4a71644665544000g",
        "pkg-550e8400e29b41d4a716446655440000 ",
    ] {
        assert!(
            parse_slot_name(std::ffi::OsStr::new(invalid)).is_none(),
            "{invalid}"
        );
    }
}

#[test]
fn missing_root_is_empty_success() {
    let f = Fixture::new();
    let result =
        read_package_candidates(&f.root().join("missing/local"), ScanLimits::production()).unwrap();
    assert!(result.packages.is_empty());
    assert_eq!(result.rejected_package_count, 0);
    assert_eq!(result.usage, super::super::ScanUsage::default());
}

// Catches counting only accepted bytes/packages or ignoring non-package root entries.
#[test]
fn scan_usage_counts_junk_structural_packages_and_over_limit_probe() {
    let f = Fixture::new();
    f.package(0, b"ok");
    f.package(1, &vec![0; 20_000]);
    let malformed = f.package(2, b"never read");
    fs::write(malformed.join("extra"), b"").unwrap();
    fs::write(f.root().join("junk"), b"never read").unwrap();
    let result = f.scan().unwrap();
    assert_eq!(result.packages.len(), 1);
    assert_eq!(result.rejected_package_count, 3);
    assert_eq!(
        result.usage,
        super::super::ScanUsage {
            root_entries: 4,
            packages: 2,
            bytes_read: 16_387,
        }
    );
}

#[test]
fn relative_root_is_unavailable() {
    assert!(
        read_package_candidates(Path::new("relative/local"), ScanLimits::production()).is_err()
    );
}

#[test]
fn non_directory_root_is_unavailable() {
    let f = Fixture::new();
    fs::write(f.root().join("file"), b"{}").unwrap();
    assert!(read_package_candidates(&f.root().join("file"), ScanLimits::production()).is_err());
}

#[test]
fn package_requires_exactly_one_case_exact_manifest_file() {
    let f = Fixture::new();
    let p = f.package(0, b"{}");
    fs::rename(p.join("manifest.json"), p.join("Manifest.json")).unwrap();
    let result = f.scan().unwrap();
    assert!(result.packages.is_empty());
    assert_eq!(result.rejected_package_count, 1);
}

#[test]
fn missing_manifest_is_rejected() {
    let f = Fixture::new();
    let p = f.package(0, b"{}");
    fs::remove_file(p.join("manifest.json")).unwrap();
    assert_eq!(f.scan().unwrap().rejected_package_count, 1);
}

#[test]
fn extra_file_is_rejected() {
    let f = Fixture::new();
    let p = f.package(0, b"{}");
    fs::write(p.join("extra"), b"").unwrap();
    let result = f.scan().unwrap();
    assert!(result.packages.is_empty());
    assert_eq!(result.rejected_package_count, 1);
}

#[test]
fn nested_directory_is_rejected() {
    let f = Fixture::new();
    let p = f.package(0, b"{}");
    fs::create_dir(p.join("nested")).unwrap();
    let result = f.scan().unwrap();
    assert!(result.packages.is_empty());
    assert_eq!(result.rejected_package_count, 1);
}

#[test]
fn manifest_directory_is_rejected() {
    let f = Fixture::new();
    let p = f.package(0, b"{}");
    fs::remove_file(p.join("manifest.json")).unwrap();
    fs::create_dir(p.join("manifest.json")).unwrap();
    assert_eq!(f.scan().unwrap().rejected_package_count, 1);
}

#[test]
fn manifest_hard_link_to_outside_file_is_rejected() {
    let f = Fixture::new();
    let outside = Fixture::new();
    let secret = outside.root().join("outside-manifest");
    fs::write(&secret, b"outside bytes").unwrap();
    let p = f.package(0, b"");
    fs::remove_file(p.join("manifest.json")).unwrap();
    fs::hard_link(&secret, p.join("manifest.json")).unwrap();

    let result = f.scan().unwrap();
    assert!(result.packages.is_empty());
    assert_eq!(result.rejected_package_count, 1);
    assert_eq!(fs::read(&secret).unwrap(), b"outside bytes");
}

#[test]
fn ordinary_single_link_manifest_is_accepted() {
    let f = Fixture::new();
    f.package(0, b"single-link bytes");
    let result = f.scan().unwrap();
    assert_eq!(result.packages.len(), 1);
    assert_eq!(result.packages[0].manifest_bytes, b"single-link bytes");
    assert_eq!(result.rejected_package_count, 0);
}

#[test]
fn manifest_at_16_kib_is_returned_as_unparsed_bytes() {
    let f = Fixture::new();
    let bytes = vec![0xff; 16 * 1024];
    f.package(0, &bytes);
    let result = f.scan().unwrap();
    assert_eq!(result.packages[0].manifest_bytes, bytes);
    assert_eq!(result.rejected_package_count, 0);
}

#[test]
fn manifest_over_16_kib_is_rejected() {
    let f = Fixture::new();
    f.package(0, &vec![0; 16 * 1024 + 1]);
    let result = f.scan().unwrap();
    assert!(result.packages.is_empty());
    assert_eq!(result.rejected_package_count, 1);
}

#[test]
fn root_at_256_entries_counts_all_junk() {
    let f = Fixture::new();
    for n in 0..256 {
        fs::write(f.root().join(format!("junk-{n}")), b"").unwrap();
    }
    let result = f.scan().unwrap();
    assert!(result.packages.is_empty());
    assert_eq!(result.rejected_package_count, 256);
}

#[test]
fn root_over_256_entries_is_unavailable_not_truncated() {
    let f = Fixture::new();
    for n in 0..257 {
        fs::write(f.root().join(format!("junk-{n}")), b"").unwrap();
    }
    assert!(f.scan().is_err());
}

#[test]
fn exactly_128_packages_and_2_mib_are_accepted() {
    let f = Fixture::new();
    for n in 0..128 {
        f.package(n, &vec![0; 16 * 1024]);
    }
    let result = f.scan().unwrap();
    assert_eq!(result.packages.len(), 128);
    assert_eq!(
        result
            .packages
            .iter()
            .map(|p| p.manifest_bytes.len())
            .sum::<usize>(),
        2 * 1024 * 1024
    );
}

#[test]
fn over_128_structurally_valid_packages_is_unavailable() {
    let f = Fixture::new();
    for n in 0..129 {
        f.package(n, b"");
    }
    assert!(f.scan().is_err());
}

#[test]
fn aggregate_input_includes_rejected_oversize_bytes() {
    let f = Fixture::new();
    for n in 0..128 {
        f.package(n, &vec![0; 16 * 1024 + 1]);
    }
    assert!(f.scan().is_err());
}

#[test]
fn explicit_total_budget_is_enforced_at_exact_boundary() {
    let f = Fixture::new();
    f.package(0, b"abcd");
    f.package(1, b"efgh");
    let mut limits = ScanLimits::production();
    limits.max_total_bytes = 8;
    assert_eq!(
        read_package_candidates(f.root(), limits)
            .unwrap()
            .packages
            .len(),
        2
    );
    limits.max_total_bytes = 7;
    assert!(read_package_candidates(f.root(), limits).is_err());
}

#[test]
fn slots_are_sorted_deterministically() {
    let f = Fixture::new();
    for n in [9, 2, 7] {
        f.package(n, &[n as u8]);
    }
    let result = f.scan().unwrap();
    let slots: Vec<_> = result.packages.iter().map(|p| p.slot.as_str()).collect();
    assert_eq!(
        slots,
        [
            "pkg-00000000000000000000000000000002",
            "pkg-00000000000000000000000000000007",
            "pkg-00000000000000000000000000000009"
        ]
    );
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{symlink, FileTypeExt};

    fn create_fifo(path: &Path) {
        // POSIX mkfifo is available on macOS too, unlike rustix 1.1.4's
        // mknodat/mkfifoat. Pass the absolute fixture path as one argument,
        // never through a shell, and fail the test if creation is unavailable.
        assert!(path.is_absolute());
        let output = std::process::Command::new("mkfifo")
            .args(["-m", "600"])
            .arg(path)
            .output()
            .expect("POSIX mkfifo must be available for Unix security tests");
        assert!(output.status.success(), "mkfifo failed: {output:?}");
        assert!(fs::symlink_metadata(path).unwrap().file_type().is_fifo());
    }

    #[test]
    fn root_intermediate_symlink_is_unavailable() {
        let f = Fixture::new();
        let target = Fixture::new();
        fs::create_dir(target.root().join("local")).unwrap();
        symlink(target.root(), f.root().join("link")).unwrap();
        assert!(
            read_package_candidates(&f.root().join("link/local"), ScanLimits::production())
                .is_err()
        );
    }

    #[test]
    fn slot_symlink_is_rejected() {
        let f = Fixture::new();
        let target = Fixture::new();
        fs::write(target.root().join("manifest.json"), b"outside").unwrap();
        symlink(
            target.root(),
            f.root().join("pkg-00000000000000000000000000000000"),
        )
        .unwrap();
        let result = f.scan().unwrap();
        assert!(result.packages.is_empty());
        assert_eq!(result.rejected_package_count, 1);
    }

    #[test]
    fn manifest_symlink_is_rejected() {
        let f = Fixture::new();
        let target = Fixture::new();
        let p = f.package(0, b"");
        fs::write(target.root().join("secret"), b"outside").unwrap();
        fs::remove_file(p.join("manifest.json")).unwrap();
        symlink(target.root().join("secret"), p.join("manifest.json")).unwrap();
        let result = f.scan().unwrap();
        assert!(result.packages.is_empty());
        assert_eq!(result.rejected_package_count, 1);
    }

    #[test]
    fn fifo_without_writer_is_rejected_promptly_then_regular_file_succeeds() {
        let f = Fixture::new();
        let p = f.package(0, b"");
        let manifest = p.join("manifest.json");
        fs::remove_file(&manifest).unwrap();
        create_fifo(&manifest);
        let root = f.root().to_owned();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            tx.send(read_package_candidates(&root, ScanLimits::production()))
                .unwrap();
        });
        let result = rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .expect("FIFO must not wait for a writer")
            .unwrap();
        assert!(result.packages.is_empty());
        assert_eq!(result.rejected_package_count, 1);
        worker.join().unwrap();
        fs::remove_file(&manifest).unwrap();
        fs::write(&manifest, b"{}").unwrap();
        assert_eq!(f.scan().unwrap().packages.len(), 1);
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::fs::{symlink_file, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_SHARE_READ,
    };

    #[test]
    fn named_alternate_streams_on_package_or_known_files_are_rejected() {
        for object in ["directory", "manifest.json", "ownership-receipt.json"] {
            let fixture = Fixture::new();
            let package = fixture.package(0, b"{}");
            fs::write(package.join("ownership-receipt.json"), b"{}").unwrap();
            let stream = if object == "directory" {
                PathBuf::from(format!("{}:private", package.display()))
            } else {
                package.join(format!("{object}:private"))
            };
            fs::write(&stream, b"must not be implicitly removed").unwrap();
            let scan = fixture.scan().unwrap();
            assert!(scan.packages.is_empty(), "accepted ADS on {object}");
            assert_eq!(fs::read(stream).unwrap(), b"must not be implicitly removed");
        }
    }

    #[test]
    fn reparse_attribute_is_always_rejected() {
        assert!(platform::is_reparse_point(FILE_ATTRIBUTE_REPARSE_POINT));
        assert!(platform::is_reparse_point(
            FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT
        ));
        assert!(!platform::is_reparse_point(FILE_ATTRIBUTE_DIRECTORY));
    }

    #[test]
    fn unc_root_is_unavailable() {
        assert!(read_package_candidates(
            Path::new(r"\\localhost\c$\local"),
            ScanLimits::production()
        )
        .is_err());
        assert!(read_package_candidates(
            Path::new(r"\\?\UNC\localhost\c$\local"),
            ScanLimits::production()
        )
        .is_err());
    }

    fn junction(link: &Path, target: &Path) {
        let result = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "junction creation failed: {:?}",
            result
        );
    }

    #[test]
    fn real_directory_junction_is_rejected_at_root_and_slot() {
        let f = Fixture::new();
        let target = Fixture::new();
        fs::create_dir(target.root().join("local")).unwrap();
        junction(&f.root().join("link"), target.root());
        assert!(
            read_package_candidates(&f.root().join("link/local"), ScanLimits::production())
                .is_err()
        );
        fs::remove_dir(f.root().join("link")).unwrap();
        junction(
            &f.root().join("pkg-00000000000000000000000000000000"),
            target.root(),
        );
        let result = f.scan().unwrap();
        assert!(result.packages.is_empty());
        assert_eq!(result.rejected_package_count, 1);
        fs::remove_dir(f.root().join("pkg-00000000000000000000000000000000")).unwrap();
    }

    #[test]
    fn real_manifest_file_symlink_is_rejected() {
        let f = Fixture::new();
        let target = Fixture::new();
        let p = f.package(0, b"");
        fs::write(target.root().join("secret"), b"outside").unwrap();
        fs::remove_file(p.join("manifest.json")).unwrap();
        if let Err(error) = symlink_file(target.root().join("secret"), p.join("manifest.json")) {
            if error.raw_os_error() == Some(1314) {
                eprintln!("SKIP real_manifest_file_symlink_is_rejected: ERROR_PRIVILEGE_NOT_HELD");
                return;
            }
            panic!("symlink creation failed: {error}");
        }
        let result = f.scan().unwrap();
        assert!(result.packages.is_empty());
        assert_eq!(result.rejected_package_count, 1);
    }

    #[test]
    fn opened_manifest_cannot_be_renamed_while_held() {
        let f = Fixture::new();
        let p = f.package(0, b"{}");
        let held = platform::open_manifest(&p.join("manifest.json")).unwrap();
        let error = fs::rename(p.join("manifest.json"), p.join("renamed")).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(32));
        drop(held);
        fs::rename(p.join("manifest.json"), p.join("renamed")).unwrap();
    }

    #[test]
    fn sharing_violation_rejects_manifest() {
        let f = Fixture::new();
        let p = f.package(0, b"{}");
        let _writer = fs::OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ)
            .open(p.join("manifest.json"))
            .unwrap();
        let result = f.scan().unwrap();
        assert!(result.packages.is_empty());
        assert_eq!(result.rejected_package_count, 1);
    }
}
