use crate::plugin::ownership::FileIdentity;
pub(in crate::storage::safe_plugin_document) use native::{identity, Directory};
use std::{
    fs::File,
    io,
    path::{Component, Path},
};
fn rejected() -> io::Error {
    io::Error::other("unsafe plugin document object")
}

#[cfg(unix)]
mod native {
    use super::*;
    use rustix::fs::{self, Dir, Mode, OFlags};
    use std::{
        ffi::OsStr,
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
    pub(in crate::storage::safe_plugin_document) struct Directory {
        file: File,
        _ancestors: Vec<File>,
    }
    impl Directory {
        pub(in crate::storage::safe_plugin_document) fn open(
            root: &Path,
            create: bool,
        ) -> io::Result<Self> {
            if !root.is_absolute()
                || root
                    .as_os_str()
                    .as_bytes()
                    .split(|b| *b == b'/')
                    .any(|v| v == b"." || v == b"..")
            {
                return Err(rejected());
            }
            let mut file = File::from(fs::open("/", DIRECTORY_FLAGS, Mode::empty())?);
            let mut ancestors = Vec::new();
            for component in root.components() {
                match component {
                    Component::RootDir => (),
                    Component::Normal(name) => {
                        exact_case(&file, name)?;
                        if create {
                            match fs::mkdirat(&file, name, Mode::RWXU) {
                                Ok(()) => fs::fsync(&file)?,
                                Err(rustix::io::Errno::EXIST) => (),
                                Err(error) => return Err(error.into()),
                            }
                        }
                        let next =
                            File::from(fs::openat(&file, name, DIRECTORY_FLAGS, Mode::empty())?);
                        ancestors.push(file);
                        file = next;
                    }
                    _ => return Err(rejected()),
                }
            }
            Ok(Self {
                file,
                _ancestors: ancestors,
            })
        }
        pub(in crate::storage::safe_plugin_document) fn exact_case(
            &self,
            name: &str,
        ) -> io::Result<()> {
            exact_case(&self.file, OsStr::new(name))
        }
        pub(in crate::storage::safe_plugin_document) fn open_file(
            &self,
            name: &str,
            create: bool,
        ) -> io::Result<File> {
            self.exact_case(name)?;
            let flags = if create {
                FILE_FLAGS | OFlags::RDWR | OFlags::CREATE | OFlags::EXCL
            } else {
                FILE_FLAGS | OFlags::RDONLY
            };
            let file = File::from(fs::openat(
                &self.file,
                name,
                flags,
                Mode::RUSR | Mode::WUSR,
            )?);
            identity(&file)?;
            Ok(file)
        }
        pub(in crate::storage::safe_plugin_document) fn verify(
            &self,
            name: &str,
            file: &File,
        ) -> io::Result<()> {
            if identity(&self.open_file(name, false)?)? != identity(file)? {
                return Err(rejected());
            }
            Ok(())
        }
        pub(in crate::storage::safe_plugin_document) fn remove(
            &self,
            name: &str,
            file: &File,
        ) -> io::Result<()> {
            self.verify(name, file)?;
            // Name-based operation: single writer/no manual edits is mandatory.
            Ok(fs::unlinkat(&self.file, name, fs::AtFlags::empty())?)
        }
        pub(in crate::storage::safe_plugin_document) fn replace(
            &self,
            source: &str,
            file: &File,
            target: &str,
            previous: Option<&File>,
        ) -> io::Result<()> {
            self.verify(source, file)?;
            self.exact_case(target)?;
            if let Some(previous) = previous {
                self.verify(target, previous)?;
            } else {
                match self.open_file(target, false) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                    _ => return Err(rejected()),
                }
            }
            // Best-effort checks cannot close a malicious same-user POSIX race.
            Ok(fs::renameat(&self.file, source, &self.file, target)?)
        }
        pub(in crate::storage::safe_plugin_document) fn sync(&self) -> io::Result<()> {
            Ok(fs::fsync(&self.file)?)
        }
    }
    fn exact_case(parent: &File, name: &OsStr) -> io::Result<()> {
        for entry in Dir::read_from(parent)? {
            let entry = entry?;
            let actual = entry.file_name().to_bytes();
            if actual.eq_ignore_ascii_case(name.as_bytes()) && actual != name.as_bytes() {
                return Err(rejected());
            }
        }
        Ok(())
    }
    pub(in crate::storage::safe_plugin_document) fn identity(
        file: &File,
    ) -> io::Result<FileIdentity> {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(rejected());
        }
        Ok(FileIdentity {
            volume: metadata.dev(),
            object: u128::from(metadata.ino()),
        })
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::{
        ffi::OsStr,
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
                FileRenameInformationEx, NtCreateFile, NtSetInformationFile, FILE_CREATE,
                FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
                FILE_RENAME_INFORMATION, FILE_RENAME_POSIX_SEMANTICS,
                FILE_RENAME_REPLACE_IF_EXISTS, FILE_SYNCHRONOUS_IO_NONALERT,
            },
        },
        Win32::{
            Foundation::{
                RtlNtStatusToDosError, OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, UNICODE_STRING,
            },
            Storage::FileSystem::{
                FileDispositionInfo, FileIdInfo, FileNameInfo, GetFileInformationByHandle,
                GetFileInformationByHandleEx, SetFileInformationByHandle,
                BY_HANDLE_FILE_INFORMATION, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
                FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_INFO, FILE_LIST_DIRECTORY,
                FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
                SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };
    const DIRECTORY_ACCESS: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
    pub(in crate::storage::safe_plugin_document) struct Directory {
        file: File,
        path: PathBuf,
        _ancestors: Vec<File>,
    }
    impl Directory {
        pub(in crate::storage::safe_plugin_document) fn open(
            root: &Path,
            create: bool,
        ) -> io::Result<Self> {
            if root
                .as_os_str()
                .to_string_lossy()
                .split(['/', '\\'])
                .any(|v| v == "." || v == "..")
            {
                return Err(rejected());
            }
            let mut parts = root.components();
            let Some(Component::Prefix(prefix)) = parts.next() else {
                return Err(rejected());
            };
            if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
                || parts.next() != Some(Component::RootDir)
            {
                return Err(rejected());
            }
            let mut path = PathBuf::from(prefix.as_os_str());
            path.push(r"\");
            let mut file = OpenOptions::new()
                .access_mode(DIRECTORY_ACCESS)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)
                .map_err(|e| io::Error::new(e.kind(), format!("volume root open: {e}")))?;
            directory_identity(&file)?;
            let mut ancestors = Vec::new();
            for part in parts {
                let Component::Normal(name) = part else {
                    return Err(rejected());
                };
                exact_case(&path, name)
                    .map_err(|e| io::Error::new(e.kind(), format!("ancestor case: {e}")))?;
                let next = match open_child(&file, name, true, false) {
                    Err(e) if create && e.kind() == io::ErrorKind::NotFound => {
                        open_child(&file, name, true, true)?
                    }
                    other => other.map_err(|e| {
                        io::Error::new(e.kind(), format!("ancestor open {name:?}: {e}"))
                    })?,
                };
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
        pub(in crate::storage::safe_plugin_document) fn exact_case(
            &self,
            name: &str,
        ) -> io::Result<()> {
            exact_case(&self.path, OsStr::new(name))
        }
        pub(in crate::storage::safe_plugin_document) fn open_file(
            &self,
            name: &str,
            create: bool,
        ) -> io::Result<File> {
            self.exact_case(name)?;
            let file = open_child(&self.file, OsStr::new(name), false, create)?;
            identity(&file)?;
            Ok(file)
        }
        pub(in crate::storage::safe_plugin_document) fn verify(
            &self,
            name: &str,
            file: &File,
        ) -> io::Result<()> {
            if identity(&self.open_file(name, false)?)? != identity(file)? {
                return Err(rejected());
            }
            Ok(())
        }
        pub(in crate::storage::safe_plugin_document) fn remove(
            &self,
            name: &str,
            file: &File,
        ) -> io::Result<()> {
            self.verify(name, file)?;
            let info = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: the held verified file has DELETE access; no path is reopened for deletion.
            if unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileDispositionInfo,
                    (&info as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        pub(in crate::storage::safe_plugin_document) fn replace(
            &self,
            source: &str,
            file: &File,
            target: &str,
            previous: Option<&File>,
        ) -> io::Result<()> {
            self.verify(source, file)?;
            self.exact_case(target)?;
            if let Some(previous) = previous {
                self.verify(target, previous)?;
            } else {
                match self.open_file(target, false) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                    _ => return Err(rejected()),
                }
            }
            self.rename_held(file, target)
        }

        // Source name is intentionally not an argument: the kernel operation
        // must act on this held object even if its old name has been exchanged.
        fn rename_held(&self, file: &File, target: &str) -> io::Result<()> {
            let units: Vec<u16> = target.encode_utf16().collect();
            let size = std::mem::size_of::<FILE_RENAME_INFORMATION>()
                .checked_add(units.len() * 2)
                .ok_or_else(rejected)?;
            // usize buffer provides native struct alignment and space for the variable UTF-16 tail.
            let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
            let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
            // SAFETY: aligned, sufficiently sized buffer; parent/source handles and
            // UTF-16 data remain live. Replacement operates on the held SOURCE handle.
            unsafe {
                (*info).Anonymous.Flags =
                    FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS;
                (*info).RootDirectory = self.file.as_raw_handle();
                (*info).FileNameLength = (units.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    units.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    units.len(),
                );
                let mut status_block = IO_STATUS_BLOCK::default();
                // The Win32 wrapper rejects this relative contract on the
                // validated host. Native user-mode Nt preserves both handles;
                // there is deliberately no path-based fallback.
                let status = NtSetInformationFile(
                    file.as_raw_handle(),
                    &mut status_block,
                    info.cast(),
                    size as u32,
                    FileRenameInformationEx,
                );
                nt_result(status)?;
            }
            Ok(())
        }
        // Windows has no ordinary directory-fsync power-loss contract. New file
        // data was flushed before rename; only process-crash safety is reported.
        pub(in crate::storage::safe_plugin_document) fn sync(&self) -> io::Result<()> {
            Ok(())
        }
    }
    fn nt_result(status: i32) -> io::Result<()> {
        if status < 0 {
            // SAFETY: translates an NTSTATUS, does not access external memory.
            Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(status) } as i32,
            ))
        } else {
            Ok(())
        }
    }
    fn exact_case(path: &Path, name: &OsStr) -> io::Result<()> {
        // The complete directory chain is held without delete sharing.
        for entry in fs::read_dir(path)? {
            let actual = entry?.file_name();
            if actual
                .to_string_lossy()
                .eq_ignore_ascii_case(&name.to_string_lossy())
                && actual != name
            {
                return Err(rejected());
            }
        }
        Ok(())
    }
    fn open_child(parent: &File, name: &OsStr, directory: bool, create: bool) -> io::Result<File> {
        let mut units: Vec<u16> = name.encode_wide().collect();
        if units.is_empty()
            || name == "."
            || name == ".."
            || units.iter().any(|u| matches!(*u, 0 | 47 | 58 | 92))
        {
            return Err(rejected());
        }
        let length = u16::try_from(units.len() * 2).map_err(|_| rejected())?;
        let unicode = UNICODE_STRING {
            Length: length,
            MaximumLength: length,
            Buffer: units.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &unicode,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            ..Default::default()
        };
        let access = if directory {
            DIRECTORY_ACCESS
        } else {
            FILE_GENERIC_READ | DELETE | if create { FILE_GENERIC_WRITE } else { 0 }
        };
        let sharing =
            FILE_SHARE_READ | FILE_SHARE_WRITE | if directory { 0 } else { FILE_SHARE_DELETE };
        let mut handle = std::ptr::null_mut();
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: single-component name and live parent handle; synchronous call
        // returns exactly one owned handle. No truncate/overwrite disposition.
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attributes,
                &mut status_block,
                std::ptr::null(),
                0,
                sharing,
                if create { FILE_CREATE } else { FILE_OPEN },
                (if directory {
                    FILE_DIRECTORY_FILE
                } else {
                    FILE_NON_DIRECTORY_FILE
                }) | FILE_OPEN_REPARSE_POINT
                    | FILE_SYNCHRONOUS_IO_NONALERT,
                std::ptr::null(),
                0,
            )
        };
        if status < 0 {
            // SAFETY: translates the returned NTSTATUS.
            return Err(io::Error::from_raw_os_error(
                unsafe { RtlNtStatusToDosError(status) } as i32,
            ));
        }
        // SAFETY: successful NtCreateFile returned a new owned handle.
        let file = unsafe { File::from_raw_handle(handle) };
        verify_opened_name(&file, name)?;
        Ok(file)
    }
    fn verify_opened_name(file: &File, expected: &OsStr) -> io::Result<()> {
        // Query the held object rather than trusting a case-insensitive/8.3
        // namespace lookup. The fixed buffer bounds even unusual NT paths.
        let mut buffer = vec![0u32; 16385];
        let buffer_bytes = (buffer.len() * std::mem::size_of::<u32>()) as u32;
        // SAFETY: live handle; aligned writable buffer, sized in bytes.
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileNameInfo,
                buffer.as_mut_ptr().cast(),
                buffer_bytes,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let byte_len = buffer[0] as usize;
        if byte_len == 0 || byte_len & 1 != 0 || byte_len > buffer_bytes as usize - 4 {
            return Err(rejected());
        }
        // SAFETY: FileNameInfo is a u32 byte count followed by UTF-16 units;
        // the validated count stays inside the allocated and aligned buffer.
        let units = unsafe {
            std::slice::from_raw_parts(buffer.as_ptr().add(1).cast::<u16>(), byte_len / 2)
        };
        let actual = units
            .rsplit(|unit| *unit == u16::from(b'\\'))
            .next()
            .ok_or_else(rejected)?;
        if actual != expected.encode_wide().collect::<Vec<_>>() {
            return Err(rejected());
        }
        Ok(())
    }
    fn object_identity(file: &File) -> io::Result<FileIdentity> {
        let mut info = FILE_ID_INFO::default();
        // SAFETY: live handle and correctly sized writable output.
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
        let meta = file.metadata()?;
        if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(rejected());
        }
        object_identity(file)
    }
    pub(in crate::storage::safe_plugin_document) fn identity(
        file: &File,
    ) -> io::Result<FileIdentity> {
        let meta = file.metadata()?;
        if !meta.is_file() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(rejected());
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: live handle and native writable output structure.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.nNumberOfLinks != 1 {
            return Err(rejected());
        }
        object_identity(file)
    }

    #[cfg(test)]
    mod diagnostics {
        use super::*;
        use std::io::Write;
        use windows_sys::Wdk::Storage::FileSystem::{FileRenameInformation, NtSetInformationFile};
        use windows_sys::Win32::Storage::FileSystem::{FileRenameInfo, FILE_RENAME_INFO};

        #[test]
        fn opened_handle_name_must_match_the_exact_requested_component() {
            let base = std::env::current_dir().unwrap().join("target");
            fs::create_dir_all(&base).unwrap();
            let temp = tempfile::tempdir_in(base).unwrap();
            let parent = Directory::open(temp.path(), false).unwrap();
            let file = parent.open_file("state.json", true).unwrap();
            verify_opened_name(&file, OsStr::new("state.json")).unwrap();
            assert!(verify_opened_name(&file, OsStr::new("STATE.JSON")).is_err());
            assert!(verify_opened_name(&file, OsStr::new("another.json")).is_err());
        }

        #[test]
        fn native_rename_uses_held_source_after_final_source_name_swap() {
            let base = std::env::current_dir().unwrap().join("target");
            fs::create_dir_all(&base).unwrap();
            let temp = tempfile::tempdir_in(base).unwrap();
            let parent = Directory::open(temp.path(), false).unwrap();
            let mut source = parent.open_file("source", true).unwrap();
            source.write_all(b"held source").unwrap();
            source.sync_all().unwrap();
            fs::rename(temp.path().join("source"), temp.path().join("parked")).unwrap();
            fs::write(temp.path().join("source"), b"replacement").unwrap();
            parent.rename_held(&source, "destination").unwrap();
            parent.verify("destination", &source).unwrap();
            assert_eq!(
                fs::read(temp.path().join("source")).unwrap(),
                b"replacement"
            );
            assert_eq!(
                fs::read(temp.path().join("destination")).unwrap(),
                b"held source"
            );
            assert!(!temp.path().join("parked").exists());
        }

        #[test]
        fn ntstatus_errors_are_mapped_and_never_report_success() {
            for (status, code) in [
                (0xc000000du32 as i32, 87),
                (0xc0000022u32 as i32, 5),
                (0xc00000bbu32 as i32, 50),
            ] {
                assert_eq!(nt_result(status).unwrap_err().raw_os_error(), Some(code));
            }
            nt_result(0).unwrap();
        }

        #[test]
        fn native_extended_replace_preserves_held_target_and_source_handles() {
            use windows_sys::Wdk::Storage::FileSystem::{
                FileRenameInformationEx, FILE_RENAME_POSIX_SEMANTICS, FILE_RENAME_REPLACE_IF_EXISTS,
            };
            let base = std::env::current_dir().unwrap().join("target");
            fs::create_dir_all(&base).unwrap();
            let temp = tempfile::tempdir_in(base).unwrap();
            let parent = Directory::open(temp.path(), false).unwrap();
            let mut source = parent.open_file("source", true).unwrap();
            source.write_all(b"next").unwrap();
            source.sync_all().unwrap();
            let mut target = parent.open_file("destination", true).unwrap();
            target.write_all(b"old").unwrap();
            target.sync_all().unwrap();
            let units: Vec<u16> = "destination".encode_utf16().collect();
            let size = std::mem::size_of::<FILE_RENAME_INFORMATION>() + units.len() * 2;
            let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
            let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
            // SAFETY: disposable verified live handles, aligned variable-size buffer.
            unsafe {
                (*info).Anonymous.Flags =
                    FILE_RENAME_REPLACE_IF_EXISTS | FILE_RENAME_POSIX_SEMANTICS;
                (*info).RootDirectory = parent.file.as_raw_handle();
                (*info).FileNameLength = (units.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    units.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    units.len(),
                );
                let mut status_block = IO_STATUS_BLOCK::default();
                let status = NtSetInformationFile(
                    source.as_raw_handle(),
                    &mut status_block,
                    info.cast(),
                    size as u32,
                    FileRenameInformationEx,
                );
                assert!(
                    status >= 0,
                    "native extended replacement failed: {status:#x}"
                );
            }
            parent.verify("destination", &source).unwrap();
            assert_eq!(fs::read(temp.path().join("destination")).unwrap(), b"next");
            assert_eq!(target.metadata().unwrap().len(), 3);
        }

        #[test]
        fn isolate_windows_relative_rename_api_contract() {
            let base = std::env::current_dir().unwrap().join("target");
            fs::create_dir_all(&base).unwrap();
            let temp = tempfile::tempdir_in(base).unwrap();
            let parent = Directory::open(temp.path(), false).unwrap();
            let mut source = parent.open_file("source", true).unwrap();
            source.write_all(b"payload").unwrap();
            source.sync_all().unwrap();
            let units: Vec<u16> = "destination".encode_utf16().collect();
            let size = std::mem::size_of::<FILE_RENAME_INFO>() + (units.len() + 1) * 2;
            eprintln!(
                "offset={}, sizeof={}, passed size={size}",
                std::mem::offset_of!(FILE_RENAME_INFO, FileName),
                std::mem::size_of::<FILE_RENAME_INFO>()
            );
            let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
            let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // SAFETY: same held disposable handles, aligned buffer, valid UTF-16.
            unsafe {
                (*info).Anonymous.ReplaceIfExists = true;
                (*info).RootDirectory = parent.file.as_raw_handle();
                (*info).FileNameLength = (units.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    units.as_ptr(),
                    (*info).FileName.as_mut_ptr(),
                    units.len(),
                );
                let win32 = SetFileInformationByHandle(
                    source.as_raw_handle(),
                    FileRenameInfo,
                    info.cast(),
                    size as u32,
                );
                let win32_error = io::Error::last_os_error();
                eprintln!("Win32 relative result={win32}, error={win32_error}");
                if win32 == 0 {
                    let mut status_block = IO_STATUS_BLOCK::default();
                    let status = NtSetInformationFile(
                        source.as_raw_handle(),
                        &mut status_block,
                        info.cast(),
                        size as u32,
                        FileRenameInformation,
                    );
                    eprintln!("Native identical-buffer relative NTSTATUS={status:#x}");
                    assert!(status >= 0, "native failed: {status:#x}");
                }
            }
            parent.verify("destination", &source).unwrap();
            assert_eq!(
                fs::read(temp.path().join("destination")).unwrap(),
                b"payload"
            );
        }
    }
}
