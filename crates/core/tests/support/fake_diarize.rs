//! Stand-in for the app's `--diarize` process in tests. Behaviour depends on the request's
//! library folder name: `fail` exits 1, `slow` hangs (for cancel), anything else answers: in
//! diarize mode the requested speakers taking turns every second, in embed mode one print.

use std::time::Duration;

use thera_diarize::{Mode, Output, Print, Request, Turn};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    assert_eq!(args[1], "--diarize");
    let req: Request = serde_json::from_slice(&std::fs::read(&args[2]).expect("request")).expect("request JSON");
    let samples = std::fs::read(&req.pcm).expect("audio").len() / 2;
    assert!(req.segmentation.ends_with("segmentation.onnx") && req.embedding.ends_with("embedding.onnx"));

    let dir = req.lib_dir.to_string_lossy().to_string();
    if dir.ends_with("fail") {
        eprintln!("error: Sprechertrennung startet nicht (Modelldateien prüfen)");
        std::process::exit(1);
    }
    eprintln!("progress 40");
    if dir.ends_with("slow") {
        std::thread::sleep(Duration::from_secs(60));
    }
    let end_ms = samples as i64 * 1000 / 16_000;
    let out = match req.mode {
        Mode::Diarize { speakers: n } => {
            assert!((2..=4).contains(&n), "{n} speakers");
            Output {
                turns: (0..end_ms / 1000).map(|s| Turn { start_ms: s * 1000, end_ms: s * 1000 + 900, speaker: (s % n as i64) as i32 }).collect(),
                // Speaker k's print is the k-th unit vector.
                speakers: (0..n)
                    .map(|k| Print { speaker: k, embedding: (0..n).map(|i| if i == k { 1.0 } else { 0.0 }).collect(), seconds: 1.0 })
                    .collect(),
            }
        }
        Mode::Embed => Output { turns: vec![], speakers: vec![Print { speaker: 0, embedding: vec![0.0, 1.0], seconds: end_ms as f32 / 1000.0 }] },
    };
    eprintln!("progress 100");
    std::fs::write(&req.out, serde_json::to_vec(&out).unwrap()).unwrap();
}
