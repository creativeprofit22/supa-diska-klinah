//! Dedicated disposable vendor fixture: never reads or writes registry/application directories.
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4);
    let directory = std::path::PathBuf::from(&args[1]);
    assert!(
        directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("vendor-fixture-")
    );
    assert_eq!(
        std::fs::read(directory.join("fixture-only")).unwrap(),
        b"disposable"
    );
    std::fs::write(directory.join("started"), b"started").unwrap();
    let delay: u64 = args[2].parse().unwrap();
    assert!(delay <= 2000);
    std::thread::sleep(std::time::Duration::from_millis(delay));
    std::fs::write(directory.join("finished"), b"finished").unwrap();
    std::process::exit(args[3].parse().unwrap());
}
