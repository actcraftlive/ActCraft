#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let (fmt, name) = text.split_once('\n').unwrap_or((text, "file.wav"));
        let _ = actcraft_library::filename::parse(name, fmt);
    }
});
