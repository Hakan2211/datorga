//! Stand-in for `whisper-cli` in tests. Behaviour depends on the model file name:
//! `fail` exits 1, `slow` hangs (for cancel), anything else writes a JSON result.

use std::time::Duration;

fn arg(args: &[String], flag: &str) -> String {
    let i = args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("missing {flag}"));
    args[i + 1].clone()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let model = arg(&args, "-m");
    let wav = arg(&args, "-f");
    let out = arg(&args, "-of");
    assert_eq!(arg(&args, "-l"), "de");
    assert!(args.iter().any(|a| a == "-oj"));
    let bytes = std::fs::read(&wav).expect("wav readable");
    assert_eq!(&bytes[0..4], b"RIFF");

    if model.ends_with("fail") {
        eprintln!("whisper_init_from_file_with_params_no_state: failed to load model");
        std::process::exit(1);
    }
    for p in [0, 50] {
        eprintln!("whisper_print_progress_callback: progress = {p:3}%");
    }
    if model.ends_with("slow") {
        std::thread::sleep(Duration::from_secs(60));
    }
    let samples = (bytes.len() - 44) / 2;
    let end_ms = samples as i64 * 1000 / 16_000;
    let json = format!(
        r#"{{"transcription":[{{"offsets":{{"from":0,"to":{end_ms}}},"text":" Hallo aus dem Test."}}]}}"#
    );
    std::fs::write(format!("{out}.json"), json).unwrap();
}
