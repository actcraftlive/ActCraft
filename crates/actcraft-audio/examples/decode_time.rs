//! Times decoding an audio file: `cargo run --release -p actcraft-audio --example decode_time -- FILE`.

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: decode_time FILE");
        return;
    };
    let read = std::time::Instant::now();
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("cannot read {path}");
        return;
    };
    let read_ms = read.elapsed().as_millis();
    let ext = path.rsplit('.').next().map(str::to_ascii_lowercase);
    let start = std::time::Instant::now();
    match actcraft_audio::clip::decode(&bytes, ext.as_deref()) {
        Ok(clip) => {
            let decode_ms = start.elapsed().as_millis();
            let secs = clip.frames() as f64 / f64::from(clip.sample_rate.max(1));
            let peaks = std::time::Instant::now();
            let _ = actcraft_audio::peaks::Peaks::of(&clip);
            println!(
                "read {read_ms} ms, decode {decode_ms} ms, peaks {} ms, {secs:.1} s of audio at {} Hz, {} channels",
                peaks.elapsed().as_millis(),
                clip.sample_rate,
                clip.channels
            );
        }
        Err(e) => eprintln!("decode failed: {e}"),
    }
}
