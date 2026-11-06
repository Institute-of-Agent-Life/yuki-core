//! Hold an unnamed local file while the HTTP body arrives. Nothing is registered
//! before EOF, digest verification and the final writer authorization.
use crate::Result;
use axum::body::Body;
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    ptr,
};

fn staging() -> Result<File> {
    let name = std::env::temp_dir().join(format!(
        "yuki-upload-{}.part",
        super::credentials::random::<24>()?
    ));
    let mut o = OpenOptions::new();
    o.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_DELETE_ON_CLOSE;
        o.share_mode(0).custom_flags(FILE_FLAG_DELETE_ON_CLOSE);
    }
    let file = o.open(&name)?;
    #[cfg(unix)]
    std::fs::remove_file(name)?;
    Ok(file)
}

pub(super) async fn receive(body: Body, size: i64, digest: &str) -> Result<File> {
    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::File::from_std(staging()?);
    let mut bytes = 0i64;
    let mut hash = Sha256::new();
    let mut body = body.into_data_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        bytes = bytes
            .checked_add(i64::try_from(chunk.len())?)
            .ok_or("validation_error")?;
        if bytes > size {
            return Err("validation_error".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk).await?;
    }
    if bytes != size || format!("{:x}", hash.finalize()) != digest {
        return Err("validation_error".into());
    }
    file.flush().await?;
    Ok(file.into_std().await)
}

/// Incremental SQLite blob I/O keeps the working set bounded by a single chunk.
/// The caller owns the writer transaction or a read snapshot for this handle.
/// The blob is always closed before the SQLx handle lock is released.
pub(super) enum Transfer<'a> {
    Write(&'a File),
    Read(&'a File),
    Verify,
}
pub(super) async fn copy(
    db: &mut sqlx::SqliteConnection,
    row: i64,
    size: i64,
    op: Transfer<'_>,
) -> Result<String> {
    let mut conn = db.lock_handle().await?;
    tokio::task::block_in_place(|| transfer(conn.as_raw_handle().as_ptr(), row, size, op))
}
fn transfer(
    raw: *mut libsqlite3_sys::sqlite3,
    row: i64,
    size: i64,
    op: Transfer<'_>,
) -> Result<String> {
    use libsqlite3_sys as ffi;
    let write = matches!(op, Transfer::Write(_));
    let mut blob = ptr::null_mut();
    // SAFETY: raw is exclusively held by LockedSqliteHandle for this entire synchronous scope.
    let rc = unsafe {
        ffi::sqlite3_blob_open(
            raw,
            c"main".as_ptr(),
            c"media_objects".as_ptr(),
            c"bytes".as_ptr(),
            row,
            i32::from(write),
            &mut blob,
        )
    };
    if rc != ffi::SQLITE_OK {
        return Err(format!("media blob open failed ({rc})").into());
    }
    struct Blob(*mut ffi::sqlite3_blob);
    impl Drop for Blob {
        fn drop(&mut self) {
            unsafe {
                ffi::sqlite3_blob_close(self.0);
            }
        }
    }
    let handle = Blob(blob);
    let length = unsafe { ffi::sqlite3_blob_bytes(handle.0) };
    if i64::from(length) != size {
        return Err("original_content_unavailable".into());
    }
    let mut file = match op {
        Transfer::Read(file) | Transfer::Write(file) => {
            let mut file = file.try_clone()?;
            file.seek(SeekFrom::Start(0))?;
            Some(file)
        }
        Transfer::Verify => None,
    };
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut offset = 0;
    while offset < length {
        let count = (length - offset).min(buf.len() as i32);
        let buf = &mut buf[..count as usize];
        let rc = if write {
            file.as_mut()
                .ok_or("missing upload file")?
                .read_exact(buf)?;
            unsafe { ffi::sqlite3_blob_write(handle.0, buf.as_ptr().cast(), count, offset) }
        } else {
            unsafe { ffi::sqlite3_blob_read(handle.0, buf.as_mut_ptr().cast(), count, offset) }
        };
        if rc != ffi::SQLITE_OK {
            return Err(format!("media blob transfer failed ({rc})").into());
        }
        if !write && let Some(file) = file.as_mut() {
            use std::io::Write;
            file.write_all(buf)?;
        }
        hash.update(&*buf);
        offset += count;
    }
    let rc = unsafe { ffi::sqlite3_blob_close(handle.0) };
    std::mem::forget(handle);
    if rc != ffi::SQLITE_OK {
        return Err(format!("media blob close failed ({rc})").into());
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(super) fn download_stage() -> Result<File> {
    staging()
}
