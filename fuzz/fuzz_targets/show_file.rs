#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(show) = actcraft_show::Show::from_bytes(data) {
        let _ = actcraft_show::Show::from_bytes(&show.to_bytes());
    }
});
