use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    ffi::OsString,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecycleItem {
    pub id: Vec<u16>,
    pub name: Vec<u16>,
    pub original_parent: PathBuf,
    pub time_deleted: i64,
}

impl RecycleItem {
    pub fn original_path(&self) -> PathBuf {
        self.original_parent.join(OsString::from_wide(&self.name))
    }
}

pub trait RecycleBin: Send + Sync {
    fn list(&self) -> Result<Vec<RecycleItem>, RecycleError>;
    fn delete(&self, path: &Path) -> Result<(), RecycleError>;
    fn restore(&self, item: RecycleItem) -> Result<(), RecycleError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecycleError {
    Failed,
    Ambiguous,
    Missing,
    Collision,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsRecycleBin;

impl RecycleBin for WindowsRecycleBin {
    fn list(&self) -> Result<Vec<RecycleItem>, RecycleError> {
        trash::os_limited::list()
            .map_err(|_| RecycleError::Failed)?
            .into_iter()
            .map(|item| {
                Ok(RecycleItem {
                    id: item.id.encode_wide().collect(),
                    name: item.name.encode_wide().collect(),
                    original_parent: item.original_parent,
                    time_deleted: item.time_deleted,
                })
            })
            .collect()
    }

    fn delete(&self, path: &Path) -> Result<(), RecycleError> {
        trash::delete(path).map_err(|_| RecycleError::Failed)
    }

    fn restore(&self, item: RecycleItem) -> Result<(), RecycleError> {
        trash::os_limited::restore_all([trash::TrashItem {
            id: OsString::from_wide(&item.id),
            name: OsString::from_wide(&item.name),
            original_parent: item.original_parent,
            time_deleted: item.time_deleted,
        }])
        .map_err(|_| RecycleError::Failed)
    }
}

pub fn recycle_exact(
    recycle_bin: &dyn RecycleBin,
    path: &Path,
) -> Result<RecycleItem, RecycleError> {
    let before: HashSet<Vec<u16>> = recycle_bin
        .list()?
        .into_iter()
        .map(|item| item.id)
        .collect();
    recycle_bin.delete(path)?;
    for attempt in 0..20 {
        let created: Vec<_> = recycle_bin
            .list()?
            .into_iter()
            .filter(|item| {
                !before.contains(&item.id) && equivalent_path(&item.original_path(), path)
            })
            .collect();
        match created.as_slice() {
            [item] => return Ok(item.clone()),
            [] if attempt < 19 => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            [] => return Err(RecycleError::Missing),
            _ => return Err(RecycleError::Ambiguous),
        }
    }
    Err(RecycleError::Missing)
}

fn equivalent_path(left: &Path, right: &Path) -> bool {
    fn key(path: &Path) -> String {
        let value = path.to_string_lossy().replace('\\', "/").to_lowercase();
        if let Some(path) = value.strip_prefix("//?/unc/") {
            format!("//{path}")
        } else {
            value.strip_prefix("//?/").unwrap_or(&value).to_owned()
        }
    }
    key(left) == key(right)
}

pub fn restore_exact(
    recycle_bin: &dyn RecycleBin,
    expected: &RecycleItem,
) -> Result<(), RecycleError> {
    if expected.original_path().exists() {
        return Err(RecycleError::Collision);
    }
    let mut matching = recycle_bin.list()?.into_iter().filter(|item| {
        item.id == expected.id && equivalent_path(&item.original_path(), &expected.original_path())
    });
    let item = matching.next().ok_or(RecycleError::Missing)?;
    if matching.next().is_some() {
        return Err(RecycleError::Ambiguous);
    }
    recycle_bin.restore(item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeRecycleBin {
        items: Mutex<Vec<RecycleItem>>,
        restored: Mutex<Vec<Vec<u16>>>,
    }

    impl RecycleBin for FakeRecycleBin {
        fn list(&self) -> Result<Vec<RecycleItem>, RecycleError> {
            Ok(self.items.lock().unwrap().clone())
        }
        fn delete(&self, path: &Path) -> Result<(), RecycleError> {
            self.items.lock().unwrap().push(RecycleItem {
                id: vec![9],
                name: path.file_name().unwrap().encode_wide().collect(),
                original_parent: path.parent().unwrap().to_path_buf(),
                time_deleted: 7,
            });
            Ok(())
        }
        fn restore(&self, item: RecycleItem) -> Result<(), RecycleError> {
            self.restored.lock().unwrap().push(item.id);
            Ok(())
        }
    }

    #[test]
    fn extended_length_and_display_paths_match_without_weakening_components() {
        assert!(equivalent_path(
            Path::new(r"\\?\C:\Users\person\cache"),
            Path::new(r"c:\users\person\cache"),
        ));
        assert!(!equivalent_path(
            Path::new(r"\\?\C:\Users\person\cache"),
            Path::new(r"c:\users\person\cache-neighbor"),
        ));
    }

    #[test]
    fn captures_and_restores_only_the_exact_new_item() {
        let bin = FakeRecycleBin::default();
        bin.items.lock().unwrap().push(RecycleItem {
            id: vec![1],
            name: "cache".encode_utf16().collect(),
            original_parent: PathBuf::from(r"C:\other"),
            time_deleted: 1,
        });
        let path = PathBuf::from(r"C:\work\cache");
        let item = recycle_exact(&bin, &path).unwrap();
        assert_eq!(item.id, vec![9]);
        restore_exact(&bin, &item).unwrap();
        assert_eq!(*bin.restored.lock().unwrap(), vec![vec![9]]);
    }

    #[test]
    fn refuses_missing_or_same_name_neighbor_items() {
        let bin = FakeRecycleBin::default();
        let expected = RecycleItem {
            id: vec![8],
            name: "cache".encode_utf16().collect(),
            original_parent: PathBuf::from(r"C:\work"),
            time_deleted: 1,
        };
        bin.items.lock().unwrap().push(RecycleItem {
            id: vec![7],
            ..expected.clone()
        });
        assert_eq!(restore_exact(&bin, &expected), Err(RecycleError::Missing));
    }
}
