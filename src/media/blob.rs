//! Open one component at a time relative to an owned root descriptor. Reading
//! through that descriptor cannot switch to a replacement path between checks.
use crate::Result;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub struct Snapshot {
    pub data: Vec<u8>,
    pub digest: String,
}
impl Snapshot {
    pub fn new(data: Vec<u8>) -> Result<Self> {
        if data.len() > MAX_BYTES {
            return Err("media exceeds single-object limit".into());
        }
        let digest = format!("{:x}", Sha256::digest(&data));
        Ok(Self { data, digest })
    }
}
pub struct Root(File);
impl Root {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self(platform::root(path)?))
    }
    pub fn writable(&self) -> Result<bool> {
        let permissions = self.0.metadata()?.permissions();
        #[cfg(unix)]
        {
            use rustix::fs::{Access, AtFlags, StatVfsMountFlags, accessat, fstatvfs};
            use std::os::unix::fs::PermissionsExt;
            if permissions.mode() & 0o222 == 0
                || fstatvfs(&self.0)?
                    .f_flag
                    .contains(StatVfsMountFlags::RDONLY)
            {
                return Ok(false);
            }
            match accessat(&self.0, ".", Access::WRITE_OK, AtFlags::EACCESS) {
                Ok(()) => Ok(true),
                Err(rustix::io::Errno::ACCESS | rustix::io::Errno::ROFS) => Ok(false),
                Err(error) => Err(error.into()),
            }
        }
        #[cfg(not(unix))]
        Ok(!permissions.readonly())
    }
    pub fn snapshot(&self, relative: &str) -> Result<Snapshot> {
        Self::read_snapshot(self.file(relative)?)
    }
    pub fn modified(&self, relative: &str) -> Result<std::time::SystemTime> {
        Ok(self.file(relative)?.metadata()?.modified()?)
    }
    pub fn snapshot_optional(&self, relative: &str) -> Result<Option<Snapshot>> {
        self.optional(relative, false)
    }
    /// Apply the v3 secret file's POSIX mode check to the opened descriptor.
    pub fn private_snapshot_optional(&self, relative: &str) -> Result<Option<Snapshot>> {
        self.optional(relative, true)
    }
    fn optional(&self, relative: &str, private: bool) -> Result<Option<Snapshot>> {
        let file = match self.file(relative) {
            Ok(file) => file,
            Err(error) if platform::missing(error.as_ref()) => return Ok(None),
            Err(error) => return Err(error),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if private && file.metadata()?.permissions().mode() & 0o077 != 0 {
                return Err("secret file permissions are not private".into());
            }
        }
        #[cfg(not(unix))]
        let _ = private;
        Ok(Some(Self::read_snapshot(file)?))
    }
    fn read_snapshot(mut file: File) -> Result<Snapshot> {
        let before = file.metadata()?;
        if !before.is_file() || before.len() > MAX_BYTES as u64 || platform::links(&file)? != 1 {
            return Err("single regular file within byte limit required".into());
        }
        let mut bytes = Vec::with_capacity(usize::try_from(before.len())?);
        (&mut file)
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        if before.len() != after.len()
            || after.len() != bytes.len() as u64
            || platform::changed(&before, &after)
        {
            return Err("source changed during snapshot".into());
        }
        Snapshot::new(bytes)
    }
    pub fn stage(&self, name: &str, bytes: &[u8]) -> Result<()> {
        use std::io::Write;
        if components(name)?.len() != 1 || bytes.len() > MAX_BYTES {
            return Err("invalid staging object".into());
        }
        let mut f = platform::create(&self.0, name)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    }
    pub fn unlink(&self, name: &str) -> Result<()> {
        if components(name)?.len() != 1 {
            return Err("invalid staging name".into());
        }
        platform::remove(&self.0, name)
    }
    pub fn replace(&self, staged: &str, destination: &str) -> Result<()> {
        if components(staged)?.len() != 1 || components(destination)?.len() != 1 {
            return Err("invalid replacement name".into());
        }
        platform::replace(&self.0, staged, destination)
    }
    fn file(&self, relative: &str) -> Result<File> {
        let names = components(relative)?;
        let mut directory = self.0.try_clone()?;
        for (n, name) in names.iter().enumerate() {
            directory = platform::child(&directory, name, n + 1 < names.len())?;
        }
        Ok(directory)
    }
}
fn components(relative: &str) -> Result<Vec<&str>> {
    let parts: Vec<_> = relative.split('/').collect();
    if relative.len() > 4096
        || parts.iter().any(|s| {
            s.is_empty()
                || s.len() > 255
                || *s == "."
                || *s == ".."
                || s.contains(['\\', ':', '\0'])
                || s.chars().any(char::is_control)
                || s.ends_with(['.', ' '])
        })
    {
        return Err("bounded relative path within selected root required".into());
    }
    Ok(parts)
}
#[cfg(unix)]
mod platform {
    use super::*;
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::os::unix::fs::MetadataExt;
    pub fn root(path: &Path) -> Result<File> {
        Ok(open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?
        .into())
    }
    pub fn child(parent: &File, name: &str, dir: bool) -> Result<File> {
        let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        Ok(openat(
            parent,
            name,
            if dir {
                flags | OFlags::DIRECTORY
            } else {
                flags
            },
            Mode::empty(),
        )?
        .into())
    }
    pub fn create(parent: &File, name: &str) -> Result<File> {
        Ok(openat(
            parent,
            name,
            OFlags::WRONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::CREATE | OFlags::EXCL,
            Mode::RUSR | Mode::WUSR,
        )?
        .into())
    }
    pub fn remove(parent: &File, name: &str) -> Result<()> {
        match rustix::fs::unlinkat(parent, name, rustix::fs::AtFlags::empty()) {
            Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn replace(parent: &File, staged: &str, destination: &str) -> Result<()> {
        rustix::fs::renameat(parent, staged, parent, destination)?;
        parent.sync_all()?;
        Ok(())
    }
    pub fn missing(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
        error.downcast_ref::<rustix::io::Errno>() == Some(&rustix::io::Errno::NOENT)
            || error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }
    pub fn links(f: &File) -> Result<u64> {
        Ok(f.metadata()?.nlink())
    }
    pub fn changed(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
        (a.ctime(), a.ctime_nsec(), a.mtime(), a.mtime_nsec())
            != (b.ctime(), b.ctime_nsec(), b.mtime(), b.mtime_nsec())
    }
}
#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        fs::OpenOptions,
        io,
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::{AsRawHandle, FromRawHandle},
        },
    };
    use windows_sys::{
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
                FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
            },
        },
        Win32::{
            Foundation::{HANDLE, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING},
            Storage::FileSystem::{
                BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT,
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
                FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_DELETE, FILE_SHARE_READ,
                FILE_SHARE_WRITE, FILE_TRAVERSE, GetFileInformationByHandle, SYNCHRONIZE,
            },
            System::IO::IO_STATUS_BLOCK,
        },
    };
    fn plain(f: &File, dir: bool) -> Result<()> {
        let m = f.metadata()?;
        if m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || m.is_dir() != dir {
            return Err("reparse point or incorrect file type".into());
        }
        Ok(())
    }
    pub fn root(path: &Path) -> Result<File> {
        let file = OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        plain(&file, true)?;
        Ok(file)
    }
    pub fn child(parent: &File, name: &str, dir: bool) -> Result<File> {
        relative(parent, name, dir, false, false)
    }
    pub fn create(parent: &File, name: &str) -> Result<File> {
        relative(parent, name, false, true, false)
    }
    pub fn remove(parent: &File, name: &str) -> Result<()> {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
        };
        let file = match relative(parent, name, false, false, true) {
            Ok(f) => f,
            Err(e)
                if e.downcast_ref::<io::Error>()
                    .is_some_and(|e| e.kind() == io::ErrorKind::NotFound) =>
            {
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let info = FILE_DISPOSITION_INFO { DeleteFile: true };
        let ok = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&info as *const FILE_DISPOSITION_INFO).cast(),
                size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }
    pub fn replace(parent: &File, staged: &str, destination: &str) -> Result<()> {
        use windows_sys::Wdk::Storage::FileSystem::{
            FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
        };
        let file = relative(parent, staged, false, false, true)?;
        let name: Vec<u16> = destination.encode_utf16().collect();
        let bytes = size_of::<FILE_RENAME_INFORMATION>() + name.len() * 2;
        let mut storage = vec![0u64; bytes.div_ceil(size_of::<u64>())];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        let mut status = IO_STATUS_BLOCK::default();
        // The native API resolves this single name against the already pinned root.
        // Both the aligned variable-length buffer and handles outlive the synchronous call.
        let code = unsafe {
            (*info).Anonymous.ReplaceIfExists = true;
            (*info).RootDirectory = parent.as_raw_handle();
            (*info).FileNameLength = u32::try_from(name.len() * 2)?;
            std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
            NtSetInformationFile(
                file.as_raw_handle(),
                &mut status,
                info.cast(),
                u32::try_from(bytes)?,
                FileRenameInformation,
            )
        };
        if code < 0 {
            return Err(
                io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(code) } as i32).into(),
            );
        }
        Ok(())
    }
    pub fn missing(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
        error
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
    }
    fn relative(parent: &File, name: &str, dir: bool, create: bool, delete: bool) -> Result<File> {
        use windows_sys::{
            Wdk::Storage::FileSystem::FILE_CREATE,
            Win32::Storage::FileSystem::{DELETE, FILE_WRITE_DATA},
        };
        let mut utf16: Vec<u16> = name.encode_utf16().collect();
        let bytes = u16::try_from(utf16.len() * 2)?;
        let object_name = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: utf16.as_mut_ptr(),
        };
        let attrs = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &object_name,
            Attributes: OBJ_CASE_INSENSITIVE,
            ..Default::default()
        };
        let mut status = IO_STATUS_BLOCK::default();
        let mut handle: HANDLE = std::ptr::null_mut();
        let access = (if create { FILE_WRITE_DATA } else { 0 })
            | (if delete { DELETE } else { 0 })
            | FILE_READ_ATTRIBUTES
            | SYNCHRONIZE
            | if dir {
                FILE_LIST_DIRECTORY | FILE_TRAVERSE
            } else {
                FILE_READ_DATA
            };
        let flags = FILE_OPEN_REPARSE_POINT
            | FILE_SYNCHRONOUS_IO_NONALERT
            | if dir {
                FILE_DIRECTORY_FILE
            } else {
                FILE_NON_DIRECTORY_FILE
            };
        // Attributes and their counted UTF-16 buffer live throughout this call.
        // NtCreateFile resolves the single component against the pinned parent handle.
        let code = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &attrs,
                &mut status,
                std::ptr::null(),
                0,
                if dir {
                    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
                } else {
                    FILE_SHARE_READ
                },
                if create { FILE_CREATE } else { FILE_OPEN },
                flags,
                std::ptr::null(),
                0,
            )
        };
        if code < 0 {
            return Err(
                io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(code) } as i32).into(),
            );
        }
        let file = unsafe { File::from_raw_handle(handle) };
        plain(&file, dir)?;
        Ok(file)
    }
    pub fn links(f: &File) -> Result<u64> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(u64::from(info.nNumberOfLinks))
    }
    pub fn changed(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
        a.last_write_time() != b.last_write_time()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_escaped_or_indirect_files_and_keeps_open_source() {
        let dir = std::env::temp_dir().join(format!(
            "yuki-file-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let inside = dir.join("inputs");
        std::fs::create_dir(&inside).unwrap();
        let outside = dir.join("outside");
        std::fs::write(&outside, b"outside").unwrap();
        std::fs::write(inside.join("file"), b"original").unwrap();
        let root = Root::open(&inside).unwrap();
        assert_eq!(root.snapshot("file").unwrap().data, b"original");
        std::fs::hard_link(&outside, inside.join("hard")).unwrap();
        assert!(root.snapshot("hard").is_err());
        for path in [
            "../outside",
            "/outside",
            "a/../file",
            "file:stream",
            "a\\file",
            "./file",
        ] {
            assert!(root.snapshot(path).is_err());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&dir, inside.join("escape")).unwrap();
            assert!(root.snapshot("escape/outside").is_err());
            let mut held = root.file("file").unwrap();
            std::fs::rename(inside.join("file"), inside.join("first")).unwrap();
            std::fs::write(inside.join("file"), b"replacement").unwrap();
            let mut bytes = Vec::new();
            held.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"original");
        }
        #[cfg(windows)]
        {
            let mut held = root.file("file").unwrap();
            assert!(std::fs::rename(inside.join("file"), inside.join("other")).is_err());
            assert!(std::fs::write(inside.join("file"), b"changed").is_err());
            let mut bytes = Vec::new();
            held.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"original");
        }
        drop(root);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
