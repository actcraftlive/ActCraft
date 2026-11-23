//! Window tests through the accessibility tree (egui_kittest): controls are found by role and
//! name, driven by keyboard or the accessibility action a screen reader would send, and the
//! engine is checked afterwards. The harness's own accessibility check stays on.

use std::sync::{Arc, Mutex};

use actcraft_engine::{Engine, MemoryServices};
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;

use crate::{ActCraftApp, PlatformInfo};

/// The window over an engine the test can look into.
fn window(setup: impl FnOnce(&mut Engine)) -> (Harness<'static, ActCraftApp>, Arc<Mutex<Engine>>) {
    let mut engine = Engine::new(Box::new(MemoryServices::default()));
    setup(&mut engine);
    let engine = Arc::new(Mutex::new(engine));
    let app = ActCraftApp::new(&egui::Context::default(), Arc::clone(&engine), PlatformInfo::default());
    let harness = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .build_ui_state(|ui, app: &mut ActCraftApp| app.ui(ui), app);
    (harness, engine)
}

/// A few frames: the window redraws on a timer, so it never settles by itself.
fn frames(h: &mut Harness<'static, ActCraftApp>, n: usize) {
    for _ in 0..n {
        h.step();
    }
}

fn with<T>(engine: &Arc<Mutex<Engine>>, f: impl FnOnce(&mut Engine) -> T) -> T {
    let mut guard = match engine.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    f(&mut guard)
}

#[test]
fn a_toggle_pad_says_its_state_and_switches_by_its_action() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("pad.add", &json!({"label": "Click", "toggle": "metronome"}));
    });
    frames(&mut h, 3);
    let on = with(&engine, |e| e.show.metronome.enabled);
    let before = if on { "Click \u{b7} on" } else { "Click \u{b7} off" };
    h.get_by_role_and_label(Role::Button, before).click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.show.metronome.enabled), !on, "pressed through its accessibility action");
    let after = if on { "Click \u{b7} off" } else { "Click \u{b7} on" };
    assert!(h.query_by_role_and_label(Role::Button, after).is_some(), "the name says the new state");
}

#[test]
fn the_dmx_monitor_sets_a_level_from_its_slider() {
    let (mut h, engine) = window(|_| {});
    h.state_mut().show_dmx_monitor();
    frames(&mut h, 3);
    // Channel 1 is picked; the Level slider sets it.
    h.get_by_role_and_label(Role::Slider, "Level").set_value_accesskit(200.0);
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.dmx.universe(1).map(|u| u[0])), Some(200));
    assert!(h.query_by_role_and_label(Role::Button, "Channel 1: 200").is_some(), "the cell's name carries the level");
}

#[test]
fn a_cue_list_goes_by_its_go_button_and_says_where_it_is() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 1.0}}));
        let _ = e.run("preset.store", &json!({"name": "Full"}));
        let _ = e.run("cue_list.add", &json!({"name": "Main"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0}));
    });
    h.state_mut().show_presets();
    frames(&mut h, 3);
    assert!(h.query_by_label("Not started").is_some());
    h.get_by_role_and_label(Role::Button, "Go").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.cue_position(0)), Some(0));
    assert!(h.query_by_label("At cue 1 of 1: Full").is_some());
}

#[test]
fn a_preset_name_typed_by_keyboard_enables_storing_it() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
    });
    h.state_mut().show_presets();
    frames(&mut h, 3);
    let field = h.get_by_role_and_label(Role::TextInput, "Preset name");
    field.focus();
    frames(&mut h, 1);
    h.get_by_role_and_label(Role::TextInput, "Preset name").type_text("Warm");
    frames(&mut h, 3);
    assert!(h.get_by_role_and_label(Role::TextInput, "Preset name").is_focused(), "focus stays in the field");
    h.get_by_role_and_label(Role::Button, "Store preset").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.show.presets.first().map(|p| p.name.clone())), Some("Warm".to_owned()));
}

#[test]
fn a_song_gets_a_lighting_cue_from_its_lighting_tab() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("song.add", &serde_json::Value::Null);
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        let _ = e.run("preset.store", &json!({"name": "Dark"}));
        let _ = e.run("cue_list.add", &json!({"name": "Main"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0}));
    });
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::Button, "Light cues").click_accesskit();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::SpinButton, "At").set_value_accesskit(12.5);
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Add lighting cue").click_accesskit();
    frames(&mut h, 3);
    let cues = with(&engine, |e| e.current().map(|s| s.light_cues.clone()).unwrap_or_default());
    assert_eq!(cues.iter().map(|c| (c.time, c.list.as_str(), c.cue)).collect::<Vec<_>>(), vec![(12.5, "Main", 0)]);
    assert!(h.query_by_label("0:12.5").is_some(), "the list shows its time");
}

#[test]
fn an_effect_is_saved_and_started_from_the_presets_window() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "rgb", "universe": 1, "count": 2}));
        let _ = e.run("group.set", &json!({"name": "Pair", "fixtures": [0, 1]}));
    });
    h.state_mut().show_presets();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::TextInput, "Effect name").focus();
    frames(&mut h, 1);
    h.get_by_role_and_label(Role::TextInput, "Effect name").type_text("Pulse");
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Save effect").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(
        with(&engine, |e| e.show.effects.first().map(|f| (f.name.clone(), f.group.clone()))),
        Some(("Pulse".to_owned(), "Pair".to_owned()))
    );
    h.get_by_role_and_label(Role::Button, "Pulse").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.running_effects()), vec!["Pulse".to_owned()]);
    assert!(h.query_by_role_and_label(Role::Button, "Pulse \u{b7} running").is_some(), "its name says it runs");
}

#[test]
fn a_fixture_is_picked_and_placed_on_the_stage_by_numbers() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "rgb", "universe": 1, "name": "Spot"}));
        let _ = e.run("fixture.set", &json!({"fixture": 0, "values": {"red": 1.0}}));
    });
    h.state_mut().show_stage_view();
    frames(&mut h, 3);
    // The fixture is a button that says the colour it shows.
    h.get_by_role_and_label(Role::Button, "Spot: red 100 %, green 0 %, blue 0 %").click_accesskit();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::SpinButton, "Left to right").set_value_accesskit(0.2);
    frames(&mut h, 3);
    let place = with(&engine, |e| e.show.fixtures.first().and_then(|f| f.place));
    assert!(place.is_some_and(|[x, _]| (x - 0.2).abs() < 1e-6), "{place:?}");
}

#[test]
fn blackout_is_switched_from_the_presets_window_and_says_its_state() {
    let (mut h, engine) = window(|_| {});
    h.state_mut().show_presets();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::Button, "Blackout \u{b7} off").click_accesskit();
    frames(&mut h, 3);
    assert!(with(&engine, |e| e.dmx.blackout));
    assert!(h.query_by_role_and_label(Role::Button, "Blackout \u{b7} on").is_some());
    h.get_by_role_and_label(Role::Slider, "Master").set_value_accesskit(0.25);
    frames(&mut h, 3);
    assert!(with(&engine, |e| (e.dmx.master - 0.25).abs() < 1e-9));
}

#[test]
fn a_ready_made_effect_fills_the_effect_form() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "moving_head", "universe": 1, "count": 2}));
        let _ = e.run("group.set", &json!({"name": "Heads", "fixtures": [0, 1]}));
    });
    h.state_mut().show_presets();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::ComboBox, "Start from").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("Fan out").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Save effect").click_accesskit();
    frames(&mut h, 3);
    let fx = with(&engine, |e| e.show.effects.first().cloned());
    assert_eq!(
        fx.map(|f| (f.name, f.attribute)),
        Some(("Fan out".to_owned(), actcraft_engine::fixtures::Attribute::Pan))
    );
}

#[test]
fn blind_is_switched_on_says_so_where_fixtures_change_and_is_left_from_there() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1, "name": "Front"}));
    });
    h.state_mut().show_presets();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::Button, "Blind \u{b7} off").click_accesskit();
    frames(&mut h, 3);
    assert!(with(&engine, |e| e.dmx.blind()));
    assert!(h.query_by_role_and_label(Role::Button, "Blind \u{b7} on").is_some());
    with(&engine, |e| e.run("fixture.set", &json!({"fixture": 0, "values": {"intensity": 1.0}})).is_ok());
    assert_eq!(with(&engine, |e| e.dmx.universe(1).map(|u| u[0])), Some(0), "the light stays dark");
    h.state_mut().show_fixtures();
    frames(&mut h, 3);
    assert!(h.query_by_label_contains("Blind is on").is_some());
    h.get_by_role_and_label(Role::Button, "Leave blind").click_accesskit();
    frames(&mut h, 3);
    assert!(!with(&engine, |e| e.dmx.blind()));
    assert!(h.query_by_label_contains("Blind is on").is_none());
}

#[test]
fn pad_pages_are_gone_through_and_a_pad_is_moved_to_another_page() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("pad.add", &json!({"label": "Intro", "command": "metronome.toggle"}));
        let _ = e.run("pad.add", &json!({"label": "Outro", "command": "metronome.toggle", "page": 1}));
    });
    frames(&mut h, 3);
    assert!(h.query_by_label("Page 1 of 2").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Intro").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Outro").is_none(), "on the other page");
    h.get_by_role_and_label(Role::Button, "Next page").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.pad_page()), 1);
    assert!(h.query_by_role_and_label(Role::Button, "Outro").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Intro").is_none());
    // Editing: the pad's own buttons say which pad they act on.
    h.get_by_role_and_label(Role::Button, "Edit pads").click_accesskit();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::ComboBox, "Page (Outro)").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("Page 1").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.show.pads.iter().map(|p| p.page).collect::<Vec<_>>()), vec![0, 0]);
    h.get_by_role_and_label(Role::Button, "Page before").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.pad_page()), 0);
    assert!(h.query_by_role_and_label(Role::Button, "Outro").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Remove (Outro)").is_some());
}

#[test]
fn a_lighting_cue_shows_on_the_lane_with_its_bar_and_moves_by_beats() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("song.add", &serde_json::Value::Null);
        let _ = e.run("song.set", &json!({"bpm": 120.0, "beats_per_bar": 4, "beat_anchor": 0.0}));
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        let _ = e.run("preset.store", &json!({"name": "Dark"}));
        let _ = e.run("cue_list.add", &json!({"name": "Main"}));
        let _ = e.run("cue.add", &json!({"list": 0, "preset": 0, "name": "Intro"}));
        let _ = e.run("song.light_cue_add", &json!({"bar": 3, "list": 0, "cue": 0}));
    });
    frames(&mut h, 3);
    h.state_mut().show_light_cues();
    frames(&mut h, 3);
    // Bar 3 at 120 BPM in 4/4 is 4 s in.
    assert!(h.query_by_role_and_label(Role::Canvas, "Lighting lane").is_some());
    h.get_by_role_and_label(Role::Button, "1. Intro, Bar 3.1, 0:04.0").click_accesskit();
    frames(&mut h, 3);
    assert!(h.query_by_label("Bar 3.1").is_some(), "the list says the bar");
    h.get_by_role_and_label(Role::Button, "Beat later (1. Intro)").click_accesskit();
    frames(&mut h, 3);
    let time = with(&engine, |e| e.current().and_then(|s| s.light_cues.first()).map(|c| c.time));
    assert_eq!(time, Some(4.5));
    assert!(h.query_by_label("Bar 3.2").is_some());
    h.get_by_role_and_label(Role::Button, "Remove (1. Intro)").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.current().map(|s| s.light_cues.len())), Some(0));
}

#[test]
fn a_circle_effect_brings_its_second_attribute_and_another_can_be_added() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "moving_head", "universe": 1}));
        let _ = e.run("group.set", &json!({"name": "Head", "fixtures": [0]}));
    });
    h.state_mut().show_presets();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::ComboBox, "Start from").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("Circle").click_accesskit();
    frames(&mut h, 2);
    // The template's tilt shows as a second attribute with its own, named controls.
    assert!(h.query_by_role_and_label(Role::ComboBox, "Also moves (1)").is_some());
    assert!(h.query_by_role_and_label(Role::Slider, "Later in the cycle (also 1)").is_some());
    h.get_by_role_and_label(Role::Button, "Move another attribute too").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Remove (also 2)").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Save effect").click_accesskit();
    frames(&mut h, 3);
    let fx = with(&engine, |e| e.show.effects.first().cloned());
    assert_eq!(
        fx.map(|f| (f.attribute, f.also.iter().map(|p| (p.attribute, p.offset)).collect::<Vec<_>>())),
        Some((actcraft_engine::fixtures::Attribute::Pan, vec![(actcraft_engine::fixtures::Attribute::Tilt, 0.25)]))
    );
}

#[test]
fn a_moving_light_on_the_stage_says_where_it_points() {
    let (mut h, _engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "moving_head", "universe": 1, "name": "Head"}));
        // Panned a sixth of 540° (90°) and tilted a sixth of 270° (45°) forward.
        let _ = e.run(
            "fixture.set",
            &json!({"fixture": 0, "values": {"pan": 0.5 + 1.0 / 6.0, "tilt": 0.5 + 1.0 / 6.0, "intensity": 1.0}}),
        );
    });
    h.state_mut().show_stage_view();
    frames(&mut h, 3);
    assert!(
        h.query_by_label_contains("aimed 90° from the audience, 71 % out").is_some(),
        "the fixture's name says where its beam goes"
    );
}

#[test]
fn a_long_message_in_the_status_line_stays_clear_of_the_notes_beside_it() {
    let long = "Could not send DMX: USB DMX on COM250: the system cannot find the file specified (os error 2), \
                so nothing goes to the lights on this output until the interface is plugged in again";
    let (mut h, _engine) = window(|e| {
        let _ = e.run("song.add", &serde_json::Value::Null);
        e.say_error(long);
    });
    // As on a real machine: an audio device and the remote control address take room on the left.
    h.state_mut().platform.audio_device = Some("OUT 1-8 (a long audio interface name)".to_owned());
    h.state_mut().platform.sample_rate = 48_000;
    h.state_mut().platform.control_address = Some("127.0.0.1:47380".to_owned());
    frames(&mut h, 3);
    let note = h.get_by_label("Unsaved changes").rect();
    let message = h.get_by_label(long).rect();
    assert!(message.left() >= note.right(), "message {message:?} runs over the note {note:?}");
}

#[test]
fn a_usb_output_is_added_by_picking_its_port() {
    let (mut h, engine) = window(|_| {});
    h.state_mut().open_settings_at("lighting");
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::ComboBox, "Protocol").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("USB interface").click_accesskit();
    frames(&mut h, 2);
    // Asking for the ports goes to the host, which fills them in.
    h.get_by_role_and_label(Role::Button, "Find USB interfaces").click_accesskit();
    frames(&mut h, 2);
    assert!(h.state_mut().take_usb_find_request());
    h.state_mut().platform.serial_ports = Some(vec!["COM7".to_owned()]);
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::ComboBox, "Port").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("COM7").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Add output").click_accesskit();
    frames(&mut h, 3);
    let out = with(&engine, |e| e.show.dmx_outputs.first().cloned());
    assert_eq!(
        out.map(|o| (o.protocol, o.address)),
        Some((actcraft_engine::dmx::DmxProtocol::UsbPro, "COM7".to_owned()))
    );
}
