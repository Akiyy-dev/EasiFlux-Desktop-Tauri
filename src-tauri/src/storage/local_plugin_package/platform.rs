use super::{FileIdentity, Hooks, ImportFsStep};
use crate::plugin::{
    discovery::LocalPackageLocator,
    ownership::{OwnershipReceiptV1, VerifiedPackageReceipt},
    record::PluginRecord,
};
use crate::storage::local_plugin_import::ImportPromotionState;
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Write},
    path::{Component, Path},
};

const MANIFEST: &str = "manifest.json";
const RECEIPT: &str = "ownership-receipt.json";

/// One verified plugins handle owns all fixed child roots and their volume boundary.
pub(crate) struct ImportDirectories {
    _plugins: native::Directory,
    pub(crate) staging: native::Directory,
    pub(crate) local: native::Directory,
    pub(crate) removal: native::Directory,
    pub(in crate::storage) hooks: Hooks,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PackageIdentities {
    pub(crate) directory: FileIdentity,
    pub(crate) manifest: FileIdentity,
    pub(crate) receipt: FileIdentity,
}

/// Validation owns the directory and both children until the caller closes them.
/// Later removal consumes these same primitives, never a path-based copy.
pub(crate) struct VerifiedPackage {
    pub(crate) directory: native::Directory,
    manifest: File,
    receipt: File,
}

impl ImportDirectories {
    pub(crate) fn open_or_create(root: &Path) -> io::Result<Self> {
        if !root.is_absolute()
            || root
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(rejected());
        }
        let plugins = native::Directory::open_or_create_root(root)?;
        let staging = plugins.fixed_child("import-staging")?;
        let local = plugins.fixed_child("local")?;
        let removal = plugins.fixed_child("removal-staging")?;
        require_same_volume(&[
            plugins.identity()?,
            staging.identity()?,
            local.identity()?,
            removal.identity()?,
        ])?;
        Ok(Self {
            _plugins: plugins,
            staging,
            local,
            removal,
            hooks: Hooks::default(),
        })
    }

    pub(crate) fn staging_count(&self, limit: usize) -> io::Result<usize> {
        Ok(self.staging.entries(limit)?.len())
    }

    pub(crate) fn create_stage(
        &self,
        name: &str,
        bytes: &[u8],
        receipt: &[u8],
    ) -> io::Result<PackageIdentities> {
        let directory = self.staging.create_stage(name)?;
        let identity = directory.identity()?;
        let mut manifest_identity = None;
        let mut receipt_identity = None;
        let result = (|| {
            self.hooks.checkpoint(ImportFsStep::CreateStage)?;
            {
                let mut file = directory.create_file(MANIFEST)?;
                manifest_identity = Some(native::manifest_identity(&file)?);
                self.hooks.checkpoint(ImportFsStep::WriteManifest)?;
                file.write_all(bytes)?;
                self.hooks.checkpoint(ImportFsStep::SyncManifest)?;
                file.sync_all()?;
            }
            {
                let mut file = directory.create_file(RECEIPT)?;
                receipt_identity = Some(native::manifest_identity(&file)?);
                self.hooks.checkpoint(ImportFsStep::WriteReceipt)?;
                file.write_all(receipt)?;
                self.hooks.checkpoint(ImportFsStep::SyncReceipt)?;
                file.sync_all()?;
            }
            #[cfg(unix)]
            {
                self.hooks.checkpoint(ImportFsStep::SyncStageDirectory)?;
                directory.sync()?;
                self.hooks.checkpoint(ImportFsStep::SyncStagingParent)?;
                self.staging.sync()?;
            }
            Ok(PackageIdentities {
                directory: identity,
                manifest: manifest_identity.unwrap(),
                receipt: receipt_identity.unwrap(),
            })
        })();
        drop(directory);
        let result = result.and_then(|ids| {
            self.verify_stage(name, &ids)?
                .verify_content(bytes, receipt)?;
            Ok(ids)
        });
        if result.is_err()
            && self
                .cleanup_partial(
                    name,
                    &identity,
                    manifest_identity.as_ref(),
                    receipt_identity.as_ref(),
                )
                .is_err()
        {
            tracing::warn!("Unverified partial import stage preserved");
        }
        // Only the initial exclusive directory creation can consume a name collision.
        result.map_err(|error: io::Error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                io::ErrorKind::Other.into()
            } else {
                error
            }
        })
    }

    pub(crate) fn verify_stage(
        &self,
        name: &str,
        ids: &PackageIdentities,
    ) -> io::Result<VerifiedPackage> {
        verify_package(&self.staging, name, ids)
    }

    pub(crate) fn verify_target(
        &self,
        name: &str,
        ids: &PackageIdentities,
    ) -> io::Result<VerifiedPackage> {
        verify_package(&self.local, name, ids)
    }

    pub(crate) fn promote_exclusive(
        &self,
        stage: &str,
        target: &str,
        ids: &PackageIdentities,
        bytes: &[u8],
        receipt: &[u8],
        state: &mut ImportPromotionState,
    ) -> io::Result<()> {
        let verified = self.verify_stage(stage, ids)?;
        verified.verify_content(bytes, receipt)?;
        // Hold source identity at the injection/native boundary; only children close.
        let VerifiedPackage {
            directory,
            manifest,
            receipt: receipt_file,
        } = verified;
        drop(manifest);
        drop(receipt_file);
        self.hooks.checkpoint(ImportFsStep::BeforePromotion)?;
        // Reopen only children below the still-held source after the last hook.
        // Closing the directory here would reopen a source-name race on Windows.
        let (manifest, receipt_file) = verify_children(
            &directory,
            &ids.directory,
            Some(&ids.manifest),
            Some(&ids.receipt),
        )?;
        verify_content(
            &directory,
            &manifest.unwrap(),
            &receipt_file.unwrap(),
            bytes,
            receipt,
        )?;
        #[cfg(test)]
        if let Some(error) = self.hooks.controls.rename_error {
            return Err(error.into());
        }
        #[cfg(unix)]
        if self.staging.open_stage(stage)?.identity()? != ids.directory {
            return Err(rejected());
        }
        *state = ImportPromotionState::CommitUnconfirmed;
        let result =
            native::promote_exclusive(&self.staging, stage, &directory, &self.local, target);
        if result.is_ok() {
            *state = ImportPromotionState::Committed;
        }
        result
    }

    /// EEXIST alone is not proof: source must still be exact and destination distinct.
    pub(crate) fn prove_collision(
        &self,
        stage: &str,
        target: &str,
        ids: &PackageIdentities,
        bytes: &[u8],
        receipt: &[u8],
    ) -> io::Result<()> {
        self.verify_stage(stage, ids)?
            .verify_content(bytes, receipt)?;
        let target_id = self.local.entry_identity(target)?;
        if target_id == ids.directory {
            return Err(rejected());
        }
        Ok(())
    }

    pub(crate) fn cleanup_stage(&self, name: &str, ids: &PackageIdentities) -> io::Result<()> {
        self.cleanup_partial(
            name,
            &ids.directory,
            Some(&ids.manifest),
            Some(&ids.receipt),
        )
    }

    pub(crate) fn cleanup_partial(
        &self,
        name: &str,
        identity: &FileIdentity,
        manifest: Option<&FileIdentity>,
        receipt: Option<&FileIdentity>,
    ) -> io::Result<()> {
        cleanup_exact(&self.staging, name, identity, manifest, receipt)?;
        #[cfg(unix)]
        self.staging.sync()?;
        self.hooks.checkpoint(ImportFsStep::AfterStageRemoval)?;
        require_absent(&self.staging, name)
    }

    pub(crate) fn sync_after_promotion(&self) -> io::Result<()> {
        self.hooks
            .checkpoint(ImportFsStep::SyncDestinationDirectory)?;
        #[cfg(unix)]
        {
            let local = self.local.sync();
            let staging = self.staging.sync();
            local.and(staging)?;
        }
        Ok(())
    }
}

pub(crate) fn require_same_volume(identities: &[FileIdentity]) -> io::Result<()> {
    if identities
        .first()
        .is_some_and(|first| identities.iter().any(|id| id.volume != first.volume))
    {
        return Err(io::ErrorKind::CrossesDevices.into());
    }
    Ok(())
}

impl VerifiedPackage {
    pub(crate) fn verify_content(&self, manifest: &[u8], receipt: &[u8]) -> io::Result<()> {
        verify_content(
            &self.directory,
            &self.manifest,
            &self.receipt,
            manifest,
            receipt,
        )
    }

    pub(crate) fn locator(&self, model: OwnershipReceiptV1) -> io::Result<LocalPackageLocator> {
        Ok(LocalPackageLocator {
            package_slot: model.package_slot().clone(),
            directory_identity: self.directory.identity()?,
            manifest_identity: native::manifest_identity(&self.manifest)?,
            receipt: Some(VerifiedPackageReceipt {
                canonical_sha256: model.canonical_sha256(),
                model,
                file_identity: native::manifest_identity(&self.receipt)?,
            }),
        })
    }
}

fn verify_content(
    directory: &native::Directory,
    manifest_file: &File,
    receipt_file: &File,
    manifest: &[u8],
    receipt: &[u8],
) -> io::Result<()> {
    let mut actual_manifest = Vec::new();
    manifest_file
        .take(16_385)
        .read_to_end(&mut actual_manifest)?;
    let mut actual_receipt = Vec::new();
    receipt_file.take(4_097).read_to_end(&mut actual_receipt)?;
    let parsed = PluginRecord::local_declarative(
        serde_json::from_slice(&actual_manifest).map_err(|_| rejected())?,
    )
    .map_err(|_| rejected())?;
    let model = OwnershipReceiptV1::parse(&actual_receipt).map_err(|_| rejected())?;
    if actual_manifest != manifest
        || actual_receipt != receipt
        || parsed.canonical_manifest_bytes().map_err(|_| rejected())? != actual_manifest
        || model.canonical_bytes().map_err(|_| rejected())? != actual_receipt
        || !model.matches_record(&parsed)
    {
        return Err(rejected());
    }
    exact_names(directory, true, true)?;
    Ok(())
}

pub(crate) fn verify_package(
    parent: &native::Directory,
    name: &str,
    ids: &PackageIdentities,
) -> io::Result<VerifiedPackage> {
    let (directory, manifest, receipt) = verify(
        parent,
        name,
        &ids.directory,
        Some(&ids.manifest),
        Some(&ids.receipt),
    )?;
    Ok(VerifiedPackage {
        directory,
        manifest: manifest.unwrap(),
        receipt: receipt.unwrap(),
    })
}

fn exact_names(directory: &native::Directory, manifest: bool, receipt: bool) -> io::Result<()> {
    let names = directory.entries(3)?;
    if names.len() != usize::from(manifest) + usize::from(receipt)
        || names.iter().any(|name| {
            !((manifest && name == OsStr::new(MANIFEST))
                || (receipt && name == OsStr::new(RECEIPT)))
        })
    {
        return Err(rejected());
    }
    Ok(())
}

fn verify(
    parent: &native::Directory,
    name: &str,
    identity: &FileIdentity,
    manifest: Option<&FileIdentity>,
    receipt: Option<&FileIdentity>,
) -> io::Result<(native::Directory, Option<File>, Option<File>)> {
    let directory = parent.open_stage(name)?;
    let (manifest, receipt) = verify_children(&directory, identity, manifest, receipt)?;
    Ok((directory, manifest, receipt))
}

fn verify_children(
    directory: &native::Directory,
    identity: &FileIdentity,
    manifest: Option<&FileIdentity>,
    receipt: Option<&FileIdentity>,
) -> io::Result<(Option<File>, Option<File>)> {
    if directory.identity()? != *identity {
        return Err(rejected());
    }
    exact_names(directory, manifest.is_some(), receipt.is_some())?;
    let open = |name: &str, expected: Option<&FileIdentity>| -> io::Result<Option<File>> {
        expected
            .map(|expected| {
                let file = directory.open_file(name)?;
                if native::manifest_identity(&file)? != *expected
                    || expected.volume != identity.volume
                {
                    return Err(rejected());
                }
                Ok(file)
            })
            .transpose()
    };
    let manifest = open(MANIFEST, manifest)?;
    let receipt = open(RECEIPT, receipt)?;
    exact_names(directory, manifest.is_some(), receipt.is_some())?;
    Ok((manifest, receipt))
}

/// Exact, nonrecursive deletion shared with later removal. Unknown identities stop all deletion.
pub(crate) fn cleanup_exact(
    parent: &native::Directory,
    name: &str,
    identity: &FileIdentity,
    manifest: Option<&FileIdentity>,
    receipt: Option<&FileIdentity>,
) -> io::Result<()> {
    let (directory, manifest, receipt) = verify(parent, name, identity, manifest, receipt)?;
    if let Some(file) = manifest {
        directory.remove_file(MANIFEST, &file)?;
        drop(file);
    }
    if let Some(file) = receipt {
        directory.remove_file(RECEIPT, &file)?;
        drop(file);
    }
    if !directory.entries(1)?.is_empty() {
        return Err(rejected());
    }
    parent.remove_stage(name, &directory)?;
    // Windows disposition is deletion-pending until every owned handle closes.
    // Success is not cleanup proof: close the final source handle, then require
    // a parent-relative NotFound. Any other result preserves evidence, no retry.
    drop(directory);
    require_absent(parent, name)
}

pub(crate) fn require_absent(parent: &native::Directory, name: &str) -> io::Result<()> {
    match parent.entry_identity(name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
        Ok(_) => Err(rejected()),
    }
}

fn rejected() -> io::Error {
    io::ErrorKind::InvalidInput.into()
}

#[cfg(unix)]
pub(crate) mod native {
    use super::*;
    use rustix::fs::{self, Dir, Mode, OFlags};
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStrExt, fs::MetadataExt},
    };

    const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);
    const FILE_FLAGS: OFlags = OFlags::NOFOLLOW
        .union(OFlags::CLOEXEC)
        .union(OFlags::NONBLOCK)
        .union(OFlags::NOCTTY);

    pub(crate) struct Directory(File);

    impl Directory {
        pub(crate) fn entry_identity(&self, name: &str) -> io::Result<FileIdentity> {
            let stat = fs::statat(&self.0, name, fs::AtFlags::SYMLINK_NOFOLLOW)?;
            Ok(FileIdentity {
                volume: stat.st_dev as u64,
                object: stat.st_ino as u128,
            })
        }
        pub(crate) fn open_or_create_root(root: &Path) -> io::Result<Self> {
            let mut directory = Self(File::from(fs::open("/", DIRECTORY_FLAGS, Mode::empty())?));
            for component in root.components() {
                if let Component::Normal(name) = component {
                    directory = directory.fixed_child_os(name)?;
                }
            }
            Ok(directory)
        }

        fn fixed_child_os(&self, name: &OsStr) -> io::Result<Self> {
            match fs::mkdirat(&self.0, name, Mode::RWXU) {
                Ok(()) | Err(rustix::io::Errno::EXIST) => (),
                Err(error) => return Err(error.into()),
            }
            Ok(Self(File::from(fs::openat(
                &self.0,
                name,
                DIRECTORY_FLAGS,
                Mode::empty(),
            )?)))
        }

        pub(crate) fn fixed_child(&self, name: &str) -> io::Result<Self> {
            self.fixed_child_os(OsStr::new(name))
        }

        pub(crate) fn create_stage(&self, name: &str) -> io::Result<Self> {
            fs::mkdirat(&self.0, name, Mode::RWXU)?;
            // If the new directory cannot be opened and identified, leave it alone.
            self.open_stage(name)
        }

        pub(crate) fn open_stage(&self, name: &str) -> io::Result<Self> {
            Ok(Self(File::from(fs::openat(
                &self.0,
                name,
                DIRECTORY_FLAGS,
                Mode::empty(),
            )?)))
        }

        pub(crate) fn create_file(&self, name: &str) -> io::Result<File> {
            Ok(File::from(fs::openat(
                &self.0,
                name,
                FILE_FLAGS | OFlags::RDWR | OFlags::CREATE | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            )?))
        }

        pub(crate) fn open_file(&self, name: &str) -> io::Result<File> {
            Ok(File::from(fs::openat(
                &self.0,
                name,
                FILE_FLAGS | OFlags::RDONLY,
                Mode::empty(),
            )?))
        }

        pub(crate) fn identity(&self) -> io::Result<FileIdentity> {
            // MetadataExt normalizes platform dev_t widths (macOS uses i32).
            // File::metadata queries this held handle, never the pathname.
            let metadata = self.0.metadata()?;
            if !metadata.is_dir() {
                return Err(rejected());
            }
            Ok(FileIdentity {
                volume: metadata.dev(),
                object: u128::from(metadata.ino()),
            })
        }

        pub(crate) fn entries(&self, limit: usize) -> io::Result<Vec<OsString>> {
            let mut names = Vec::new();
            for entry in Dir::read_from(&self.0)? {
                let entry = entry?;
                let name = entry.file_name().to_bytes();
                if name == b"." || name == b".." {
                    continue;
                }
                names.push(OsStr::from_bytes(name).to_owned());
                if names.len() >= limit {
                    break;
                }
            }
            Ok(names)
        }

        pub(crate) fn remove_file(&self, name: &str, file: &File) -> io::Result<()> {
            if manifest_identity(&self.open_file(name)?)? != manifest_identity(file)? {
                return Err(rejected());
            }
            Ok(fs::unlinkat(&self.0, name, fs::AtFlags::empty())?)
        }

        pub(crate) fn remove_stage(&self, name: &str, stage: &Self) -> io::Result<()> {
            if self.open_stage(name)?.identity()? != stage.identity()? {
                return Err(rejected());
            }
            Ok(fs::unlinkat(&self.0, name, fs::AtFlags::REMOVEDIR)?)
        }

        pub(crate) fn sync(&self) -> io::Result<()> {
            Ok(fs::fsync(&self.0)?)
        }
    }

    pub(crate) fn manifest_identity(file: &File) -> io::Result<FileIdentity> {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(rejected());
        }
        Ok(FileIdentity {
            volume: metadata.dev(),
            object: u128::from(metadata.ino()),
        })
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn promote_exclusive(
        source: &Directory,
        stage: &str,
        held: &Directory,
        destination: &Directory,
        target: &str,
    ) -> io::Result<()> {
        // Single writer/no manual edits required: renameat resolves the source name.
        if source.open_stage(stage)?.identity()? != held.identity()? {
            return Err(rejected());
        }
        Ok(fs::renameat_with(
            &source.0,
            stage,
            &destination.0,
            target,
            fs::RenameFlags::NOREPLACE,
        )?)
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn promote_exclusive(
        source: &Directory,
        stage: &str,
        held: &Directory,
        destination: &Directory,
        target: &str,
    ) -> io::Result<()> {
        if source.open_stage(stage)?.identity()? != held.identity()? {
            return Err(rejected());
        }
        use std::{ffi::CString, os::fd::AsRawFd};
        let stage = CString::new(stage).map_err(|_| rejected())?;
        let target = CString::new(target).map_err(|_| rejected())?;
        // SAFETY: both dirfds remain open and both NUL-terminated names remain
        // live through the call. RENAME_EXCL never replaces a destination.
        let result = unsafe {
            libc::renameatx_np(
                source.0.as_raw_fd(),
                stage.as_ptr(),
                destination.0.as_raw_fd(),
                target.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub(crate) fn promote_exclusive(
        _: &Directory,
        _: &str,
        _: &Directory,
        _: &Directory,
        _: &str,
    ) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(windows)]
pub(crate) mod native {
    use super::*;
    use std::{
        ffi::OsString,
        fs::{self, OpenOptions},
        os::windows::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
            io::{AsRawHandle, FromRawHandle},
        },
        path::{PathBuf, Prefix},
    };
    use windows_sys::{
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                NtCreateFile, FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
                FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT,
            },
        },
        Win32::{
            Foundation::{
                RtlNtStatusToDosError, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING,
            },
            Storage::FileSystem::{
                FileDispositionInfo, FileIdInfo, GetFileInformationByHandle,
                GetFileInformationByHandleEx, SetFileInformationByHandle,
                BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
                FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_INFO, FILE_LIST_DIRECTORY,
                FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };

    const DIRECTORY_ACCESS: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

    pub(crate) struct Directory {
        file: File,
        path: PathBuf,
        // Only a root owns its path's complete chain; child objects are held
        // with their parents by ImportDirectories or its synchronous methods.
        _ancestors: Vec<File>,
    }

    impl Directory {
        pub(crate) fn entry_identity(&self, name: &str) -> io::Result<FileIdentity> {
            match self.open_stage(name) {
                Ok(directory) => directory.identity(),
                Err(_) => identity(&open_child(
                    &self.file,
                    OsStr::new(name),
                    false,
                    FILE_OPEN,
                    false,
                )?),
            }
        }
        pub(crate) fn open_or_create_root(root: &Path) -> io::Result<Self> {
            let mut components = root.components();
            let Some(Component::Prefix(prefix)) = components.next() else {
                return Err(rejected());
            };
            if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
                || components.next() != Some(Component::RootDir)
            {
                return Err(rejected());
            }
            let mut path = PathBuf::from(prefix.as_os_str());
            path.push(r"\");
            let mut file = OpenOptions::new()
                .access_mode(DIRECTORY_ACCESS)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)?;
            directory_identity(&file)?;
            let mut ancestors = Vec::new();
            for component in components {
                let Component::Normal(name) = component else {
                    return Err(rejected());
                };
                let next = open_fixed_child(&file, name)?;
                directory_identity(&next)?;
                ancestors.push(file);
                file = next;
                path.push(name);
            }
            Ok(Self {
                file,
                path,
                _ancestors: ancestors,
            })
        }

        fn child(&self, name: &str, disposition: u32, owned: bool) -> io::Result<Self> {
            let file = open_child(&self.file, OsStr::new(name), true, disposition, owned)?;
            directory_identity(&file)?;
            Ok(Self {
                file,
                path: self.path.join(name),
                _ancestors: Vec::new(),
            })
        }

        pub(crate) fn fixed_child(&self, name: &str) -> io::Result<Self> {
            let file = open_fixed_child(&self.file, OsStr::new(name))?;
            directory_identity(&file)?;
            Ok(Self {
                file,
                path: self.path.join(name),
                _ancestors: Vec::new(),
            })
        }
        pub(crate) fn create_stage(&self, name: &str) -> io::Result<Self> {
            self.child(name, FILE_CREATE, true)
        }
        pub(crate) fn open_stage(&self, name: &str) -> io::Result<Self> {
            self.child(name, FILE_OPEN, true)
        }
        pub(crate) fn create_file(&self, name: &str) -> io::Result<File> {
            open_child(&self.file, OsStr::new(name), false, FILE_CREATE, true)
        }
        pub(crate) fn open_file(&self, name: &str) -> io::Result<File> {
            open_child(&self.file, OsStr::new(name), false, FILE_OPEN, true)
        }
        pub(crate) fn identity(&self) -> io::Result<FileIdentity> {
            directory_identity(&self.file)
        }

        pub(crate) fn entries(&self, limit: usize) -> io::Result<Vec<OsString>> {
            // Fixed ancestors deny delete sharing; the checked stage additionally
            // denies write sharing while enumerating this pinned path.
            fs::read_dir(&self.path)?
                .take(limit)
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect()
        }

        pub(crate) fn remove_file(&self, _name: &str, file: &File) -> io::Result<()> {
            delete_handle(file)
        }
        pub(crate) fn remove_stage(&self, _name: &str, stage: &Self) -> io::Result<()> {
            delete_handle(&stage.file)
        }
    }

    fn open_fixed_child(parent: &File, name: &OsStr) -> io::Result<File> {
        match open_child(parent, name, true, FILE_OPEN, false) {
            Ok(file) => Ok(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match open_child(parent, name, true, FILE_CREATE, false) {
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        open_child(parent, name, true, FILE_OPEN, false)
                    }
                    created => created,
                }
            }
            Err(error) => Err(error),
        }
    }

    fn open_child(
        parent: &File,
        name: &OsStr,
        directory: bool,
        disposition: u32,
        owned: bool,
    ) -> io::Result<File> {
        let mut units: Vec<u16> = name.encode_wide().collect();
        if units.is_empty()
            || name == OsStr::new(".")
            || name == OsStr::new("..")
            || units.iter().any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
        {
            return Err(rejected());
        }
        let byte_len = u16::try_from(units.len().checked_mul(2).ok_or_else(rejected)?)
            .map_err(|_| rejected())?;
        let unicode = UNICODE_STRING {
            Length: byte_len,
            MaximumLength: byte_len,
            Buffer: units.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &unicode,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            ..Default::default()
        };
        let mut access = if directory {
            DIRECTORY_ACCESS
        } else {
            FILE_GENERIC_READ
        };
        if !directory && disposition == FILE_CREATE {
            access |= FILE_GENERIC_WRITE;
        }
        if owned {
            access |= DELETE;
        }
        // Fixed parents permit child-link mutations but remain pinned. The owned
        // source stays open with DELETE access; only children close before rename.
        let sharing = if owned {
            FILE_SHARE_READ
        } else {
            FILE_SHARE_READ | FILE_SHARE_WRITE
        };
        let shape = if directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        };
        let mut handle = std::ptr::null_mut();
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: parent and UTF-16 single-component name stay live throughout
        // this synchronous call. Outputs are valid; a success transfers exactly
        // one owned handle to File. No overwrite disposition is ever used.
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attributes,
                &mut status_block,
                std::ptr::null(),
                0,
                sharing,
                disposition,
                shape | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
                std::ptr::null(),
                0,
            )
        };
        if status < 0 {
            // SAFETY: translates the NTSTATUS value returned by NtCreateFile.
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(status) } as i32,
            ));
        }
        // SAFETY: the successful native call returned a new owned file handle.
        Ok(unsafe { File::from_raw_handle(handle) })
    }

    fn identity(file: &File) -> io::Result<FileIdentity> {
        let mut info = FILE_ID_INFO::default();
        // SAFETY: live file handle and correctly sized writable output buffer.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(FileIdentity {
            volume: info.VolumeSerialNumber,
            object: u128::from_le_bytes(info.FileId.Identifier),
        })
    }

    fn directory_identity(file: &File) -> io::Result<FileIdentity> {
        let metadata = file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(rejected());
        }
        identity(file)
    }

    pub(crate) fn manifest_identity(file: &File) -> io::Result<FileIdentity> {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(rejected());
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: file owns a live handle and info is a writable native struct.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.nNumberOfLinks != 1 {
            return Err(rejected());
        }
        identity(file)
    }

    fn delete_handle(file: &File) -> io::Result<()> {
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        // SAFETY: this is an owned, verified DELETE-capable handle. The native
        // operation marks this object only; directories must already be empty.
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(test)]
    #[test]
    fn windows_native_basic_held_directory_no_replace_probe() {
        use windows_sys::Wdk::Storage::FileSystem::{
            FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION,
        };
        let base = std::env::current_dir().unwrap().join("target");
        let temp = tempfile::tempdir_in(base).unwrap();
        let root = Directory::open_or_create_root(&temp.path().canonicalize().unwrap()).unwrap();
        let staging = root.fixed_child("staging").unwrap();
        let local = root.fixed_child("local").unwrap();
        let source = staging.create_stage("source").unwrap();
        let expected = source.identity().unwrap();
        let child = source.create_file(MANIFEST).unwrap();
        drop(child);
        assert!(fs::rename(&source.path, root.path.join("swapped")).is_err());
        let units: Vec<u16> = "target".encode_utf16().collect();
        let size = std::mem::size_of::<FILE_RENAME_INFORMATION>() + units.len() * 2;
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        // SAFETY: aligned native struct plus name storage and all handles held.
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = local.file.as_raw_handle();
            (*info).FileNameLength = (units.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(
                units.as_ptr(),
                (*info).FileName.as_mut_ptr(),
                units.len(),
            );
            let mut iosb = IO_STATUS_BLOCK::default();
            let status = NtSetInformationFile(
                source.file.as_raw_handle(),
                &mut iosb,
                info.cast(),
                size as u32,
                FileRenameInformation,
            );
            assert!(
                status >= 0,
                "native basic status={status:#x} win32={}",
                RtlNtStatusToDosError(status)
            );
        }
        assert_eq!(source.identity().unwrap(), expected);
        drop(source);
        assert_eq!(
            local.open_stage("target").unwrap().identity().unwrap(),
            expected
        );
        assert!(staging.open_stage("source").is_err());
    }

    #[cfg(test)]
    #[test]
    fn windows_disposition_requires_all_held_handles_closed_before_absence() {
        let base = std::env::current_dir().unwrap().join("target");
        let temp = tempfile::tempdir_in(base).unwrap();
        let root = Directory::open_or_create_root(&temp.path().canonicalize().unwrap()).unwrap();
        let source = root.create_stage("source").unwrap();
        let duplicate = source.file.try_clone().unwrap();
        delete_handle(&source.file).unwrap();
        drop(source);
        assert!(require_absent(&root, "source").is_err());
        drop(duplicate);
        require_absent(&root, "source").unwrap();
    }

    pub(crate) fn promote_exclusive(
        _source: &Directory,
        _stage: &str,
        held: &Directory,
        destination: &Directory,
        target: &str,
    ) -> io::Result<()> {
        use windows_sys::Wdk::Storage::FileSystem::{
            FileRenameInformation, NtSetInformationFile, FILE_RENAME_INFORMATION,
        };
        let units: Vec<u16> = target.encode_utf16().collect();
        if units.is_empty()
            || target == "."
            || target == ".."
            || units.iter().any(|unit| matches!(*unit, 0 | 47 | 58 | 92))
        {
            return Err(rejected());
        }
        let size = std::mem::size_of::<FILE_RENAME_INFORMATION>()
            .checked_add(units.len().checked_mul(2).ok_or_else(rejected)?)
            .ok_or_else(rejected)?;
        let size_u32 = u32::try_from(size).map_err(|_| rejected())?;
        let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        // SAFETY: aligned sizeof-native-struct plus UTF-16 tail. The source and
        // destination parent remain held; basic native rename never replaces.
        unsafe {
            (*info).Anonymous.ReplaceIfExists = false;
            (*info).RootDirectory = destination.file.as_raw_handle();
            (*info).FileNameLength = (units.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(
                units.as_ptr(),
                (*info).FileName.as_mut_ptr(),
                units.len(),
            );
            let mut iosb = IO_STATUS_BLOCK::default();
            let status = NtSetInformationFile(
                held.file.as_raw_handle(),
                &mut iosb,
                info.cast(),
                size_u32,
                FileRenameInformation,
            );
            if status < 0 {
                return Err(io::Error::from_raw_os_error(
                    RtlNtStatusToDosError(status) as i32
                ));
            }
        }
        Ok(())
    }
}
