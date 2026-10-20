use super::*;
use std::{fs, os::windows::io::AsRawHandle, ptr::null};
use windows_sys::Win32::{Foundation::FILETIME, Storage::FileSystem::SetFileTime};

fn fixed_modified(path: &Path) {
    let file = OpenOptions::new().write(true).open(path).unwrap();
    let time = FILETIME {
        dwLowDateTime: 1_234_567_890,
        dwHighDateTime: 30_000_000,
    };
    assert_ne!(
        unsafe { SetFileTime(file.as_raw_handle(), null(), null(), &time) },
        0
    );
}

#[test]
fn identical_replacement_with_preserved_length_and_time_changes_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("mise.toml");
    fs::write(&path, b"[tools]\nnode = '22'\n").unwrap();
    fixed_modified(&path);
    let before = configuration(&path).unwrap();
    assert_eq!(before, configuration(&path).unwrap());
    // 保留旧文件，确保测试不依赖文件 ID 回收时机。
    fs::rename(&path, directory.path().join("retained.toml")).unwrap();
    fs::write(&path, b"[tools]\nnode = '22'\n").unwrap();
    fixed_modified(&path);
    let after = configuration(&path).unwrap();
    assert_eq!(before.0, after.0);
    let before = before.1.unwrap();
    let after = after.1.unwrap();
    assert_eq!(before.length, after.length);
    assert_eq!(before.modified, after.modified);
    assert_ne!(before.identity, after.identity);
    assert_ne!(before, after);
}

#[test]
fn hard_links_share_handle_identity_without_merging_distinct_locators() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("runtime.exe");
    let linked = directory.path().join("linked.exe");
    fs::write(&original, b"same physical file").unwrap();
    fs::hard_link(&original, &linked).unwrap();
    let original_stamp = file_stamp(&original).unwrap();
    let linked_stamp = file_stamp(&linked).unwrap();
    assert_eq!(original_stamp.identity, linked_stamp.identity);
    assert_ne!(original_stamp.canonical, linked_stamp.canonical);
}

#[test]
fn directories_and_device_names_never_supply_configuration_or_executable_evidence() {
    let directory = tempfile::tempdir().unwrap();
    assert!(file_stamp(directory.path()).is_err());
    assert!(configuration(directory.path()).is_err());
    for path in [
        r"\\.\pipe\zenv-never-connect",
        r"\\?\GLOBALROOT\Device\NamedPipe\zenv-never-connect",
        r"\\server\pipe\zenv-never-connect",
        r"C:\NUL",
        r"C:\some-directory\con.txt",
        r"C:\some-directory\COM¹.log",
        r"C:\some-directory\mise.toml:alternate-stream",
    ] {
        let path = Path::new(path);
        assert!(file_stamp(path).is_err(), "{path:?}");
        assert!(configuration(path).is_err(), "{path:?}");
    }
}

#[test]
fn normal_and_verbatim_disk_and_unc_paths_are_not_rejected_by_namespace_validation() {
    for path in [
        r"C:\Users\developer\mise.toml",
        r"\\?\C:\Users\developer\mise.toml",
        r"\\server\share\project\mise.toml",
        r"\\?\UNC\server\share\project\mise.toml",
    ] {
        assert!(disk_path(Path::new(path)).is_ok(), "{path}");
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("missing").join("mise.toml");
    let (fingerprint, stamp) = configuration(&path).unwrap();
    assert!(!fingerprint.exists);
    assert!(stamp.is_none());
    assert!(fingerprint
        .canonical_path
        .unwrap()
        .ends_with(r"missing\mise.toml"));
}

#[test]
#[ignore = "requires Windows symlink privileges; not part of unprivileged acceptance"]
fn resolved_alias_remains_usable_but_raw_reparse_leaf_is_not_opened() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("runtime.exe");
    let alias = directory.path().join("alias.exe");
    fs::write(&original, b"original runtime").unwrap();
    std::os::windows::fs::symlink_file(&original, &alias).unwrap();
    assert_eq!(file_stamp(&original).unwrap(), file_stamp(&alias).unwrap());
    assert!(opened(&alias).is_err());
    assert_eq!(
        configuration(&original).unwrap().1,
        configuration(&alias).unwrap().1
    );
    fs::remove_file(&original).unwrap();
    assert!(configuration(&alias).is_err());
}
