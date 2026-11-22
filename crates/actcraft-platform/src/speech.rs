//! The computer's own speech, for spoken cues. Each platform's speech is run as its own program
//! (nothing is linked in), writing an audio file that is read back:
//!
//! - Windows: the built-in speech of .NET (`System.Speech`) through PowerShell; the text and file
//!   go in environment variables, never into the script.
//! - macOS: `say`.
//! - Linux and others: `espeak-ng`, `espeak` or `pico2wave`, whichever is installed.
//!
//! Text and paths are passed as separate arguments (or variables), never through a shell.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use actcraft_engine::services::Speaker;

/// Longest text spoken at once.
const MAX_TEXT: usize = 400;
/// How long one phrase may take before it is given up.
const TIMEOUT: Duration = Duration::from_secs(30);

/// The speaker for this computer.
pub fn speaker() -> Speaker {
    Arc::new(|text: &str, language: &str| speak(text, language))
}

fn speak(text: &str, language: &str) -> Result<Vec<u8>, String> {
    let text: String = text.chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect();
    if text.trim().is_empty() {
        return Err("nothing to say".to_owned());
    }
    let language: String = language.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(16).collect();
    let file = temp_file(if cfg!(target_os = "macos") { "aiff" } else { "wav" });
    let result = run(&text, &language, &file).and_then(|()| std::fs::read(&file).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&file);
    result.and_then(|bytes| if bytes.len() > 44 { Ok(bytes) } else { Err("the speech came out empty".to_owned()) })
}

/// A fresh file name in the temporary folder.
fn temp_file(ext: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("actcraft-voice-{}-{n}.{ext}", std::process::id()))
}

#[cfg(windows)]
fn run(text: &str, language: &str, file: &Path) -> Result<(), String> {
    // The script is fixed; the text, file and language come from variables.
    let script = "Add-Type -AssemblyName System.Speech; \
        $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
        if ($env:ACTCRAFT_SAY_LANG) { \
          $v = $s.GetInstalledVoices() | Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.TwoLetterISOLanguageName -eq $env:ACTCRAFT_SAY_LANG } | Select-Object -First 1; \
          if ($v) { $s.SelectVoice($v.VoiceInfo.Name) } }; \
        $s.SetOutputToWaveFile($env:ACTCRAFT_SAY_FILE); \
        $s.Speak($env:ACTCRAFT_SAY_TEXT); $s.Dispose()";
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .env("ACTCRAFT_SAY_TEXT", text)
        .env("ACTCRAFT_SAY_FILE", file)
        .env("ACTCRAFT_SAY_LANG", language);
    hide_window(&mut cmd);
    wait(cmd, "PowerShell speech")
}

#[cfg(windows)]
fn hide_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: no console flashes up while speaking.
    cmd.creation_flags(0x0800_0000);
}

#[cfg(target_os = "macos")]
fn run(text: &str, language: &str, file: &Path) -> Result<(), String> {
    let mut cmd = Command::new("say");
    cmd.arg("-o").arg(file);
    if let Some(voice) = mac_voice(language) {
        cmd.args(["-v", voice]);
    }
    cmd.arg("--").arg(text);
    wait(cmd, "say")
}

/// A voice macOS ships for a language, or the system's own.
#[cfg(target_os = "macos")]
fn mac_voice(language: &str) -> Option<&'static str> {
    match language {
        "de" => Some("Anna"),
        "en" => Some("Samantha"),
        "fr" => Some("Thomas"),
        "es" => Some("Monica"),
        "it" => Some("Alice"),
        _ => None,
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn run(text: &str, language: &str, file: &Path) -> Result<(), String> {
    let mut last = String::from("no speech program found (install espeak-ng)");
    for program in ["espeak-ng", "espeak"] {
        let mut cmd = Command::new(program);
        cmd.arg("-w").arg(file);
        if !language.is_empty() {
            cmd.args(["-v", language]);
        }
        cmd.arg("--").arg(text);
        match wait(cmd, program) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
    }
    let mut cmd = Command::new("pico2wave");
    cmd.arg("-w").arg(file);
    if let Some(lang) = pico_language(language) {
        cmd.arg("-l").arg(lang);
    }
    cmd.arg("--").arg(text);
    wait(cmd, "pico2wave").map_err(|_| last)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn pico_language(language: &str) -> Option<&'static str> {
    match language {
        "de" => Some("de-DE"),
        "en" => Some("en-US"),
        "fr" => Some("fr-FR"),
        "es" => Some("es-ES"),
        "it" => Some("it-IT"),
        _ => None,
    }
}

/// Runs a speech program to its end, within the time limit.
fn wait(mut cmd: Command, what: &str) -> Result<(), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{what} could not start: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{what} failed ({status})")),
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{what} took too long"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("{what}: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_refused_before_anything_runs() {
        assert!(speak("", "de").is_err());
        assert!(speak("\n\t", "").is_err());
    }

    /// Needs a computer with speech installed; run with `cargo test -p actcraft-platform -- --ignored`.
    #[test]
    #[ignore = "needs the computer's speech"]
    fn the_computer_speaks_into_audio_actcraft_can_play() {
        let bytes = speak("Refrain", "de");
        assert!(bytes.is_ok(), "{:?}", bytes.as_ref().err());
        let bytes = bytes.unwrap_or_default();
        // A WAV (or AIFF on macOS) file with a good part of a second of sound in it.
        assert!(bytes.starts_with(b"RIFF") || bytes.starts_with(b"FORM"));
        assert!(bytes.len() > 10_000, "{} bytes", bytes.len());
    }

    #[test]
    fn temp_files_are_new_each_time() {
        assert_ne!(temp_file("wav"), temp_file("wav"));
    }
}
