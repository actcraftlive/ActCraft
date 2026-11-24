//! Fixture types from `.lightkeyfxt` fixture profiles, so people bring the fixtures they already
//! set up in another lighting program. Such a file is a binary property list (read by the shared
//! `craft-plist`) holding a keyed archive: a list of objects that refer to each other by number,
//! each naming its class. Learnt from the files themselves: the profile has a `name` and a list of
//! modes (`personalities`); a mode has a channel count (`footprint`), an optional `customName` and
//! a set of `capabilities`. A capability sits on one `channel` (from 0); its class says what it
//! does (a colour component names its colour in `componentName`, a custom one has a
//! `customName`), and its `settings` are parts of the channel's levels, each with a `name` and its
//! lowest and highest level (one byte each) in an old-style array under `$0`.

use actcraft_command::CommandError;
use actcraft_show::limits::{MAX_CHANNEL_RANGES, MAX_FIXTURE_CHANNELS, MAX_FIXTURE_MODES};
use actcraft_show::{ChannelRange, FixtureMode, FixtureType};
use craft_plist::{Limits, Value};

use crate::fixture_library::{attribute_by_name, colour};
use crate::fixtures::Attribute;

/// The extension of these fixture profiles.
pub const EXTENSION: &str = "lightkeyfxt";

/// Most items of a list looked at.
const MAX_ITEMS: usize = 1_024;

fn not_a_profile() -> CommandError {
    CommandError::Failed("that file is not a .lightkeyfxt fixture profile".into())
}

/// The objects of a keyed archive, followed by number.
struct Archive<'a> {
    objects: &'a [Value],
}

impl<'a> Archive<'a> {
    /// The object a reference points at (object 0 is the archive's "nothing"), or the value.
    fn follow(&self, v: &'a Value) -> &'a Value {
        match v {
            Value::Uid(0) => &Value::Null,
            Value::Uid(u) => usize::try_from(*u).ok().and_then(|i| self.objects.get(i)).unwrap_or(&Value::Null),
            other => other,
        }
    }

    /// A field of an object, followed.
    fn field(&self, obj: &'a Value, key: &str) -> &'a Value {
        obj.get(key).map_or(&Value::Null, |v| self.follow(v))
    }

    fn text(&self, obj: &'a Value, key: &str) -> Option<&'a str> {
        self.field(obj, key).as_str().map(str::trim).filter(|s| !s.is_empty())
    }

    fn int(&self, obj: &'a Value, key: &str) -> Option<i128> {
        self.field(obj, key).as_int()
    }

    /// The name of an object's class.
    fn class(&self, obj: &'a Value) -> &'a str {
        self.field(obj, "$class").get("$classname").and_then(Value::as_str).unwrap_or_default()
    }

    /// The items of an array or a set, followed.
    fn items(&self, obj: &'a Value) -> impl Iterator<Item = &'a Value> + '_ {
        self.field(obj, "NS.objects").as_array().unwrap_or_default().iter().take(MAX_ITEMS).map(|v| self.follow(v))
    }

    /// The value under `key` in a dictionary (its keys and values are two lists side by side).
    fn lookup(&self, dict: &'a Value, key: &str) -> &'a Value {
        let keys = self.field(dict, "NS.keys").as_array().unwrap_or_default();
        let values = self.field(dict, "NS.objects").as_array().unwrap_or_default();
        keys.iter()
            .zip(values)
            .take(MAX_ITEMS)
            .find(|(k, _)| self.follow(k).as_str() == Some(key))
            .map_or(&Value::Null, |(_, v)| self.follow(v))
    }

    /// A setting's lowest and highest level, when they are one byte each.
    fn levels(&self, setting: &'a Value) -> Option<(u8, u8)> {
        let pair = self.field(setting, "$0");
        if self.int(pair, "NS.count") != Some(2) || self.int(pair, "NS.size") != Some(1) {
            return None;
        }
        // Stored as signed bytes: -1 is 255.
        let byte = |key| self.int(pair, key).map(|v| (v & 0xFF) as u8);
        Some((byte("$0")?, byte("$1")?))
    }
}

/// What a capability drives, from its class (`LX…Capability`), its colour or its own name.
fn attribute(class: &str, component: Option<&str>, name: Option<&str>) -> Attribute {
    let kind = class.to_ascii_lowercase();
    if kind.contains("colorcomponent") {
        return colour(component.unwrap_or_default());
    }
    const BY_CLASS: &[(&str, Attribute)] = &[
        ("intensity", Attribute::Intensity),
        ("dimmer", Attribute::Intensity),
        ("pan", Attribute::Pan),
        ("tilt", Attribute::Tilt),
        ("strobe", Attribute::Strobe),
        ("shutter", Attribute::Strobe),
        ("colorwheel", Attribute::ColourWheel),
        ("gobo", Attribute::Gobo),
        ("zoom", Attribute::Zoom),
        ("focus", Attribute::Focus),
    ];
    let kind = kind.trim_start_matches("lx");
    BY_CLASS
        .iter()
        .find(|(word, _)| kind.starts_with(word))
        .map_or_else(|| name.map_or(Attribute::Other, attribute_by_name), |(_, a)| *a)
}

/// Reads a fixture type from the bytes of a `.lightkeyfxt` fixture profile.
pub fn read_profile(bytes: &[u8]) -> Result<FixtureType, CommandError> {
    let limits = Limits { max_input_bytes: crate::fixture_library::MAX_BYTES, ..Limits::default() };
    let root = craft_plist::parse_binary_with_limits(bytes, limits).map_err(|_| not_a_profile())?;
    let objects = root.get("$objects").and_then(Value::as_array).ok_or_else(not_a_profile)?;
    let a = Archive { objects };
    let top = root.get("$top").ok_or_else(not_a_profile)?;
    let profile = a.field(top, "fixtureProfile");
    let name = a.text(profile, "name").ok_or_else(not_a_profile)?;
    let mut modes = Vec::new();
    for personality in a.items(a.field(profile, "personalities")) {
        if modes.len() >= MAX_FIXTURE_MODES {
            break;
        }
        let Some(count) = a.int(personality, "footprint").and_then(|n| usize::try_from(n).ok()) else { continue };
        if count == 0 || count > MAX_FIXTURE_CHANNELS {
            continue;
        }
        let mut channels = vec![Attribute::Other; count];
        let mut ranges = Vec::new();
        for cap in a.items(a.field(personality, "capabilities")) {
            let Some(at) = a.int(cap, "channel").and_then(|c| usize::try_from(c).ok()).filter(|&c| c < count) else {
                continue;
            };
            let custom = a.text(cap, "customName");
            let settings: Vec<&Value> = a.items(a.field(cap, "settings")).collect();
            let component = settings.iter().find_map(|s| a.lookup(a.field(s, "params"), "componentName").as_str());
            if let Some(slot) = channels.get_mut(at)
                && *slot == Attribute::Other
            {
                *slot = attribute(a.class(cap), component, custom);
            }
            for s in &settings {
                let own = a.lookup(a.field(s, "params"), "name").as_str().map(str::trim).filter(|n| !n.is_empty());
                let words = match (custom, own) {
                    (Some(c), Some(o)) => format!("{c}: {o}"),
                    (None, Some(o)) => o.to_owned(),
                    (Some(c), None) if settings.len() == 1 => c.to_owned(),
                    _ => continue,
                };
                let Some((from, to)) = a.levels(s).filter(|(f, t)| f <= t) else { continue };
                if ranges.len() < MAX_CHANNEL_RANGES {
                    ranges.push(ChannelRange {
                        channel: u16::try_from(at + 1).unwrap_or(1),
                        from,
                        to,
                        name: words.chars().take(64).collect(),
                        ..ChannelRange::default()
                    });
                }
            }
        }
        ranges.sort_by_key(|r| (r.channel, r.from));
        let mode_name = a
            .text(personality, "customName")
            .map_or_else(|| format!("{count} channels"), |n| n.chars().take(64).collect());
        modes.push(FixtureMode { name: mode_name, channels, ranges, ..FixtureMode::default() });
    }
    if modes.is_empty() {
        return Err(CommandError::Failed("that fixture has no mode ActCraft can patch".into()));
    }
    Ok(FixtureType { name: name.chars().take(64).collect(), modes, ..FixtureType::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Engine;
    use serde_json::json;

    /// Writes a value as a binary property list (only what these tests need: each value one
    /// object, 2-byte references, 4-byte offsets).
    fn write(top: &Value) -> Vec<u8> {
        fn head(kind: u8, n: usize, out: &mut Vec<u8>) {
            if n < 15 {
                out.push(kind << 4 | n as u8);
            } else {
                out.push(kind << 4 | 0x0F);
                out.push(0x11);
                out.extend((n as u16).to_be_bytes());
            }
        }
        fn add(v: &Value, objs: &mut Vec<Vec<u8>>) -> u16 {
            let at = objs.len();
            objs.push(Vec::new());
            let mut out = Vec::new();
            match v {
                Value::Null => out.push(0x00),
                Value::Bool(b) => out.push(if *b { 0x09 } else { 0x08 }),
                Value::Integer(i) => {
                    out.push(0x13);
                    out.extend((*i as i64).to_be_bytes());
                }
                Value::Real(r) => {
                    out.push(0x23);
                    out.extend(r.to_be_bytes());
                }
                Value::String(s) => {
                    head(0x5, s.len(), &mut out);
                    out.extend(s.as_bytes());
                }
                Value::Data(d) => {
                    head(0x4, d.len(), &mut out);
                    out.extend(d);
                }
                Value::Uid(u) => {
                    out.push(0x81);
                    out.extend((*u as u16).to_be_bytes());
                }
                Value::Array(items) => {
                    let refs: Vec<u16> = items.iter().map(|i| add(i, objs)).collect();
                    head(0xA, refs.len(), &mut out);
                    refs.iter().for_each(|r| out.extend(r.to_be_bytes()));
                }
                Value::Dict(pairs) => {
                    let keys: Vec<u16> = pairs.iter().map(|(k, _)| add(&Value::String(k.clone()), objs)).collect();
                    let values: Vec<u16> = pairs.iter().map(|(_, v)| add(v, objs)).collect();
                    head(0xD, keys.len(), &mut out);
                    keys.iter().chain(&values).for_each(|r| out.extend(r.to_be_bytes()));
                }
                Value::Date(_) => out.push(0x00),
            }
            objs[at] = out;
            at as u16
        }
        let mut objs = Vec::new();
        add(top, &mut objs);
        let mut file = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        for o in &objs {
            offsets.push(file.len() as u32);
            file.extend(o);
        }
        let table = file.len() as u64;
        offsets.iter().for_each(|o| file.extend(o.to_be_bytes()));
        file.extend([0u8; 6]);
        file.extend([4, 2]);
        file.extend((objs.len() as u64).to_be_bytes());
        file.extend(0u64.to_be_bytes());
        file.extend(table.to_be_bytes());
        file
    }

    /// Builds a keyed archive object by object.
    struct Builder {
        objects: Vec<Value>,
    }

    impl Builder {
        fn new() -> Self {
            Self { objects: vec![Value::String("$null".into())] }
        }

        fn add(&mut self, v: Value) -> Value {
            self.objects.push(v);
            Value::Uid(self.objects.len() as u64 - 1)
        }

        fn object(&mut self, class: &str, fields: Vec<(&str, Value)>) -> Value {
            let c = self.add(Value::Dict(vec![
                ("$classname".into(), Value::String(class.into())),
                ("$classes".into(), Value::Array(vec![Value::String(class.into())])),
            ]));
            let mut pairs = vec![("$class".to_owned(), c)];
            pairs.extend(fields.into_iter().map(|(k, v)| (k.to_owned(), v)));
            self.add(Value::Dict(pairs))
        }

        fn text(&mut self, s: &str) -> Value {
            self.add(Value::String(s.into()))
        }

        fn list(&mut self, items: Vec<Value>) -> Value {
            self.object("NSArray", vec![("NS.objects", Value::Array(items))])
        }

        /// A setting from `from` to `to`, with a name or a colour.
        fn setting(&mut self, from: i128, to: i128, key: &str, words: &str) -> Value {
            let pair = self.object(
                "_NSKeyedCoderOldStyleArray",
                vec![
                    ("NS.count", Value::Integer(2)),
                    ("NS.size", Value::Integer(1)),
                    ("NS.type", Value::Integer(67)),
                    ("$0", Value::Integer(from)),
                    ("$1", Value::Integer(to)),
                ],
            );
            let k = self.text(key);
            let w = self.text(words);
            let params = self.object(
                "NSDictionary",
                vec![("NS.keys", Value::Array(vec![k])), ("NS.objects", Value::Array(vec![w]))],
            );
            self.object("LXSetting", vec![("$0", pair), ("params", params)])
        }

        fn capability(&mut self, class: &str, channel: i128, custom: Option<&str>, settings: Vec<Value>) -> Value {
            let custom = match custom {
                Some(c) => self.text(c),
                None => Value::Uid(0),
            };
            let settings = self.list(settings);
            self.object(
                class,
                vec![("channel", Value::Integer(channel)), ("customName", custom), ("settings", settings)],
            )
        }

        fn archive(mut self, profile: Value) -> Vec<u8> {
            let top = Value::Dict(vec![("fixtureProfile".into(), profile)]);
            write(&Value::Dict(vec![
                ("$version".into(), Value::Integer(100_000)),
                ("$archiver".into(), Value::String("NSKeyedArchiver".into())),
                ("$top".into(), top),
                ("$objects".into(), Value::Array(std::mem::take(&mut self.objects))),
            ]))
        }
    }

    /// A made-up 6-channel wash light in the layout these profiles have (our own test data):
    /// a mode channel with four parts, a dimmer by name, red, green, blue, and a speed.
    fn wash() -> Vec<u8> {
        let mut b = Builder::new();
        let mut caps = Vec::new();
        let parts = vec![
            b.setting(0, 63, "name", "Auto"),
            b.setting(64, 127, "name", "Sound"),
            b.setting(-128, -65, "name", "Manual"),
            b.setting(-64, -1, "name", "Blackout"),
        ];
        caps.push(b.capability("LXCustomCapability", 0, Some("Mode"), parts));
        let s = b.setting(0, -1, "name", "Closed > Open");
        caps.push(b.capability("LXCustomCapability", 1, Some("Master Dimmer"), vec![s]));
        for (ch, c) in [(3, "Red"), (2, "Green"), (4, "Blue")] {
            let s = b.setting(0, -1, "componentName", c);
            caps.push(b.capability("LXColorComponentCapability", ch, None, vec![s]));
        }
        let s = b.setting(0, -1, "name", "Slow > Fast");
        caps.push(b.capability("LXSpinCapability", 5, Some("Speed"), vec![s]));
        caps.push(b.capability("LXColorComponentCapability", 9, None, vec![]));
        let set = b.object("NSSet", vec![("NS.objects", Value::Array(caps))]);
        let mode = b.object(
            "LXPersonality",
            vec![("footprint", Value::Integer(6)), ("customName", Value::Uid(0)), ("capabilities", set)],
        );
        let modes = b.list(vec![mode]);
        let name = b.text("Test Wash 6");
        let profile = b.object("LXFixtureProfile", vec![("name", name), ("personalities", modes)]);
        b.archive(profile)
    }

    #[test]
    fn a_profile_becomes_a_fixture_type_with_named_ranges() {
        let ty = read_profile(&wash());
        assert!(ty.is_ok(), "{:?}", ty.err());
        let Ok(ty) = ty else { return };
        assert_eq!(ty.name, "Test Wash 6");
        let mode = &ty.modes[0];
        assert_eq!(mode.name, "6 channels");
        assert_eq!(
            mode.channels,
            vec![
                Attribute::Other,
                Attribute::Intensity,
                Attribute::Green,
                Attribute::Red,
                Attribute::Blue,
                Attribute::Other
            ]
        );
        let ranges: Vec<(u16, u8, u8, &str)> =
            mode.ranges.iter().map(|r| (r.channel, r.from, r.to, r.name.as_str())).collect();
        assert_eq!(
            ranges,
            vec![
                (1, 0, 63, "Mode: Auto"),
                (1, 64, 127, "Mode: Sound"),
                (1, 128, 191, "Mode: Manual"),
                (1, 192, 255, "Mode: Blackout"),
                (2, 0, 255, "Master Dimmer: Closed > Open"),
                (6, 0, 255, "Speed: Slow > Fast"),
            ],
            "the colours have no named parts; a capability past the last channel is left out"
        );
    }

    #[test]
    fn other_files_are_refused() {
        for bad in [b"".to_vec(), b"<!DOCTYPE html><html>Not found</html>".to_vec(), b"bplist00".to_vec()] {
            assert!(read_profile(&bad).is_err());
        }
        // A property list that is not a profile, and a profile without a mode it can use.
        assert!(read_profile(&write(&Value::Dict(vec![("a".into(), Value::Integer(1))]))).is_err());
        let mut b = Builder::new();
        let mode = b.object("LXPersonality", vec![("footprint", Value::Integer(999))]);
        let modes = b.list(vec![mode]);
        let name = b.text("Too Big");
        let profile = b.object("LXFixtureProfile", vec![("name", name), ("personalities", modes)]);
        assert!(read_profile(&b.archive(profile)).is_err());
        // References that point nowhere or at themselves are only followed one step.
        let mut b = Builder::new();
        let profile = b.object("LXFixtureProfile", vec![("name", Value::Uid(999)), ("personalities", Value::Uid(2))]);
        assert!(read_profile(&b.archive(profile)).is_err());
    }

    /// The owner's case: a profile file brought in through the import, then patched.
    #[test]
    fn a_profile_is_imported_and_patched() {
        let mut e = Engine::new(Box::new(crate::MemoryServices::default()));
        let _ = e.services_mut().write_file("/f/Test Wash.lightkeyfxt", &wash());
        assert_eq!(
            e.run("fixture_type.import", &json!({"path": "/f/Test Wash.lightkeyfxt"})).ok(),
            Some(json!({"type": 0, "name": "Test Wash 6", "modes": ["6 channels"]}))
        );
        assert!(e.run("fixture.add", &json!({"type": 0, "universe": 1, "address": 1})).is_ok());
        assert_eq!(e.show.fixtures.first().map(|f| f.kind.as_str()), Some("Test Wash 6 \u{b7} 6 channels"));
        let _ = e.services_mut().write_file("/f/page.lightkeyfxt", b"<!DOCTYPE html>");
        assert!(e.run("fixture_type.import", &json!({"path": "/f/page.lightkeyfxt"})).is_err());
    }
}
