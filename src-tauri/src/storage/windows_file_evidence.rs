//! Bounded evidence queried only from live Windows handles.
use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
};
use windows_sys::Win32::{
    Storage::FileSystem::{FileNameInfo, GetFileInformationByHandleEx},
    System::IO::IO_STATUS_BLOCK,
};

fn rejected() -> io::Error {
    io::ErrorKind::InvalidInput.into()
}

pub(crate) fn verify_opened_name(file: &File, expected: &OsStr) -> io::Result<()> {
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
    let units =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().add(1).cast::<u16>(), byte_len / 2) };
    let actual = units
        .rsplit(|unit| *unit == u16::from(b'\\'))
        .next()
        .ok_or_else(rejected)?;
    if actual != expected.encode_wide().collect::<Vec<_>>() {
        return Err(rejected());
    }
    Ok(())
}
/// A named stream is extra user data even though directory enumeration does
/// not show it. Query only the held object; never open or read a stream path.
pub(crate) fn no_named_streams(file: &File) -> Result<(), ()> {
    use windows_sys::Wdk::Storage::FileSystem::{FileStreamInformation, NtQueryInformationFile};
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
    let bytes =
        unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), status.Information) };
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
