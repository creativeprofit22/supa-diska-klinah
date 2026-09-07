#![cfg(windows)]
use cleanup_core::{
    EntryKind, FileSystem,
    storage::{ObservedEntry, RootAuthorization},
};
use std::{fs, path::PathBuf, time::UNIX_EPOCH};
use windows_platform::cleanup::WindowsFileSystem;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("storage-races-{:032x}", getrandom::u64().unwrap()));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn evidence(&self, name: &str) -> (RootAuthorization, ObservedEntry) {
        let fs = WindowsFileSystem;
        let path = self.0.join(name);
        let meta = fs.metadata_no_follow(&path).unwrap();
        (
            RootAuthorization {
                snapshot_id: "a".repeat(32),
                root_id: "b".repeat(32),
                canonical_path: self.0.clone(),
                identity: fs.metadata_no_follow(&self.0).unwrap().identity.unwrap(),
            },
            ObservedEntry {
                canonical_path: path,
                identity: meta.identity.unwrap(),
                kind: meta.kind,
                logical_bytes: if meta.kind == EntryKind::Directory {
                    0
                } else {
                    meta.size
                },
                allocated_bytes: Some(0),
                modified_unix_nanos: meta
                    .modified
                    .unwrap()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64,
            },
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn late_child_survives_empty_only_disposition() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("empty")).unwrap();
    let (root, entry) = fixture.evidence("empty");
    let guard = WindowsFileSystem.guard_entry(&root, &entry, false).unwrap();
    fs::write(entry.canonical_path.join("late.txt"), b"survive").unwrap();
    assert!(guard.remove().is_err());
    assert_eq!(
        fs::read(entry.canonical_path.join("late.txt")).unwrap(),
        b"survive"
    );
}

#[test]
fn held_target_and_ancestor_cannot_be_replaced() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("file"), b"payload").unwrap();
    let (root, entry) = fixture.evidence("file");
    let guard = WindowsFileSystem.guard_entry(&root, &entry, false).unwrap();
    assert!(fs::rename(&entry.canonical_path, fixture.0.join("replacement")).is_err());
    assert!(fs::rename(&fixture.0, fixture.0.with_extension("moved")).is_err());
    assert!(fs::write(&entry.canonical_path, b"changed").is_err());
    guard.remove().unwrap();
    assert!(!entry.canonical_path.exists());
}

#[test]
fn stale_identity_rejects_replacement_and_keeper_denies_writers() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("file"), b"payload").unwrap();
    let (root, entry) = fixture.evidence("file");
    fs::rename(&entry.canonical_path, fixture.0.join("old")).unwrap();
    fs::write(&entry.canonical_path, b"payload").unwrap();
    assert!(WindowsFileSystem.guard_entry(&root, &entry, false).is_err());
    let (root, entry) = fixture.evidence("file");
    let writer = fs::OpenOptions::new()
        .write(true)
        .open(&entry.canonical_path)
        .unwrap();
    assert!(WindowsFileSystem.guard_entry(&root, &entry, true).is_err());
    drop(writer);
    let keeper = WindowsFileSystem.guard_entry(&root, &entry, true).unwrap();
    assert!(fs::write(&entry.canonical_path, b"changed").is_err());
    assert!(fs::remove_file(&entry.canonical_path).is_err());
    drop(keeper);
    assert_eq!(fs::read(entry.canonical_path).unwrap(), b"payload");
}

#[test]
fn in_place_ancestor_reparse_attempt_cannot_redirect_held_target_deletion() {
    use std::{
        ffi::c_void,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        ptr,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    // Test-only native declaration, matching installed windows-sys 0.61.2 System/IO.
    // OVERLAPPED is always null (synchronous handles); no production command surface.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn DeviceIoControl(
            handle: *mut c_void,
            code: u32,
            input: *const c_void,
            input_size: u32,
            output: *mut c_void,
            output_size: u32,
            returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }
    const GET_REPARSE_POINT: u32 = 589992;
    const SET_REPARSE_POINT: u32 = 589988;
    let fixture = Fixture::new();
    let outside = Fixture::new();
    let ancestor = fixture.0.join("ancestor");
    fs::create_dir(&ancestor).unwrap();
    fs::write(ancestor.join("file"), b"owned target").unwrap();
    fs::write(outside.0.join("file"), b"outside sentinel").unwrap();
    let outside_identity = WindowsFileSystem
        .metadata_no_follow(&outside.0.join("file"))
        .unwrap()
        .identity;
    // Obtain a valid mount-point buffer from the existing native junction helper.
    let template = fixture.0.join("template");
    junction::create(&outside.0, &template).unwrap();
    let flags = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT;
    let template_handle = fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(&template)
        .unwrap();
    let mut buffer = [0_u64; 2048];
    let mut length = 0;
    // SAFETY: live synchronous handle, aligned 16KiB output buffer, valid length pointer.
    assert_ne!(
        unsafe {
            DeviceIoControl(
                template_handle.as_raw_handle(),
                GET_REPARSE_POINT,
                ptr::null(),
                0,
                buffer.as_mut_ptr().cast(),
                std::mem::size_of_val(&buffer) as u32,
                &mut length,
                ptr::null_mut(),
            )
        },
        0
    );
    assert!(length > 8 && length as usize <= std::mem::size_of_val(&buffer));
    drop(template_handle);
    junction::delete(&template).unwrap();

    let (root, entry) = fixture.evidence("ancestor/file");
    let guard = WindowsFileSystem.guard_entry(&root, &entry, false).unwrap();
    // Open the EXISTING ancestor and set its reparse data in place: not a rename swap.
    let ancestor_handle = fs::OpenOptions::new()
        .write(true)
        .custom_flags(flags)
        .open(&ancestor)
        .unwrap();
    let mut returned = 0;
    // SAFETY: live synchronous handle and the exact initialized buffer returned above.
    let changed = unsafe {
        DeviceIoControl(
            ancestor_handle.as_raw_handle(),
            SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            length,
            ptr::null_mut(),
            0,
            &mut returned,
            ptr::null_mut(),
        )
    };
    let failure = if changed == 0 {
        Some(std::io::Error::last_os_error())
    } else {
        None
    };
    drop(ancestor_handle);
    if changed != 0 {
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&ancestor)
                .unwrap()
                .kind,
            EntryKind::LinkLike
        );
        assert_eq!(
            fs::read(ancestor.join("file")).unwrap(),
            b"outside sentinel"
        );
    } else {
        // NTFS may reject adding a mount point to this nonempty, guarded directory.
        assert!(failure.unwrap().raw_os_error().is_some());
        assert_eq!(
            WindowsFileSystem
                .metadata_no_follow(&ancestor)
                .unwrap()
                .kind,
            EntryKind::Directory
        );
    }
    guard.remove().unwrap();
    assert_eq!(
        fs::read(outside.0.join("file")).unwrap(),
        b"outside sentinel"
    );
    assert_eq!(
        WindowsFileSystem
            .metadata_no_follow(&outside.0.join("file"))
            .unwrap()
            .identity,
        outside_identity
    );
    if changed != 0 {
        junction::delete(&ancestor).unwrap();
    }
    assert!(!ancestor.join("file").exists());
}

#[test]
fn literal_empty_directory_is_removed_without_tree_walk() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("empty")).unwrap();
    let (root, entry) = fixture.evidence("empty");
    WindowsFileSystem
        .guard_entry(&root, &entry, false)
        .unwrap()
        .remove()
        .unwrap();
    assert!(!entry.canonical_path.exists());
    assert!(fixture.0.exists());
}
