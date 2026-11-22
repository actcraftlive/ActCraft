#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(mut score) = actcraft_notation::read(data) {
        for part in &mut score.parts {
            actcraft_notation::tab::fill_frets(part);
        }
    }
});
