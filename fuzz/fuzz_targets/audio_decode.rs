#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = actcraft_audio::clip::decode(data, None);
    let _ = actcraft_audio::clip::wav_markers(data);
});
