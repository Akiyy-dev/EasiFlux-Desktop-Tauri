use super::{FileIdentity, Hooks, ImportFsStep};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Write},
    path::{Component, Path},
};

const MANIFEST: &str = "manifest.json";

/// Fixed plugins/staging/local handles stay open through stage ownership;
/// Windows additionally pins their complete ancestor chain.
pub(super) struct ImportDirectories {
    _plugins: native::Directory,
    staging: native::Directory,
    local: native::Directory,
    pub(super) hooks: Hooks,
}

impl ImportDirectories {
    pub(super) fn open_or_create(root: &Path) -> io::Result<Self> {
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
        if staging.identity()?.volume != local.identity()?.volume {
            return Err(io::ErrorKind::CrossesDevices.into());
        }
        Ok(Self {
            _plugins: plugins,
            staging,
            local,
            hooks: Hooks::default(),
        })
    }

    pub(super) fn staging_count(&self, limit: usize) -> io::Result<usize> {
        Ok(self.staging.entries(limit)?.len())
    }

    pub(super) fn create_stage(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> io::Result<(FileIdentity, FileIdentity)> {
        let directory = self.staging.create_stage(name)?;
        // If identity acquisition fails, ownership is uncertain: preserve the object.
        let directory_identity = directory.identity()?;
        let mut manifest_identity = None;
        let result: io::Result<(FileIdentity, FileIdentity)> = (|| {
            self.hooks.checkpoint(ImportFsStep::CreateStage)?;
            let mut file = directory.create_manifest()?;
            let identity = native::manifest_identity(&file)?;
            manifest_identity = Some(identity);
            self.hooks.checkpoint(ImportFsStep::WriteManifest)?;
            file.write_all(bytes)?;
            self.hooks.checkpoint(ImportFsStep::SyncManifest)?;
            file.sync_all()?;
            #[cfg(unix)]
            {
                self.hooks.checkpoint(ImportFsStep::SyncStageDirectory)?;
                directory.sync()?;
                self.hooks.checkpoint(ImportFsStep::SyncStagingParent)?;
                self.staging.sync()?;
            }
            Ok((directory_identity, identity))
        })();
        // Windows stage/file no-delete handles must close before cleanup/move.
        drop(directory);
        let result = result.and_then(|identities| {
            self.verify_stage(name, &identities.0, &identities.1)?;
            Ok(identities)
        });
        if result.is_err() {
            let cleaned =
                self.cleanup_partial(name, &directory_identity, manifest_identity.as_ref());
            if let Err(error) = cleaned {
                tracing::warn!(kind = ?error.kind(), "Unverified partial import stage preserved");
            }
        }
        // AlreadyExists denotes only a collision at the initial mkdir/create.
        // Any failure after acquiring a new stage is not a new-name retry.
        result.map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                io::ErrorKind::Other.into()
            } else {
                error
            }
        })
    }

    pub(super) fn promote_exclusive(&self, stage: &str, target: &str) -> io::Result<()> {
        self.hooks.checkpoint(ImportFsStep::BeforePromotion)?;
        #[cfg(test)]
        if let Some(error) = self.hooks.controls.rename_error {
            return Err(error.into());
        }
        native::promote_exclusive(&self.staging, stage, &self.local, target)
    }

    pub(super) fn verify_stage(
        &self,
        name: &str,
        directory: &FileIdentity,
        manifest: &FileIdentity,
    ) -> io::Result<()> {
        verify(&self.staging, name, directory, Some(manifest)).map(|_| ())
    }

    pub(super) fn verify_target(
        &self,
        name: &str,
        directory: &FileIdentity,
        manifest: &FileIdentity,
    ) -> io::Result<()> {
        verify(&self.local, name, directory, Some(manifest)).map(|_| ())
    }

    pub(super) fn cleanup_stage(
        &self,
        name: &str,
        directory: &FileIdentity,
        manifest: &FileIdentity,
    ) -> io::Result<()> {
        self.cleanup_partial(name, directory, Some(manifest))
    }

    fn cleanup_partial(
        &self,
        name: &str,
        identity: &FileIdentity,
        manifest: Option<&FileIdentity>,
    ) -> io::Result<()> {
        let (directory, file) = verify(&self.staging, name, identity, manifest)?;
        if let Some(file) = file {
            directory.remove_manifest(&file)?;
            drop(file);
        }
        // Never recurse, and never remove a renamed/replaced directory entry.
        if !directory.entries(1)?.is_empty() {
            return Err(rejected());
        }
        self.staging.remove_stage(name, &directory)
    }

    pub(super) fn sync_after_promotion(&self) -> io::Result<()> {
        self.hooks
            .checkpoint(ImportFsStep::SyncDestinationDirectory)?;
        #[cfg(unix)]
        {
            // Attempt both even if syncing the destination fails.
            let local = self.local.sync();
            let staging = self.staging.sync();
            local.and(staging)?;
        }
        Ok(())
    }
}

fn verify(
    parent: &native::Directory,
    name: &str,
    identity: &FileIdentity,
    manifest: Option<&FileIdentity>,
) -> io::Result<(native::Directory, Option<File>)> {
    let directory = parent.open_stage(name)?;
    if directory.identity()? != *identity {
        return Err(rejected());
    }
    let names = directory.entries(2)?;
    let file = match manifest {
        Some(expected) if names.len() == 1 && names[0] == OsStr::new(MANIFEST) => {
            let file = directory.open_manifest()?;
            if native::manifest_identity(&file)? != *expected {
                return Err(rejected());
            }
            Some(file)
        }
        None if names.is_empty() => None,
        _ => return Err(rejected()),
    };
    Ok((directory, file))
}

fn rejected() -> io::Error {
    io::ErrorKind::InvalidInput.into()
}

#[cfg(unix)]
mod native {
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

    pub(super) struct Directory(File);

    impl Directory {
        pub(super) fn open_or_create_root(root: &Path) -> io::Result<Self> {
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

        pub(super) fn fixed_child(&self, name: &str) -> io::Result<Self> {
            self.fixed_child_os(OsStr::new(name))
        }

        pub(super) fn create_stage(&self, name: &str) -> io::Result<Self> {
            fs::mkdirat(&self.0, name, Mode::RWXU)?;
            // If the new directory cannot be opened and identified, leave it alone.
            self.open_stage(name)
        }

        pub(super) fn open_stage(&self, name: &str) -> io::Result<Self> {
            Ok(Self(File::from(fs::openat(
                &self.0,
                name,
                DIRECTORY_FLAGS,
                Mode::empty(),
            )?)))
        }

        pub(super) fn create_manifest(&self) -> io::Result<File> {
            Ok(File::from(fs::openat(
                &self.0,
                MANIFEST,
                FILE_FLAGS | OFlags::RDWR | OFlags::CREATE | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            )?))
        }

        pub(super) fn open_manifest(&self) -> io::Result<File> {
            Ok(File::from(fs::openat(
                &self.0,
                MANIFEST,
                FILE_FLAGS | OFlags::RDONLY,
                Mode::empty(),
            )?))
        }

        pub(super) fn identity(&self) -> io::Result<FileIdentity> {
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

        pub(super) fn entries(&self, limit: usize) -> io::Result<Vec<OsString>> {
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

        pub(super) fn remove_manifest(&self, file: &File) -> io::Result<()> {
            if manifest_identity(&self.open_manifest()?)? != manifest_identity(file)? {
                return Err(rejected());
            }
            Ok(fs::unlinkat(&self.0, MANIFEST, fs::AtFlags::empty())?)
        }

        pub(super) fn remove_stage(&self, name: &str, stage: &Self) -> io::Result<()> {
            if self.open_stage(name)?.identity()? != stage.identity()? {
                return Err(rejected());
            }
            Ok(fs::unlinkat(&self.0, name, fs::AtFlags::REMOVEDIR)?)
        }

        pub(super) fn sync(&self) -> io::Result<()> {
            Ok(fs::fsync(&self.0)?)
        }
    }

    pub(super) fn manifest_identity(file: &File) -> io::Result<FileIdentity> {
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
    pub(super) fn promote_exclusive(
        source: &Directory,
        stage: &str,
        destination: &Directory,
        target: &str,
    ) -> io::Result<()> {
        Ok(fs::renameat_with(
            &source.0,
            stage,
            &destination.0,
            target,
            fs::RenameFlags::NOREPLACE,
        )?)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn promote_exclusive(
        source: &Directory,
        stage: &str,
        destination: &Directory,
        target: &str,
    ) -> io::Result<()> {
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
    pub(super) fn promote_exclusive(
        _: &Directory,
        _: &str,
        _: &Directory,
        _: &str,
    ) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}

#[cfg(windows)]
mod native {
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
                GetFileInformationByHandleEx, MoveFileExW, SetFileInformationByHandle,
                BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
                FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_INFO, FILE_LIST_DIRECTORY,
                FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };

    const DIRECTORY_ACCESS: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

    pub(super) struct Directory {
        file: File,
        path: PathBuf,
        // Only a root owns its path's complete chain; child objects are held
        // with their parents by ImportDirectories or its synchronous methods.
        _ancestors: Vec<File>,
    }

    impl Directory {
        pub(super) fn open_or_create_root(root: &Path) -> io::Result<Self> {
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

        pub(super) fn fixed_child(&self, name: &str) -> io::Result<Self> {
            let file = open_fixed_child(&self.file, OsStr::new(name))?;
            directory_identity(&file)?;
            Ok(Self {
                file,
                path: self.path.join(name),
                _ancestors: Vec::new(),
            })
        }
        pub(super) fn create_stage(&self, name: &str) -> io::Result<Self> {
            self.child(name, FILE_CREATE, true)
        }
        pub(super) fn open_stage(&self, name: &str) -> io::Result<Self> {
            self.child(name, FILE_OPEN, true)
        }
        pub(super) fn create_manifest(&self) -> io::Result<File> {
            open_child(&self.file, OsStr::new(MANIFEST), false, FILE_CREATE, true)
        }
        pub(super) fn open_manifest(&self) -> io::Result<File> {
            open_child(&self.file, OsStr::new(MANIFEST), false, FILE_OPEN, true)
        }
        pub(super) fn identity(&self) -> io::Result<FileIdentity> {
            directory_identity(&self.file)
        }

        pub(super) fn entries(&self, limit: usize) -> io::Result<Vec<OsString>> {
            // Fixed ancestors deny delete sharing; the checked stage additionally
            // denies write sharing while enumerating this pinned path.
            fs::read_dir(&self.path)?
                .take(limit)
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect()
        }

        pub(super) fn remove_manifest(&self, file: &File) -> io::Result<()> {
            delete_handle(file)
        }
        pub(super) fn remove_stage(&self, _name: &str, stage: &Self) -> io::Result<()> {
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
        // Fixed directories must permit child-link mutations by MoveFileExW.
        // They remain pinned against rename/deletion. Owned stage/file handles
        // are stricter, and all of them close before the move.
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

    pub(super) fn manifest_identity(file: &File) -> io::Result<FileIdentity> {
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

    fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
        let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
        if units.contains(&0) {
            return Err(rejected());
        }
        units.push(0);
        Ok(units)
    }

    pub(super) fn promote_exclusive(
        source: &Directory,
        stage: &str,
        destination: &Directory,
        target: &str,
    ) -> io::Result<()> {
        let source = wide_path(&source.path.join(stage))?;
        let destination = wide_path(&destination.path.join(target))?;
        // SAFETY: these backend-generated paths are below held fixed ancestors;
        // buffers are NUL-terminated and live through the call. All stage/file
        // no-delete handles were released by verification before reaching here.
        // Zero flags means neither replacement nor cross-volume copy is allowed.
        // This does not defend against a continuously malicious same-user account.
        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
