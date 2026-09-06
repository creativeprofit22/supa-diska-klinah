use std::{env, fs, process::Command, thread, time::Duration};

fn main() {
    let mut args = env::args_os().skip(1);
    match args.next().and_then(|value| value.into_string().ok()).as_deref() {
        Some("parent") | None => {
            // No arguments: Cargo can compile and run this same fixture as build.rs.
            let ready = args.next().map(std::path::PathBuf::from)
                .unwrap_or_else(|| env::current_dir().unwrap().join("descendant.pid"));
            let sentinel = args.next().map(std::path::PathBuf::from)
                .unwrap_or_else(|| env::current_dir().unwrap().join("descendant.sentinel"));
            fs::write(ready.with_extension("parent.pid"), std::process::id().to_string())
                .expect("write parent pid");
            let mut descendant = Command::new(env::current_exe().expect("fixture executable"))
                .arg("descendant")
                .arg(&ready)
                .arg(sentinel)
                .spawn()
                .expect("spawn descendant");
            descendant.wait().expect("wait for descendant");
        }
        Some("descendant") => {
            let ready = std::path::PathBuf::from(args.next().expect("ready path"));
            let sentinel = args.next().expect("sentinel path");
            let pending = ready.with_extension("pending");
            fs::write(&pending, std::process::id().to_string()).expect("write descendant pid");
            fs::rename(pending, ready).expect("publish descendant readiness");
            thread::sleep(Duration::from_millis(1_500));
            fs::write(sentinel, b"descendant survived cancellation")
                .expect("write sentinel");
            thread::sleep(Duration::from_secs(30));
        }
        _ => panic!("expected parent or descendant mode"),
    }
}
