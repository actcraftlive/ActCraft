#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let text = actcraft_lyrics::decode_text(data);
    let lyrics = actcraft_lyrics::parse(&text);
    let _ = lyrics.line_at(30.0);
});
