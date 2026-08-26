#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = supa_diska_klinah_lib::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
