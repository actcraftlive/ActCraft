//! `cargo xtask <task>`: ActCraft's build gate.
//!
//! - `ci`: fmt, clippy (warnings are errors), tests, layers, wasm, licences, assets — in that
//!   order, stopping at the first failure. Run it before every commit.
//! - `layers`, `wasm`, `licences`, `assets`: one step on its own.
//!
//! `--skip-missing-targets` lets the wasm step pass with a loud note when the
//! `wasm32-unknown-unknown` target isn't installed on this computer; CI never passes it.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod layers;
mod licences;

use std::path::{Path, PathBuf};
use std::process::Command;

use craft_xtask::assets::{self, Policy};
use craft_xtask::runner::{self, RunOptions, Step, TestMode};

/// Crates whose code must build for `wasm32-unknown-unknown` (everything below the platform layer).
pub const WASM_CRATES: &[&str] = &[
    "actcraft-command",
    "actcraft-show",
    "actcraft-library",
    "actcraft-audio",
    "actcraft-lyrics",
    "actcraft-notation",
    "actcraft-engine",
];

/// Asset policy: every image, sound, document or font in the repository needs an ATTRIBUTION.md
/// row; font files are not allowed at all (they live in storytold/craft-fonts).
const ASSETS: Policy<'static> = Policy {
    forbidden_ext: &["ttf", "otf", "ttc", "otc", "woff", "woff2"],
    skip_folders: &["target"],
    ..Policy::new("ATTRIBUTION.md")
};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).take(8).collect();
    let task = args.first().map_or("ci", String::as_str);
    let skip_missing = args.iter().any(|a| a == "--skip-missing-targets");
    let result = match runner::root(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        Ok(root) if task == "icon" => icon(&root, args.get(1).map(String::as_str)),
        Ok(root) if task == "package" => package(&root),
        Ok(root) => run(task, &root, skip_missing),
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = result {
        eprintln!("xtask: {e}");
        std::process::exit(1);
    }
}

/// `cargo xtask icon [SOURCE]`: makes the logo files the app and docs use from a large source
/// picture: `assets/logo/actcraft-logo.png` (512 px) and `assets/logo/actcraft-icon-256.png`
/// (the window icon, embedded in the binary) and `assets/logo/actcraft.ico` (the Windows program
/// icon).
fn icon(root: &Path, source: Option<&str>) -> Result<(), String> {
    let dir = root.join("assets").join("logo");
    let src = source.map_or_else(|| dir.join("actcraft-logo.png"), PathBuf::from);
    let img = image::open(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    // The drawing is finely detailed, so 512 px keeps the repository light.
    for (size, name) in [(512, "actcraft-logo.png"), (256, "actcraft-icon-256.png")] {
        let out = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgba8();
        let path = dir.join(name);
        out.save(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }
    // The Windows program icon: every size Explorer, the taskbar and shortcuts ask for, each a
    // PNG inside the .ico (Windows Vista and later read those).
    let mut pngs = Vec::new();
    for size in [16u32, 20, 24, 32, 40, 48, 64, 128, 256] {
        let out = img.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgba8();
        let mut png = Vec::new();
        image::ImageEncoder::write_image(
            image::codecs::png::PngEncoder::new(&mut png),
            out.as_raw(),
            size,
            size,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| e.to_string())?;
        pngs.push((size, png));
    }
    let path = dir.join("actcraft.ico");
    std::fs::write(&path, ico(&pngs)?).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

/// An .ico file holding these PNG pictures (size in pixels, PNG bytes).
fn ico(pngs: &[(u32, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let count = u16::try_from(pngs.len()).map_err(|_| "too many icon sizes".to_owned())?;
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&count.to_le_bytes());
    let mut offset = 6 + 16 * u32::from(count);
    for (size, png) in pngs {
        // 0 stands for 256 in the one-byte width and height.
        let side = u8::try_from(*size).unwrap_or(0);
        let len = u32::try_from(png.len()).map_err(|_| "icon picture too large".to_owned())?;
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += len;
    }
    for (_, png) in pngs {
        out.extend_from_slice(png);
    }
    Ok(out)
}

/// `cargo xtask package`: a portable, unsigned archive of the app for this computer's system, in
/// `target/package/`: the release binaries with the licences, the attribution and the readme.
/// A zip on Windows, a .tar.gz elsewhere (made with the system's `tar`). Nothing is signed or
/// published.
fn package(root: &Path) -> Result<(), String> {
    let mut build = runner::cargo(root);
    build.args(["build", "--release", "-p", "actcraft", "--bins"]);
    runner::run(build, "cargo build --release -p actcraft --bins").map_err(|e| e.to_string())?;
    let version = workspace_version(root)?;
    let name = format!("actcraft-{version}-{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    let out = root.join("target").join("package");
    let dir = out.join(&name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let exe = std::env::consts::EXE_SUFFIX;
    let release = root.join("target").join("release");
    for bin in ["actcraft", "actcraft-cli"] {
        let file = format!("{bin}{exe}");
        copy(&release.join(&file), &dir.join(&file))?;
    }
    for doc in ["LICENSE-MIT", "LICENSE-APACHE", "ATTRIBUTION.md", "README.md"] {
        copy(&root.join(doc), &dir.join(doc))?;
    }
    copy(&root.join("docs").join("licences.md"), &dir.join("THIRD-PARTY-LICENCES.md"))?;
    let archive = if cfg!(windows) { format!("{name}.zip") } else { format!("{name}.tar.gz") };
    let path = out.join(&archive);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let mut tar = std::process::Command::new("tar");
    tar.current_dir(&out);
    // `-a` picks the format from the name: zip on Windows (its tar is bsdtar), gzip elsewhere.
    tar.args(["-a", "-c", "-f", &archive, &name]);
    runner::run(tar, &format!("tar -a -c -f {archive} {name}")).map_err(|e| e.to_string())?;
    println!("wrote {}", path.display());
    Ok(())
}

/// The workspace's version from the root `Cargo.toml`.
fn workspace_version(root: &Path) -> Result<String, String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?;
    manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("version = \"").and_then(|v| v.strip_suffix('"')))
        .filter(|v| v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
        .map(str::to_owned)
        .ok_or_else(|| "no version in Cargo.toml".to_owned())
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("{}: {e}", from.display()))
}

fn run(task: &str, root: &Path, skip_missing: bool) -> Result<(), String> {
    let root = root.to_path_buf();
    let mut steps: Vec<Step<'_>> = Vec::new();
    let want = |name: &str| task == "ci" || task == name;
    if task == "ci" {
        let r = root.clone();
        steps.push(Step::new("fmt", move |log| {
            let mut c = runner::cargo(&r);
            c.args(["fmt", "--all", "--", "--check"]);
            runner::run(c, log.command("cargo fmt --all -- --check"))
        }));
        let r = root.clone();
        steps.push(Step::new("clippy", move |log| {
            let mut c = runner::cargo(&r);
            c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
            runner::run(c, log.command("cargo clippy --workspace --all-targets -- -D warnings"))
        }));
        let r = root.clone();
        steps.push(Step::new("test", move |log| {
            let (c, label) = runner::nextest_or_test(&r, "xtask", TestMode::CargoTest, &["--lib", "--bins", "--tests"])
                .map_err(|e| e.to_string())?;
            runner::run(c, log.command(&label))
        }));
    }
    if want("layers") {
        let r = root.clone();
        steps.push(Step::new("layers", move |log| {
            log.command("cargo metadata (layer rules)");
            layers::check(&r)
        }));
    }
    if want("wasm") {
        let r = root.clone();
        steps.push(Step::new("wasm", move |log| {
            if !target_installed(&r, "wasm32-unknown-unknown") {
                let msg =
                    "the wasm32-unknown-unknown target is not installed (rustup target add wasm32-unknown-unknown)";
                if skip_missing {
                    eprintln!("NOTE: wasm step NOT RUN: {msg}");
                    return Ok(());
                }
                return Err(msg.to_owned());
            }
            let mut c = runner::cargo(&r);
            c.args(["check", "--target", "wasm32-unknown-unknown"]);
            for k in WASM_CRATES {
                c.args(["-p", k]);
            }
            runner::run(c, log.command("cargo check --target wasm32-unknown-unknown (engine crates)"))
        }));
    }
    if want("licences") {
        let r = root.clone();
        steps.push(Step::new("licences", move |log| {
            log.command("cargo metadata (licence allowlist)");
            licences::check(&r)
        }));
    }
    if want("assets") {
        let r = root.clone();
        steps.push(Step::new("assets", move |log| {
            log.command("assets attribution check");
            let report = assets::run(&r, &ASSETS).map_err(|e| e.to_string())?;
            if report.is_ok() {
                Ok(())
            } else {
                Err(report.violations.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))
            }
        }));
    }
    if steps.is_empty() {
        return Err(format!("unknown task `{task}`; use ci, layers, wasm, licences or assets"));
    }
    runner::run_steps(&mut steps, &RunOptions::default().with_title(format!("cargo xtask {task}")))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn target_installed(root: &PathBuf, target: &str) -> bool {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let out = Command::new(rustc).current_dir(root).args(["--print", "target-libdir", "--target", target]).output();
    match out {
        Ok(o) if o.status.success() => {
            let dir = String::from_utf8_lossy(&o.stdout).trim().to_owned();
            std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some())
        }
        _ => false,
    }
}
