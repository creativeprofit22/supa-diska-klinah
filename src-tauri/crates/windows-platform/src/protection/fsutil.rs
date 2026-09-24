//! Small native file helpers owned by the protection module: atomic
//! write-through replace, flushed exclusive creation, and reparse checks.

use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FsFault {
    Io,
    Invalid,
    TooLarge,
    Exists,
    Reparse,
}

pub(crate) fn wide(value: &OsStr) -> Result<Vec<u16>, FsFault> {
    let mut out: Vec<u16> = value.encode_wide().collect();
    if out.contains(&0) {
        return Err(FsFault::Invalid);
    }
    out.push(0);
    Ok(out)
}

pub(crate) fn random_hex() -> Result<String, FsFault> {
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| FsFault::Io)?;
    Ok(protection_core::to_hex(&nonce))
}

/// True when the path itself (not its target) is a reparse point.
pub(crate) fn is_reparse(path: &Path) -> Result<bool, FsFault> {
    let metadata = fs::symlink_metadata(path).map_err(|_| FsFault::Io)?;
    Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

/// Reject when any existing component from `root` down to `path` is a reparse point.
pub(crate) fn ensure_no_reparse_below(root: &Path, path: &Path) -> Result<(), FsFault> {
    let relative = path.strip_prefix(root).map_err(|_| FsFault::Invalid)?;
    let mut current = root.to_path_buf();
    if is_reparse(&current)? {
        return Err(FsFault::Reparse);
    }
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                return Err(FsFault::Reparse);
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    Ok(())
}

/// Create a new file (never overwriting), write and flush it.
pub(crate) fn write_new_flushed(path: &Path, bytes: &[u8]) -> Result<(), FsFault> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::AlreadyExists => FsFault::Exists,
            _ => FsFault::Io,
        })?;
    if file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(FsFault::Io);
    }
    Ok(())
}

pub(crate) fn move_replace(source: &Path, destination: &Path) -> Result<(), FsFault> {
    let source = wide(source.as_os_str())?;
    let destination = wide(destination.as_os_str())?;
    // SAFETY: both buffers are NUL-terminated UTF-16 paths that outlive the call.
    let ok = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 { Err(FsFault::Io) } else { Ok(()) }
}

/// Write a temporary sibling, flush it, then atomically replace `path`.
pub(crate) fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), FsFault> {
    let parent = path.parent().ok_or(FsFault::Invalid)?;
    let temporary = parent.join(format!(".tmp-{}", random_hex()?));
    write_new_flushed(&temporary, bytes)?;
    move_replace(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

/// Read a regular, non-reparse file up to `max` bytes.
pub(crate) fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, FsFault> {
    if is_reparse(path)? {
        return Err(FsFault::Reparse);
    }
    let file = File::open(path).map_err(|_| FsFault::Io)?;
    let metadata = file.metadata().map_err(|_| FsFault::Io)?;
    if !metadata.is_file() {
        return Err(FsFault::Invalid);
    }
    if metadata.len() > max {
        return Err(FsFault::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| FsFault::Io)?;
    if bytes.len() as u64 > max {
        return Err(FsFault::TooLarge);
    }
    Ok(bytes)
}

/// Remove a directory tree that this module created, refusing reparse points
/// anywhere inside it so a planted junction can never redirect deletion.
pub(crate) fn remove_owned_tree(path: &Path) -> Result<(), FsFault> {
    if is_reparse(path)? {
        // Remove only the link itself, never its target.
        return fs::remove_dir(path)
            .or_else(|_| fs::remove_file(path))
            .map_err(|_| FsFault::Io);
    }
    for entry in fs::read_dir(path).map_err(|_| FsFault::Io)? {
        let entry = entry.map_err(|_| FsFault::Io)?;
        let child: PathBuf = entry.path();
        let metadata = fs::symlink_metadata(&child).map_err(|_| FsFault::Io)?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            fs::remove_dir(&child)
                .or_else(|_| fs::remove_file(&child))
                .map_err(|_| FsFault::Io)?;
        } else if metadata.is_dir() {
            remove_owned_tree(&child)?;
        } else {
            fs::remove_file(&child).map_err(|_| FsFault::Io)?;
        }
    }
    fs::remove_dir(path).map_err(|_| FsFault::Io)
}

pub(crate) fn valid_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
