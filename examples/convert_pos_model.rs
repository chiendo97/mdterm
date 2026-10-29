//! One-off tool: gzip NLTK's `weights.json` into `pos_model/weights.json.gz`.
//!
//! Usage:
//!     cargo run --example convert_pos_model --features pos -- pos_model/weights.json pos_model/weights.json.gz
//!
//! Run only when updating the vendored model. `src/pos.rs` embeds the output
//! via `include_bytes!` and gunzips it at load time. Downcasting/conversion is
//! NOT done here — we keep the original f64 JSON and let serde parse f64→f32
//! at load. Compression is what shrinks the ~5.7 MB JSON to ~1.5 MB.

use std::env;
use std::fs;
use std::io::Write;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: convert_pos_model <weights.json> <weights.json.gz>");
        process::exit(2);
    }
    let src = &args[1];
    let dst = &args[2];

    let bytes = fs::read(src).expect("read weights.json");
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&bytes).expect("gzip");
    let gz = encoder.finish().expect("finish gz");
    fs::write(dst, &gz).expect("write weights.json.gz");
    eprintln!(
        "wrote {} ({} bytes, source {} bytes) from {}",
        dst,
        gz.len(),
        bytes.len(),
        src
    );
}
