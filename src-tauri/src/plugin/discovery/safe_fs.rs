//! Bounded bytes only: no JSON, catalog identities, paths, or OS errors leave this API.
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path};

use super::ScanUsage;
use crate::plugin::ownership::{FileIdentity, PackageSlot, RemovalSlot};

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

    #[cfg(test)]
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
    pub(super) package_slot: PackageSlot,
    pub(super) directory_identity: FileIdentity,
    pub(super) manifest_identity: FileIdentity,
    pub(super) manifest_bytes: Vec<u8>,
    pub(super) receipt: Option<(Vec<u8>, FileIdentity)>,
}

#[derive(Default)]
pub(super) struct PackageScan {
    pub(super) packages: Vec<PackageBytes>,
    pub(super) rejected_package_count: u32,
    pub(super) usage: ScanUsage,
    pub(super) occupied_slots: Vec<PackageSlot>,
}

/// Deliberately carries neither paths nor underlying platform error details.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct RootReadError;

/// Deliberately carries neither paths nor underlying platform error details.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct SourceReadError;

pub(crate) fn read_manifest_source(path: &Path) -> Result<Vec<u8>, SourceReadError> {
    read_manifest_source_with_boundary(
        path,
        #[cfg(test)]
        || {},
    )
}

#[cfg(all(test, windows))]
pub(crate) fn read_manifest_source_at_parent_boundary(
    path: &Path,
    before_file_open: impl FnOnce(),
) -> Result<Vec<u8>, SourceReadError> {
    read_manifest_source_with_boundary(path, before_file_open)
}

fn read_manifest_source_with_boundary(
    path: &Path,
    #[cfg(test)] before_file_open: impl FnOnce(),
) -> Result<Vec<u8>, SourceReadError> {
    // Components normalizes interior dots on ordinary paths. Inspect the raw
    // spelling first so no current/parent component disappears before opening.
    let is_separator = |byte: &u8| *byte == b'/' || (cfg!(windows) && *byte == b'\\');
    if !path.is_absolute()
        || path
            .as_os_str()
            .as_encoded_bytes()
            .last()
            .is_some_and(is_separator)
        || path
            .as_os_str()
            .as_encoded_bytes()
            .split(is_separator)
            .any(|part| part == b"." || part == b"..")
        || path.components().any(|component| match component {
            Component::ParentDir | Component::CurDir => true,
            // A drive-prefix colon is not a Normal component. Reject ADS in
            // every other component before any ancestor is opened.
            Component::Normal(name) => cfg!(windows) && name.as_encoded_bytes().contains(&b':'),
            _ => false,
        })
    {
        return Err(SourceReadError);
    }
    let parent = path.parent().ok_or(SourceReadError)?;
    let name = path.file_name().ok_or(SourceReadError)?;
    let directory = platform::SourceDirectory::open_root(parent)
        .map_err(|_| SourceReadError)?
        .ok_or(SourceReadError)?;
    #[cfg(test)]
    before_file_open();
    let mut file = directory
        .open_regular_file(name)
        .map_err(|_| SourceReadError)?;
    // The existing loop reads at most 16,385 bytes, including the probe byte.
    let bytes =
        read_bounded(&mut file, ScanLimits::production(), &mut 0).map_err(|_| SourceReadError);
    drop(file);
    drop(directory);
    bytes
}

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

#[cfg(test)]
pub(super) fn read_package_candidates(
    root: &Path,
    limits: ScanLimits,
) -> Result<PackageScan, RootReadError> {
    limits.validate()?;
    if !safe_root_path(root) {
        return Err(RootReadError);
    }
    let Some(root) = platform::Directory::open_root(root)? else {
        return Ok(PackageScan::default());
    };
    read_local_directory(&root, limits)
}

fn safe_root_path(root: &Path) -> bool {
    root.is_absolute()
        && !root
            .as_os_str()
            .as_encoded_bytes()
            .split(|b| *b == b'/' || (cfg!(windows) && *b == b'\\'))
            .any(|part| part == b"." || part == b"..")
        && !root
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
}

fn read_local_directory(
    root: &platform::Directory,
    limits: ScanLimits,
) -> Result<PackageScan, RootReadError> {
    let names = root
        .entries(limits.max_root_entries)
        .map_err(|_| RootReadError)?;
    let mut result = PackageScan::default();
    result.usage.root_entries = names.len();
    let mut slots = Vec::new();
    for name in names {
        match parse_slot_name(&name) {
            Some(slot) => slots.push(slot),
            None => {
                result.rejected_package_count += 1;
                // Windows case-insensitive aliases are occupied, not absent.
                // This is presence-only evidence and can never create a locator.
                #[cfg(windows)]
                if let Some(slot) = name
                    .to_str()
                    .and_then(|name| PackageSlot::parse(&name.to_ascii_lowercase()).ok())
                {
                    result.occupied_slots.push(slot);
                }
            }
        }
    }
    slots.sort();
    for slot in slots {
        let package_slot = PackageSlot::parse(slot.as_str()).map_err(|_| RootReadError)?;
        result.occupied_slots.push(package_slot.clone());
        let object = read_object(
            root,
            slot.as_str(),
            limits,
            &mut result.usage.bytes_read,
            false,
            &mut || {
                result.usage.packages += 1;
                (result.usage.packages <= limits.max_packages)
                    .then_some(())
                    .ok_or(ReadFailure::Root)
            },
        );
        match object {
            Ok(object) if object.manifest.is_some() => {
                let (manifest_bytes, manifest_identity) = object.manifest.unwrap();
                result.packages.push(PackageBytes {
                    package_slot,
                    #[cfg(test)]
                    slot,
                    directory_identity: object.directory_identity,
                    manifest_identity,
                    manifest_bytes,
                    receipt: object.receipt,
                });
            }
            Err(ReadFailure::Root) => return Err(RootReadError),
            _ => result.rejected_package_count += 1,
        }
    }
    result.occupied_slots.sort();
    if result
        .occupied_slots
        .windows(2)
        .any(|slots| slots[0] == slots[1])
    {
        return Err(RootReadError);
    }
    Ok(result)
}

pub(super) struct ObjectBytes {
    pub(super) unknown_shape: bool,
    pub(super) directory_identity: FileIdentity,
    pub(super) manifest: Option<(Vec<u8>, FileIdentity)>,
    pub(super) receipt: Option<(Vec<u8>, FileIdentity)>,
}

#[cfg(test)]
thread_local! {
    static POST_READ: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

fn known_shape(directory: &platform::Directory, partial: bool) -> Result<(bool, bool), ()> {
    let names = directory.entries(3)?;
    if names
        .iter()
        .any(|name| name != "manifest.json" && name != "ownership-receipt.json")
    {
        return Err(());
    }
    let manifest = names.iter().any(|name| name == "manifest.json");
    let receipt = names.iter().any(|name| name == "ownership-receipt.json");
    if !partial && !manifest {
        return Err(());
    }
    Ok((manifest, receipt))
}

/// Every opened object stays held until both the exact names and named object
/// identities have been checked again. No bytes or identity can authorize deletion alone.
fn read_object(
    root: &platform::Directory,
    name: &str,
    limits: ScanLimits,
    total: &mut usize,
    partial: bool,
    on_shape: &mut dyn FnMut() -> Result<(), ReadFailure>,
) -> Result<ObjectBytes, ReadFailure> {
    let directory = root.open_slot(name).map_err(|_| ReadFailure::Package)?;
    let shape = known_shape(&directory, partial).map_err(|_| ReadFailure::Package)?;
    let directory_identity = directory.identity().map_err(|_| ReadFailure::Package)?;
    let mut manifest = shape
        .0
        .then(|| directory.open_regular_file(OsStr::new("manifest.json")))
        .transpose()
        .map_err(|_| ReadFailure::Package)?;
    let mut receipt = shape
        .1
        .then(|| directory.open_regular_file(OsStr::new("ownership-receipt.json")))
        .transpose()
        .map_err(|_| ReadFailure::Package)?;
    on_shape()?;
    // Do not skip the second known-file probe when the first is oversized.
    let manifest_bytes = manifest
        .as_mut()
        .map(|file| read_bounded(file, limits, total))
        .transpose();
    if matches!(manifest_bytes, Err(ReadFailure::Root)) {
        return Err(ReadFailure::Root);
    }
    let receipt_limits = ScanLimits {
        max_manifest_bytes: 4096,
        ..limits
    };
    let receipt_bytes = receipt
        .as_mut()
        .map(|file| read_bounded(file, receipt_limits, total))
        .transpose();
    if matches!(receipt_bytes, Err(ReadFailure::Root)) {
        return Err(ReadFailure::Root);
    }
    let manifest_bytes = manifest_bytes?;
    let receipt_bytes = receipt_bytes?;
    #[cfg(test)]
    POST_READ.with(|hook| {
        if let Some(callback) = hook.borrow_mut().take() {
            callback();
        }
    });
    let identity = |file: &File| platform::file_identity(file).map_err(|_| ReadFailure::Package);
    let manifest_identity = manifest.as_ref().map(identity).transpose()?;
    let receipt_identity = receipt.as_ref().map(identity).transpose()?;
    if manifest_identity.is_some_and(|identity| identity.volume != directory_identity.volume)
        || receipt_identity.is_some_and(|identity| identity.volume != directory_identity.volume)
    {
        return Err(ReadFailure::Package);
    }
    if known_shape(&directory, partial).ok() != Some(shape)
        || root
            .open_slot(name)
            .and_then(|current| current.identity())
            .ok()
            != Some(directory_identity)
        || manifest_identity.is_some_and(|expected| {
            directory
                .open_regular_file(OsStr::new("manifest.json"))
                .and_then(|current| platform::file_identity(&current))
                .ok()
                != Some(expected)
        })
        || receipt_identity.is_some_and(|expected| {
            directory
                .open_regular_file(OsStr::new("ownership-receipt.json"))
                .and_then(|current| platform::file_identity(&current))
                .ok()
                != Some(expected)
        })
    {
        return Err(ReadFailure::Package);
    }
    Ok(ObjectBytes {
        unknown_shape: false,
        directory_identity,
        manifest: manifest_bytes.zip(manifest_identity),
        receipt: receipt_bytes.zip(receipt_identity),
    })
}

pub(super) struct RemovalScan {
    pub(super) objects: Vec<(RemovalSlot, ObjectBytes)>,
    pub(super) occupied_slots: Vec<RemovalSlot>,
    pub(super) unknown_count: u32,
    pub(super) bytes_read: usize,
}

pub(super) const REMOVAL_BYTE_LIMIT: usize = 328 * 1024;

pub(super) fn read_both_roots(
    root: &Path,
    #[cfg(test)] removal_limit: usize,
) -> (
    Result<PackageScan, RootReadError>,
    Result<RemovalScan, RootReadError>,
) {
    #[cfg(not(test))]
    let removal_limit = REMOVAL_BYTE_LIMIT;
    if !safe_root_path(root) || removal_limit > REMOVAL_BYTE_LIMIT {
        return (Err(RootReadError), Err(RootReadError));
    }
    let root = match platform::Directory::open_root(root) {
        Ok(Some(root)) => root,
        Ok(None) => return (Ok(PackageScan::default()), Ok(empty_removal_scan())),
        Err(_) => return (Err(RootReadError), Err(RootReadError)),
    };
    let Ok(root_identity) = root.identity() else {
        return (Err(RootReadError), Err(RootReadError));
    };
    let local = root
        .optional_child("local")
        .map_err(|_| RootReadError)
        .and_then(|local| {
            local.map_or_else(
                || Ok(PackageScan::default()),
                |local| {
                    if local.identity().map_err(|_| RootReadError)?.volume != root_identity.volume {
                        return Err(RootReadError);
                    }
                    read_local_directory(&local, ScanLimits::production())
                },
            )
        });
    let removals = root
        .optional_child("removal-staging")
        .map_err(|_| RootReadError)
        .and_then(|removals| {
            removals.map_or_else(
                || Ok(empty_removal_scan()),
                |removals| {
                    if removals.identity().map_err(|_| RootReadError)?.volume
                        != root_identity.volume
                    {
                        return Err(RootReadError);
                    }
                    read_removal_directory(&removals, removal_limit)
                },
            )
        });
    (local, removals)
}

fn empty_removal_scan() -> RemovalScan {
    RemovalScan {
        objects: vec![],
        occupied_slots: vec![],
        unknown_count: 0,
        bytes_read: 0,
    }
}

fn read_removal_directory(
    root: &platform::Directory,
    limit: usize,
) -> Result<RemovalScan, RootReadError> {
    let mut names = root.entries(16).map_err(|_| RootReadError)?;
    names.sort();
    let mut result = empty_removal_scan();
    for name in names {
        let Some(slot) = name.to_str().and_then(|name| RemovalSlot::parse(name).ok()) else {
            #[cfg(windows)]
            if let Some(slot) = name
                .to_str()
                .and_then(|name| RemovalSlot::parse(&name.to_ascii_lowercase()).ok())
            {
                result.occupied_slots.push(slot);
                continue;
            }
            result.unknown_count += 1;
            continue;
        };
        result.occupied_slots.push(slot.clone());
        let limits = ScanLimits {
            max_total_bytes: limit,
            ..ScanLimits::production()
        };
        match read_object(
            root,
            slot.as_str(),
            limits,
            &mut result.bytes_read,
            true,
            &mut || Ok(()),
        ) {
            Ok(object) => result.objects.push((slot, object)),
            Err(ReadFailure::Root) => return Err(RootReadError),
            Err(ReadFailure::Package) => {
                // Unknown children are never read. Retain only a safely reopened
                // directory identity as conflict evidence, not cleanup authority.
                if let Ok(directory_identity) = root
                    .open_slot(slot.as_str())
                    .and_then(|directory| directory.identity())
                {
                    result.objects.push((
                        slot,
                        ObjectBytes {
                            unknown_shape: true,
                            directory_identity,
                            manifest: None,
                            receipt: None,
                        },
                    ));
                }
            }
        }
    }
    result.occupied_slots.sort();
    if result
        .occupied_slots
        .windows(2)
        .any(|slots| slots[0] == slots[1])
    {
        return Err(RootReadError);
    }
    Ok(result)
}

#[cfg(unix)]
pub(crate) fn is_single_link_regular_file(
    mode: rustix::fs::RawMode,
    links: impl Into<u64>,
) -> bool {
    rustix::fs::FileType::from_raw_mode(mode) == rustix::fs::FileType::RegularFile
        && links.into() == 1
}

#[cfg(unix)]
mod platform {
    use super::*;
    use rustix::fs::{fstat, open, openat, Dir, Mode, OFlags};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;

    const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    pub(super) struct Directory(File);

    pub(super) type SourceDirectory = Directory;

    impl Directory {
        pub(super) fn identity(&self) -> Result<FileIdentity, ()> {
            let metadata = self.0.metadata().map_err(|_| ())?;
            Ok(FileIdentity {
                volume: metadata.dev(),
                object: u128::from(metadata.ino()),
            })
        }

        pub(super) fn optional_child(&self, name: &str) -> Result<Option<Self>, ()> {
            match openat(&self.0, name, DIRECTORY_FLAGS, Mode::empty()) {
                Ok(fd) => Ok(Some(Self(File::from(fd)))),
                Err(rustix::io::Errno::NOENT) => Ok(None),
                Err(_) => Err(()),
            }
        }
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
            Ok(Some(Self(File::from(held))))
        }

        pub(super) fn open_slot(&self, slot: &str) -> Result<Self, ()> {
            openat(&self.0, slot, DIRECTORY_FLAGS, Mode::empty())
                .map(File::from)
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

        pub(super) fn open_regular_file(&self, name: &OsStr) -> Result<File, ()> {
            let fd = openat(
                &self.0,
                name,
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
            if !is_single_link_regular_file(metadata.st_mode, metadata.st_nlink) {
                return Err(());
            }
            Ok(File::from(fd))
        }
    }

    pub(super) fn file_identity(file: &File) -> Result<FileIdentity, ()> {
        let metadata = file.metadata().map_err(|_| ())?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(());
        }
        Ok(FileIdentity {
            volume: metadata.dev(),
            object: u128::from(metadata.ino()),
        })
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::{PathBuf, Prefix};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        NtCreateFile, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
        FILE_OPEN_REPARSE_POINT, FILE_SEQUENTIAL_ONLY, FILE_SYNCHRONOUS_IO_NONALERT,
    };
    use windows_sys::Win32::Foundation::{OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING};
    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, FILE_ID_INFO,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ,
        FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    pub(super) fn is_reparse_point(attributes: u32) -> bool {
        attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    pub(super) struct Directory {
        path: PathBuf,
        // Discovery retains its existing attribute-only read/write-sharing mode.
        _held: Vec<File>,
    }

    // No pathname is retained: every source child is opened relative to the
    // immediately preceding handle, even if an ancestor becomes a reparse point.
    pub(super) struct SourceDirectory {
        held: Vec<File>,
    }

    impl SourceDirectory {
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
            // Only the disk root is absolute. Nothing below it is reopened by path.
            let mut disk_root = PathBuf::from(prefix.as_os_str());
            disk_root.push(r"\");
            let mut held = vec![open_source_directory(&disk_root).map_err(|_| RootReadError)?];
            for component in components {
                let Component::Normal(name) = component else {
                    return Err(RootReadError);
                };
                let child = open_source_child(held.last().ok_or(RootReadError)?, name, true)
                    .map_err(|_| RootReadError)?;
                held.push(child);
            }
            Ok(Some(Self { held }))
        }

        pub(super) fn open_regular_file(&self, name: &OsStr) -> Result<File, ()> {
            open_source_child(self.held.last().ok_or(())?, name, false)
        }
    }

    fn open_source_child(parent: &File, name: &OsStr, directory: bool) -> Result<File, ()> {
        open_relative_child(parent, name, directory).map_err(|_| ())
    }

    fn open_relative_child(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
        // Native relative names must be exactly one component: no namespace,
        // separator, dot navigation, ADS, or embedded NUL can reach NtCreateFile.
        let mut units: Vec<u16> = name.encode_wide().collect();
        if units.is_empty()
            || name == OsStr::new(".")
            || name == OsStr::new("..")
            || units.iter().any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
        {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let byte_len = u16::try_from(
            units
                .len()
                .checked_mul(2)
                .ok_or(io::ErrorKind::InvalidInput)?,
        )
        .map_err(|_| io::ErrorKind::InvalidInput)?;
        let unicode_name = UNICODE_STRING {
            Length: byte_len,
            MaximumLength: byte_len,
            Buffer: units.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &unicode_name,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            ..Default::default()
        };
        let access = if directory {
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE
        } else {
            FILE_GENERIC_READ
        };
        let shape = if directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE | FILE_SEQUENTIAL_ONLY
        };
        let mut handle = std::ptr::null_mut();
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: parent is live; unicode_name and its initialized UTF-16 buffer
        // outlive this synchronous call. Output pointers are valid and writable.
        // FILE_OPEN cannot create/replace anything. A successful handle is owned
        // exactly once by File below, including all post-open validation failures.
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attributes,
                &mut status_block,
                std::ptr::null(),
                0,
                FILE_SHARE_READ,
                FILE_OPEN,
                shape | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                std::ptr::null(),
                0,
            )
        };
        if status < 0 {
            // SAFETY: converts a scalar NTSTATUS, with no pointer arguments.
            return Err(io::Error::from_raw_os_error(unsafe {
                windows_sys::Win32::Foundation::RtlNtStatusToDosError(status)
            } as i32));
        }
        // SAFETY: NtCreateFile succeeded and transferred this owned file handle.
        let file = unsafe { File::from_raw_handle(handle) };
        if directory {
            validate_directory(&file).map_err(|_| io::ErrorKind::InvalidInput)?;
        } else {
            validate_regular_file(&file).map_err(|_| io::ErrorKind::InvalidInput)?;
        }
        Ok(file)
    }

    fn open_directory(path: &Path) -> io::Result<File> {
        open_directory_with_access(
            path,
            FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
        )
    }

    fn open_source_directory(path: &Path) -> io::Result<File> {
        // Attribute-only opens do not establish the required sharing checks.
        // Real directory-read access, without write/delete sharing, pins every
        // ancestor against rename and incompatible writer handles until read.
        open_directory_with_access(
            path,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES,
            FILE_SHARE_READ,
        )
    }

    fn open_directory_with_access(path: &Path, access: u32, sharing: u32) -> io::Result<File> {
        let file = OpenOptions::new()
            .access_mode(access)
            .share_mode(sharing)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        validate_directory(&file)?;
        Ok(file)
    }

    fn validate_directory(file: &File) -> io::Result<()> {
        let metadata = file.metadata()?;
        if !metadata.is_dir() || is_reparse_point(metadata.file_attributes()) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        no_named_streams(file).map_err(|_| io::ErrorKind::InvalidInput)?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn open_manifest(path: &Path) -> Result<File, ()> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT
                    | windows_sys::Win32::Storage::FileSystem::FILE_FLAG_SEQUENTIAL_SCAN,
            )
            .open(path)
            .map_err(|_| ())?;
        validate_regular_file(&file)?;
        Ok(file)
    }

    fn validate_regular_file(file: &File) -> Result<(), ()> {
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
        no_named_streams(file)?;
        Ok(())
    }

    /// A named stream is extra user data even though directory enumeration does
    /// not show it. Query only the held object; never open or read a stream path.
    fn no_named_streams(file: &File) -> Result<(), ()> {
        use windows_sys::Wdk::Storage::FileSystem::{
            FileStreamInformation, NtQueryInformationFile,
        };
        let mut storage = [0u64; 512];
        let mut status = IO_STATUS_BLOCK::default();
        // SAFETY: storage is initialized, 8-byte aligned, writable for the stated
        // length; the owned handle and status block outlive the synchronous call.
        let result = unsafe {
            NtQueryInformationFile(
                file.as_raw_handle(),
                &mut status,
                storage.as_mut_ptr().cast(),
                std::mem::size_of_val(&storage) as u32,
                FileStreamInformation,
            )
        };
        if result < 0 || status.Information > std::mem::size_of_val(&storage) {
            return Err(());
        }
        // SAFETY: the byte view remains inside the initialized storage buffer.
        let bytes = unsafe {
            std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), status.Information)
        };
        if bytes.is_empty() {
            return Ok(());
        }
        // The only permitted stream is the unnamed default data stream. Any
        // continuation (including an overlong response) is conservatively unsafe.
        if bytes.len() < 38 {
            return Err(());
        }
        let next = u32::from_le_bytes(bytes[0..4].try_into().map_err(|_| ())?);
        let length = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| ())?);
        let expected: Vec<u8> = "::$DATA"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        if next != 0 || length != 14 || bytes[24..38] != expected {
            return Err(());
        }
        Ok(())
    }

    fn object_identity(file: &File) -> Result<FileIdentity, ()> {
        let mut info = FILE_ID_INFO::default();
        // SAFETY: the live owned handle and correctly sized writable buffer
        // remain valid for the synchronous metadata query.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if ok == 0 {
            return Err(());
        }
        Ok(FileIdentity {
            volume: info.VolumeSerialNumber,
            object: u128::from_le_bytes(info.FileId.Identifier),
        })
    }

    pub(super) fn file_identity(file: &File) -> Result<FileIdentity, ()> {
        validate_regular_file(file)?;
        object_identity(file)
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
                match open_relative_child(held.last().ok_or(RootReadError)?, name, true) {
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
            let held = open_source_child(self._held.last().ok_or(())?, OsStr::new(slot), true)?;
            Ok(Self {
                path,
                _held: vec![held],
            })
        }

        pub(super) fn identity(&self) -> Result<FileIdentity, ()> {
            object_identity(self._held.last().ok_or(())?)
        }

        pub(super) fn optional_child(&self, name: &str) -> Result<Option<Self>, ()> {
            match open_relative_child(self._held.last().ok_or(())?, OsStr::new(name), true) {
                Ok(file) => {
                    // Windows lookup ignores case: require the exact enumerated name.
                    if !self.entries(256)?.iter().any(|entry| entry == name) {
                        return Err(());
                    }
                    Ok(Some(Self {
                        path: self.path.join(name),
                        _held: vec![file],
                    }))
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(()),
            }
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

        pub(super) fn open_regular_file(&self, name: &OsStr) -> Result<File, ()> {
            open_source_child(self._held.last().ok_or(())?, name, false)
        }
    }
}

#[cfg(test)]
mod tests;
