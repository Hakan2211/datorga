// Prevents an additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Speaker separation runs in a child process of the app itself (see thera-diarize), so a
    // running diarization can be cancelled by killing it.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.len() == 3 && args[1] == "--diarize" {
        std::process::exit(thera_diarize::main(std::path::Path::new(&args[2])));
    }
    thera_app_lib::run()
}
