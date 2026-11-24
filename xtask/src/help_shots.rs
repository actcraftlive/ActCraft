//! `cargo xtask help-shots`: the screenshots of the help, in every language it has.
//!
//! 1. Builds `actcraft` and `actcraft-cli`.
//! 2. Makes an example show in `target/help-shots/` with `actcraft-cli`: three songs (their audio is
//!    a quiet tone this task writes), lyrics with chords, regions, a setlist with an encore, a moving
//!    head from the fixture library, four pars, presets, a cue list and a sequence. Nothing of this
//!    computer goes in.
//! 3. Opens the window once per picture and language with `--no-devices` (no audio interface, no
//!    remote control) and settings in `target/help-shots/settings`, so the person's own settings
//!    are neither shown nor changed, and saves the picture to
//!    `crates/actcraft-ui/i18n/help/img_<language>/<name>.png`.
//!
//! Every language folder of the help (`crates/actcraft-ui/i18n/help/<language>/`) gets its pictures.
//! The pictures are recorded in ATTRIBUTION.md by one row for the `img_*` folders.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// The pictures: file name, then what the window opens with.
const SHOTS: &[(&str, &[&str])] = &[
    ("window", &[]),
    ("lyrics", &["--lyrics"]),
    ("fixtures", &["--fixtures"]),
    ("presets", &["--presets"]),
    ("lighting-settings", &["--open-settings", "lighting"]),
    ("about", &["--about"]),
];

/// How long one window may take to start and save its picture.
const WAIT: Duration = Duration::from_secs(60);

pub fn run(root: &Path) -> Result<(), String> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned()))
        .current_dir(root)
        .args(["build", "-p", "actcraft", "--bin", "actcraft", "--bin", "actcraft-cli"])
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err("cargo build failed".to_owned());
    }
    let bin = root.join("target").join("debug");
    let exe = |name: &str| bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let dir = root.join("target").join("help-shots");
    // A fresh start: the show and settings of the last run would carry over otherwise.
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir.join("settings")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let show = example_show(&exe("actcraft-cli"), &dir)?;

    let help = root.join("crates").join("actcraft-ui").join("i18n").join("help");
    for lang in languages(&help)? {
        let out = help.join(format!("img_{lang}"));
        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        for (name, flags) in SHOTS {
            let raw = dir.join(format!("{lang}-{name}.png"));
            shoot(&exe("actcraft"), &dir, &show, &lang, flags, &raw)?;
            let file = out.join(format!("{name}.png"));
            compress(&raw, &file)?;
            println!("{}", file.strip_prefix(root).unwrap_or(&file).display());
        }
    }
    Ok(())
}

/// The help's languages: its folders other than the `img_` ones.
fn languages(help: &Path) -> Result<Vec<String>, String> {
    let mut langs: Vec<String> = std::fs::read_dir(help)
        .map_err(|e| format!("{}: {e}", help.display()))?
        .filter_map(Result::ok)
        .filter(|d| d.path().is_dir())
        .filter_map(|d| d.file_name().to_str().map(str::to_owned))
        .filter(|n| !n.starts_with("img_"))
        .collect();
    langs.sort();
    Ok(langs)
}

/// Opens the window on the example show and waits until it has saved its picture and closed.
fn shoot(exe: &Path, dir: &Path, show: &Path, lang: &str, flags: &[&str], to: &Path) -> Result<(), String> {
    let mut child = Command::new(exe)
        .arg(show)
        .args(["--lang", lang, "--no-devices"])
        .args(flags)
        .arg("--screenshot")
        .arg(to)
        .env("ACTCRAFT_CONFIG_DIR", dir.join("settings"))
        .spawn()
        .map_err(|e| format!("{}: {e}", exe.display()))?;
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() && to.exists() {
                Ok(())
            } else {
                Err(format!("the window made no picture for {}", to.display()))
            };
        }
        if started.elapsed() > WAIT {
            let _ = child.kill();
            return Err(format!("the window took too long for {}", to.display()));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Saves the picture without its alpha channel and with the strongest PNG compression, which
/// keeps the help's pictures small in the repository and the binary.
fn compress(from: &Path, to: &Path) -> Result<(), String> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let img = image::open(from).map_err(|e| format!("{}: {e}", from.display()))?.to_rgb8();
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Best, FilterType::Adaptive)
        .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    std::fs::write(to, bytes).map_err(|e| format!("{}: {e}", to.display()))
}

/// Builds the example show with `actcraft-cli`, one command at a time, and returns its path.
fn example_show(cli: &Path, dir: &Path) -> Result<PathBuf, String> {
    let show = dir.join("example.actcraft");
    let tone = dir.join("tone.wav");
    std::fs::write(&tone, tone_wav()).map_err(|e| format!("{}: {e}", tone.display()))?;
    let tone = tone.to_string_lossy().into_owned();
    let run = |id: &str, params: Value| -> Result<Value, String> {
        let mut cmd = Command::new(cli);
        cmd.args(["run", id, &params.to_string()]);
        if id != "show.save_as" {
            cmd.arg("--show").arg(&show).arg("--save");
        }
        let out = cmd.output().map_err(|e| format!("{}: {e}", cli.display()))?;
        if !out.status.success() {
            return Err(format!("{id}: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        serde_json::from_slice(&out.stdout).map_err(|e| format!("{id}: {e}"))
    };
    let index = |v: &Value, key: &str| v.get(key).cloned().ok_or_else(|| format!("no {key} in {v}"));

    run("show.save_as", json!({"path": show}))?;

    // Songs: the first one with tracks, lyrics and regions.
    let lyrics = "[G]Paper boats on the [D]river\n[Em]Carry every [C]wish we made\n\
                  [G]Down to where the [D]water\n[C]Meets the evening [D]shade\n\n\
                  Chorus:\n[C]Let them [G]go, let them [D]go\n[C]Further than we'll [D]ever know";
    let first = index(
        &run(
            "song.add",
            json!({"title": "Paper Boats", "artist": "The Example Band", "key": "G", "bpm": 96,
                   "beats_per_bar": 4, "lyrics": lyrics}),
        )?,
        "song",
    )?;
    for track in ["Click", "Bass", "Keys", "Vocals"] {
        run("track.add", json!({"song": first, "name": track, "file": tone}))?;
    }
    run("region.add", json!({"song": first, "name": "Verse", "start": 0.0, "end": 10.0, "mode": "play"}))?;
    run("region.add", json!({"song": first, "name": "Chorus", "start": 10.0, "end": 20.0, "mode": "loop"}))?;
    let second = index(
        &run("song.add", json!({"title": "Northern Lights", "artist": "The Example Band", "key": "Em", "bpm": 124}))?,
        "song",
    )?;
    let third = index(
        &run("song.add", json!({"title": "Last Train Home", "artist": "The Example Band", "key": "A", "bpm": 88}))?,
        "song",
    )?;
    for song in [&second, &third] {
        run("track.add", json!({"song": song, "name": "Mix", "file": tone}))?;
    }

    // A setlist with an encore and a note before its song.
    let list = index(&run("playlist.create", json!({"name": "Summer Festival"}))?, "playlist")?;
    run("playlist.add_songs", json!({"playlist": list, "set": 0, "songs": [first, second]}))?;
    run("playlist.add_set", json!({"playlist": list, "name": "Encore"}))?;
    run("playlist.add_songs", json!({"playlist": list, "set": 1, "songs": [third]}))?;
    run("set.note", json!({"playlist": list, "set": 1, "song": third, "text": "Acoustic guitar"}))?;

    // Lights: a moving head from the library and four pars, two looks, a cue list, a sequence.
    let kind = index(&run("fixture_type.from_library", json!({"name": "Betopper LM70SR"}))?, "type")?;
    run("fixture.add", json!({"type": kind, "mode": 0, "universe": 1, "address": 1, "name": "Moving head"}))?;
    let pars = index(
        &run("fixture.add", json!({"kind": "rgb", "universe": 1, "address": 20, "count": 4, "name": "Par"}))?,
        "fixtures",
    )?;
    run("group.set", json!({"name": "Pars", "fixtures": pars}))?;
    run("fixture.set", json!({"fixture": 0, "values": {"intensity": 1.0, "pan": 0.3, "tilt": 0.6}}))?;
    run(
        "fixture.set",
        json!({"fixtures": pars, "values": {"intensity": 1.0, "red": 1.0, "green": 0.45, "blue": 0.1}}),
    )?;
    let warm = index(&run("preset.store", json!({"name": "Warm"}))?, "preset")?;
    run("fixture.set", json!({"fixture": 0, "values": {"pan": 0.7, "tilt": 0.4}}))?;
    run("fixture.set", json!({"fixtures": pars, "values": {"red": 0.1, "green": 0.2, "blue": 1.0}}))?;
    let blue = index(&run("preset.store", json!({"name": "Blue"}))?, "preset")?;
    let cues = index(&run("cue_list.add", json!({"name": "Paper Boats"}))?, "list")?;
    run("cue.add", json!({"list": cues, "preset": warm, "fade": 2.0, "name": "Verse"}))?;
    run("cue.add", json!({"list": cues, "preset": blue, "fade": 1.0, "name": "Chorus"}))?;
    let seq = index(&run("sequence.set", json!({"name": "Sweep", "mode": "bounce", "in_beats": true}))?, "sequence")?;
    run("sequence.step_add", json!({"sequence": seq, "preset": warm, "length": 4.0, "fade": 0.5}))?;
    run("sequence.step_add", json!({"sequence": seq, "preset": blue, "length": 4.0, "fade": 0.5}))?;
    Ok(show)
}

/// Twenty seconds of a quiet 220 Hz tone, 44.1 kHz mono 16-bit WAV: audio for the example songs.
fn tone_wav() -> Vec<u8> {
    const RATE: u32 = 44_100;
    let samples = RATE * 20;
    let data_len = samples * 2;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&RATE.to_le_bytes());
    b.extend_from_slice(&(RATE * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..samples {
        let t = f64::from(i) / f64::from(RATE);
        let v = (t * 220.0 * std::f64::consts::TAU).sin() * 0.1 * f64::from(i16::MAX);
        b.extend_from_slice(&(v as i16).to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tone_is_a_wav_file_of_twenty_seconds() {
        let wav = tone_wav();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 44_100 * 20 * 2);
    }

    #[test]
    fn the_languages_are_the_help_folders_without_pictures() {
        let help = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/actcraft-ui/i18n/help");
        let langs = languages(&help).unwrap_or_default();
        assert!(langs.contains(&"en".to_owned()), "{langs:?}");
        assert!(langs.iter().all(|l| !l.starts_with("img_")), "{langs:?}");
    }
}
