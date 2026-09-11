//! Bounded bytes only: no JSON, catalog identities, paths, or OS errors leave this API.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path};

#[derive(Clone, Copy)]
pub(super) struct ScanLimits {
    pub(super) max_root_entries: usize,
    pub(super) max_packages: usize,
    pub(super) max_manifest_bytes: usize,
    pub(super) max_total_bytes: usize,
}

impl ScanLimits {
    pub(super) const fn production() -> Self {
        Self {
            max_root_entries: 256,
            max_packages: 128,
            max_manifest_bytes: 16 * 1024,
            max_total_bytes: 2 * 1024 * 1024,
        }
    }

    fn validate(self) -> Result<(), RootReadError> {
        let ceiling = Self::production();
        if self.max_root_entries > ceiling.max_root_entries
            || self.max_packages > ceiling.max_packages
            || self.max_manifest_bytes > ceiling.max_manifest_bytes
            || self.max_total_bytes > ceiling.max_total_bytes
        {
            return Err(RootReadError);
        }
        Ok(())
    }
}

/// Opaque ordering key, never a plugin identity or an arbitrary path component.
#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct LocalPackageSlot(String);

impl LocalPackageSlot {
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

fn parse_slot_name(name: &OsStr) -> Option<LocalPackageSlot> {
    let name = name.to_str()?;
    let suffix = name.strip_prefix("pkg-")?;
    (suffix.len() == 32
        && suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then(|| LocalPackageSlot(name.to_owned()))
}

pub(super) struct PackageBytes {
    // Retained only so tests can assert deterministic scan ordering.
    #[cfg(test)]
    pub(super) slot: LocalPackageSlot,
    pub(super) manifest_bytes: Vec<u8>,
}

#[derive(Default)]
pub(super) struct PackageScan {
    pub(super) packages: Vec<PackageBytes>,
    pub(super) rejected_package_count: u32,
}

/// Deliberately carries neither paths nor underlying platform error details.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct RootReadError;

enum ReadFailure {
    Package,
    Root,
}

/// Every byte actually read counts, including rejected data and the over-limit
/// probe byte. Never read an entire file before applying either byte bound.
fn read_bounded(
    file: &mut File,
    limits: ScanLimits,
    total: &mut usize,
) -> Result<Vec<u8>, ReadFailure> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let count = buffer
            .len()
            .min(limits.max_manifest_bytes + 1 - bytes.len())
            .min(limits.max_total_bytes + 1 - *total);
        let count = match file.read(&mut buffer[..count]) {
            Ok(0) => return Ok(bytes),
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(ReadFailure::Package),
        };
        *total += count;
        if *total > limits.max_total_bytes {
            return Err(ReadFailure::Root);
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > limits.max_manifest_bytes {
            return Err(ReadFailure::Package);
        }
    }
}

pub(super) fn read_package_candidates(
    root: &Path,
    limits: ScanLimits,
) -> Result<PackageScan, RootReadError> {
    limits.validate()?;
    if !root.is_absolute()
        || root
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(RootReadError);
    }
    let Some(root) = platform::Directory::open_root(root)? else {
        return Ok(PackageScan::default());
    };
    let names = root
        .entries(limits.max_root_entries)
        .map_err(|_| RootReadError)?;
    let mut result = PackageScan::default();
    let mut slots = Vec::new();
    for name in names {
        match parse_slot_name(&name) {
            Some(slot) => slots.push(slot),
            None => result.rejected_package_count += 1,
        }
    }
    slots.sort();
    let mut structurally_acceptable = 0;
    let mut total = 0;
    for slot in slots {
        let opened = root.open_slot(slot.as_str()).and_then(|directory| {
            directory.check_shape()?;
            let manifest = directory.open_manifest()?;
            Ok((directory, manifest))
        });
        let (directory, mut manifest) = match opened {
            Ok(opened) => opened,
            Err(()) => {
                result.rejected_package_count += 1;
                continue;
            }
        };
        structurally_acceptable += 1;
        if structurally_acceptable > limits.max_packages {
            return Err(RootReadError);
        }
        match read_bounded(&mut manifest, limits, &mut total) {
            Ok(manifest_bytes) if directory.check_shape().is_ok() => {
                result.packages.push(PackageBytes {
                    #[cfg(test)]
                    slot,
                    manifest_bytes,
                });
            }
            Err(ReadFailure::Root) => return Err(RootReadError),
            _ => result.rejected_package_count += 1,
        }
        // Keep both handles alive through the second shape check.
        drop(manifest);
        drop(directory);
    }
    Ok(result)
}

#[cfg(unix)]
mod platform {
    use super::*;
    use rustix::fd::OwnedFd;
    use rustix::fs::{fstat, open, openat, Dir, FileType, Mode, OFlags};
    use std::os::unix::ffi::OsStrExt;

    const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    pub(super) struct Directory(OwnedFd);

    impl Directory {
        pub(super) fn open_root(path: &Path) -> Result<Option<Self>, RootReadError> {
            let mut held = open("/", DIRECTORY_FLAGS, Mode::empty()).map_err(|_| RootReadError)?;
            for component in path.components() {
                let Component::Normal(name) = component else {
                    continue;
                };
                held = match openat(&held, name, DIRECTORY_FLAGS, Mode::empty()) {
                    Ok(fd) => fd,
                    Err(rustix::io::Errno::NOENT) => return Ok(None),
                    Err(_) => return Err(RootReadError),
                };
            }
            Ok(Some(Self(held)))
        }

        pub(super) fn open_slot(&self, slot: &str) -> Result<Self, ()> {
            openat(&self.0, slot, DIRECTORY_FLAGS, Mode::empty())
                .map(Self)
                .map_err(|_| ())
        }

        pub(super) fn entries(&self, max: usize) -> Result<Vec<OsString>, ()> {
            let dir = Dir::read_from(&self.0).map_err(|_| ())?;
            let mut names = Vec::new();
            for entry in dir {
                let entry = entry.map_err(|_| ())?;
                let name = entry.file_name().to_bytes();
                if name == b"." || name == b".." {
                    continue;
                }
                if names.len() == max {
                    return Err(());
                }
                names.push(OsStr::from_bytes(name).to_owned());
            }
            Ok(names)
        }

        pub(super) fn check_shape(&self) -> Result<(), ()> {
            let names = self.entries(1)?;
            if names.len() == 1 && names[0] == OsStr::new("manifest.json") {
                Ok(())
            } else {
                Err(())
            }
        }

        pub(super) fn open_manifest(&self) -> Result<File, ()> {
            let fd = openat(
                &self.0,
                "manifest.json",
                OFlags::RDONLY
                    | OFlags::NOFOLLOW
                    | OFlags::CLOEXEC
                    | OFlags::NONBLOCK
                    | OFlags::NOCTTY,
                Mode::empty(),
            )
            .map_err(|_| ())?;
            // Do not read (or trust a pre-open type check) before this fstat.
            let metadata = fstat(&fd).map_err(|_| ())?;
            if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
                || metadata.st_nlink != 1
            {
                return Err(());
            }
            Ok(File::from(fd))
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use std::path::{PathBuf, Prefix};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_SEQUENTIAL_SCAN,
        FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    pub(super) fn is_reparse_point(attributes: u32) -> bool {
        attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    pub(super) struct Directory {
        path: PathBuf,
        // Excluding FILE_SHARE_DELETE pins every path prefix against replacement.
        // Ancestors stay held for the complete root scan, slots through each read.
        _held: Vec<File>,
    }

    fn open_directory(path: &Path) -> io::Result<File> {
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_dir() || is_reparse_point(metadata.file_attributes()) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        Ok(file)
    }

    pub(super) fn open_manifest(path: &Path) -> Result<File, ()> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_SEQUENTIAL_SCAN)
            .open(path)
            .map_err(|_| ())?;
        let metadata = file.metadata().map_err(|_| ())?;
        if !metadata.is_file() || is_reparse_point(metadata.file_attributes()) {
            return Err(());
        }
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file owns a live handle throughout the call; information is a
        // valid, writable struct of the exact type required by this Win32 API.
        let success = unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) };
        if success == 0 || information.nNumberOfLinks != 1 {
            return Err(());
        }
        Ok(file)
    }

    impl Directory {
        pub(super) fn open_root(path: &Path) -> Result<Option<Self>, RootReadError> {
            let mut components = path.components();
            let Some(Component::Prefix(prefix)) = components.next() else {
                return Err(RootReadError);
            };
            if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
                || components.next() != Some(Component::RootDir)
            {
                return Err(RootReadError);
            }
            let mut current = PathBuf::from(prefix.as_os_str());
            current.push(r"\");
            let first = open_directory(&current).map_err(|_| RootReadError)?;
            let mut held = vec![first];
            for component in components {
                let Component::Normal(name) = component else {
                    return Err(RootReadError);
                };
                current.push(name);
                match open_directory(&current) {
                    Ok(file) => held.push(file),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
                    Err(_) => return Err(RootReadError),
                }
            }
            Ok(Some(Self {
                path: current,
                _held: held,
            }))
        }

        pub(super) fn open_slot(&self, slot: &str) -> Result<Self, ()> {
            let path = self.path.join(slot);
            let held = open_directory(&path).map_err(|_| ())?;
            Ok(Self {
                path,
                _held: vec![held],
            })
        }

        pub(super) fn entries(&self, max: usize) -> Result<Vec<OsString>, ()> {
            let mut names = Vec::new();
            for entry in fs::read_dir(&self.path).map_err(|_| ())? {
                let entry = entry.map_err(|_| ())?;
                if names.len() == max {
                    return Err(());
                }
                names.push(entry.file_name());
            }
            Ok(names)
        }

        pub(super) fn check_shape(&self) -> Result<(), ()> {
            let names = self.entries(1)?;
            if names.len() == 1 && names[0] == OsStr::new("manifest.json") {
                Ok(())
            } else {
                Err(())
            }
        }

        pub(super) fn open_manifest(&self) -> Result<File, ()> {
            open_manifest(&self.path.join("manifest.json"))
        }
    }
}

#[cfg(test)]
mod tests;
