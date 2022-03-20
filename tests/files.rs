use std::{io::Write, path::Path};

use playspace::{Playspace, WriteError};

#[test]
fn write_files() {
    let space = Playspace::new().expect("Failed to create playspace");

    let file1_path = Path::new("file1.txt");
    assert!(!file1_path.exists());
    space
        .write_file(file1_path, "some file1 contents")
        .expect("Failed to write file1");
    let file1_contents = std::fs::read_to_string(file1_path).expect("Failed to read file1");
    assert_eq!(file1_contents, "some file1 contents");
    let file1_canonical = file1_path
        .canonicalize()
        .expect("Failed to canonicalize file1");

    let file2_path = Path::new("file2.log");
    assert!(!file2_path.exists());
    let mut file2 = space
        .create_file(file2_path)
        .expect("Failed to create file2");
    file2
        .write_all("a file2 log message".as_bytes())
        .expect("Failed ot write to file2");
    drop(file2);
    let file2_contents = std::fs::read_to_string(file2_path).expect("Failed to read file2");
    assert_eq!(file2_contents, "a file2 log message");
    let file2_canonical = file2_path
        .canonicalize()
        .expect("Failed to canonicalize file2");

    drop(space);

    assert!(file1_canonical.is_absolute());
    assert!(!file1_canonical.exists());
    assert!(file2_canonical.is_absolute());
    assert!(!file2_canonical.exists());
}

#[test]
fn good_absolute_file() {
    let space = Playspace::new().expect("Failed to create playspace");

    let mut path = std::env::current_dir().unwrap().canonicalize().unwrap();
    path.push("a_file.txt");
    assert!(path.is_absolute());

    space.write_file(&path, "some file contents").unwrap();
    let file_contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(file_contents, "some file contents");

    drop(space);

    assert!(!path.exists());
}

#[test]
fn bad_absolute_file() {
    let space = Playspace::new().expect("Failed to create playspace");

    let mut path = std::env::temp_dir();
    path.extend(["playspace", "some", "nonsense", "path.txt"]);
    assert!(!path.exists());

    #[allow(clippy::match_wild_err_arm)]
    match space.create_file(path) {
        Err(WriteError::OutsidePlayspace(_)) => (),
        Err(_) => panic!("Wrong error"),
        Ok(_) => panic!("Should not have worked"),
    }
}

#[test]
fn good_absolute_dir() {
    let space = Playspace::new().expect("Failed to create playspace");

    let mut path = std::env::current_dir().unwrap().canonicalize().unwrap();
    path.push("some/new/dirs");
    assert!(path.is_absolute());

    space.create_dir_all(&path).unwrap();
    space
        .write_file(path.join("a_file.txt"), "some file contents")
        .unwrap();
    std::env::set_current_dir("some/new").unwrap();
    let file_contents = std::fs::read_to_string("dirs/a_file.txt").unwrap();
    assert_eq!(file_contents, "some file contents");

    drop(space);

    assert!(!path.exists());
}

#[test]
fn bad_absolute_dir() {
    let space = Playspace::new().expect("Failed to create playspace");

    let path = Path::new("/tmp/playspace/some/nonsense/path");
    assert!(!path.exists());

    #[allow(clippy::match_wild_err_arm)]
    match space.create_dir_all(path) {
        Err(WriteError::OutsidePlayspace(_)) => (),
        Err(_) => panic!("Wrong error"),
        Ok(_) => panic!("Should not have worked"),
    }
}
