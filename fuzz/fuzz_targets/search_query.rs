#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data)
        && let Ok(q) = actcraft_library::search::Query::parse(text)
    {
        let _ = q.matches(&actcraft_show::Song::default());
    }
});
