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

#[test]
fn dmx_from_other_gear_is_switched_on_in_settings_and_shown_in_the_monitor() {
    let (mut h, _engine) = window(|e| {
        e.dmx_in(3, &[200, 0, 9]);
    });
    h.state_mut().open_settings_at("lighting");
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::CheckBox, "Receive DMX over Art-Net and sACN").click_accesskit();
    frames(&mut h, 2);
    assert_eq!(h.state_mut().take_dmx_in_request(), Some(Some(false)), "listen, this computer only");
    h.state_mut().show_dmx_monitor();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::ComboBox, "Show").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("DMX in, Art-Net 3 / sACN 4").click_accesskit();
    frames(&mut h, 3);
    assert!(h.query_by_label_contains("Channel 1: 200").is_some(), "the heard level shows");
    assert!(h.query_by_role_and_label(Role::Button, "This universe to zero").is_none(), "read only");
}

#[test]
fn a_dmx_trigger_is_added_and_removed_in_settings() {
    let (mut h, engine) = window(|_| {});
    h.state_mut().open_settings_at("lighting");
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::SpinButton, "Desk universe").set_value_accesskit(2.0);
    h.get_by_role_and_label(Role::SpinButton, "Desk channel").set_value_accesskit(7.0);
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::ComboBox, "Runs").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("Metronome on or off").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Add trigger").click_accesskit();
    frames(&mut h, 3);
    let got = with(&engine, |e| e.show.dmx_triggers.first().map(|t| (t.universe, t.channel, t.command.clone())));
    assert_eq!(got, Some((2, 7, "metronome.toggle".to_owned())));
    h.get_by_role_and_label(Role::Button, "Remove (Universe 2, channel 7)").click_accesskit();
    frames(&mut h, 3);
    assert!(with(&engine, |e| e.show.dmx_triggers.is_empty()));
}

#[test]
fn a_controller_button_is_learned_and_set_to_run_a_command() {
    let (mut h, engine) = window(|_| {});
    h.state_mut().open_settings_at("midi");
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::Button, "Learn a button").click_accesskit();
    frames(&mut h, 2);
    assert!(h.state_mut().gamepad_learning(), "waits for a button");
    // The host hands the next press over while learning.
    h.state_mut().gamepad_learned("start");
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::ComboBox, "The button runs").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("Metronome on or off").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Add button").click_accesskit();
    frames(&mut h, 3);
    let got = with(&engine, |e| e.show.gamepad_actions.first().map(|a| (a.button.clone(), a.command.clone())));
    assert_eq!(got, Some(("start".to_owned(), "metronome.toggle".to_owned())));
    assert!(h.query_by_role_and_label(Role::Button, "Remove (Start)").is_some(), "listed, named by its button");
}

#[test]
fn a_cue_that_goes_on_by_itself_is_added_from_the_presets_window() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "dimmer", "universe": 1}));
        let _ = e.run("preset.store", &json!({"name": "Dark"}));
        let _ = e.run("cue_list.add", &json!({"name": "Chase"}));
    });
    h.state_mut().show_presets();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::SpinButton, "Then on after").set_value_accesskit(0.5);
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Add cue").click_accesskit();
    frames(&mut h, 3);
    let follow = with(&engine, |e| e.show.cue_lists.first().and_then(|l| l.cues.first()).map(|c| c.follow));
    assert_eq!(follow, Some(0.5));
    assert!(h.query_by_label("then on after 0.5 s").is_some(), "the list says so");
}

#[test]
fn a_controller_trigger_is_set_to_move_the_lighting_master() {
    let (mut h, engine) = window(|_| {});
    h.state_mut().open_settings_at("midi");
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::ComboBox, "Stick or trigger").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("Right trigger").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::ComboBox, "Moves").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("Lighting master").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Add fader").click_accesskit();
    frames(&mut h, 3);
    let got = with(&engine, |e| e.show.gamepad_faders.first().map(|f| (f.axis.clone(), f.volume)));
    assert_eq!(got, Some(("right_trigger".to_owned(), None)));
    assert!(h.query_by_role_and_label(Role::Button, "Remove (Right trigger: Lighting master)").is_some());
    // Pulled half way, the lighting master follows.
    with(&engine, |e| e.gamepad_axis("right_trigger", 0.5));
    assert!(with(&engine, |e| (e.dmx.master - 0.5).abs() < 1e-9));
}

#[test]
fn a_hue_bridge_is_found_and_paired_in_settings() {
    use crate::HueRequest;
    let (mut h, _engine) = window(|_| {});
    h.state_mut().open_settings_at("lighting");
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::Button, "Find Hue bridges").click_accesskit();
    frames(&mut h, 2);
    assert_eq!(h.state_mut().take_hue_request(), Some(HueRequest::Find));
    // The host's answer: one bridge.
    h.state_mut().platform.hue_bridges = Some(Ok(vec!["192.168.1.5".to_owned()]));
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Pair (192.168.1.5)").click_accesskit();
    frames(&mut h, 2);
    assert_eq!(h.state_mut().take_hue_request(), Some(HueRequest::Pair("192.168.1.5".to_owned())));
    // Paired, with its lights read.
    h.state_mut().platform.hue_paired = Some("192.168.1.5".to_owned());
    h.state_mut().platform.hue_lights = Some(Ok(vec![("1".to_owned(), "Front".to_owned())]));
    frames(&mut h, 2);
    assert!(h.query_by_label("Lights: 1: Front").is_some());
    h.get_by_role_and_label(Role::Button, "Forget this bridge").click_accesskit();
    frames(&mut h, 2);
    assert_eq!(h.state_mut().take_hue_request(), Some(HueRequest::Forget));
}

#[test]
fn a_fixture_gets_a_hue_light_to_follow_it() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("fixture.add", &json!({"kind": "rgb", "universe": 1, "name": "Front"}));
    });
    h.state_mut().platform.hue_paired = Some("192.168.1.5".to_owned());
    h.state_mut().platform.hue_lights = Some(Ok(vec![("1".to_owned(), "Lamp".to_owned())]));
    h.state_mut().show_fixtures();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::ComboBox, "Hue light (Front)").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("1: Lamp").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.show.fixtures[0].hue.clone()), "1");
}

#[test]
fn band_hosting_shows_its_code_and_a_follower_asks_to_follow() {
    use crate::BandRequest;
    let (mut h, _engine) = window(|_| {});
    h.state_mut().open_settings_at("band");
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::CheckBox, "Let band members follow this computer").click_accesskit();
    frames(&mut h, 2);
    assert_eq!(h.state_mut().take_band_request(), Some(BandRequest::Host(true)));
    h.state_mut().platform.band_hosting = Some(("0.0.0.0:47390".to_owned(), "123456".to_owned(), 1));
    frames(&mut h, 2);
    assert!(h.query_by_label_contains("Code for a new follower: 123456").is_some());
    h.get_by_role_and_label(Role::TextInput, "Host address").focus();
    h.get_by_role_and_label(Role::TextInput, "Host address").type_text("192.168.1.20");
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::TextInput, "Code").focus();
    h.get_by_role_and_label(Role::TextInput, "Code").type_text("654321");
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Follow").click_accesskit();
    frames(&mut h, 2);
    assert_eq!(
        h.state_mut().take_band_request(),
        Some(BandRequest::Follow("192.168.1.20".to_owned(), "654321".to_owned()))
    );
    h.state_mut().platform.band_following = Some(("192.168.1.20".to_owned(), false));
    frames(&mut h, 2);
    assert!(h.query_by_label_contains("is not answering; playing on").is_some());
    h.get_by_role_and_label(Role::Button, "Stop following").click_accesskit();
    frames(&mut h, 2);
    assert_eq!(h.state_mut().take_band_request(), Some(BandRequest::Unfollow));
}

#[test]
fn a_tracks_colour_and_remove_buttons_say_which_track() {
    let (mut h, _engine) = window(|e| {
        let _ = e.run("song.add", &json!({"title": "Opener"}));
        let _ = e.run("track.add", &json!({"name": "Bass"}));
        let _ = e.run("track.add", &json!({"name": "Keys"}));
    });
    frames(&mut h, 3);
    for name in ["Bass", "Keys"] {
        assert!(h.query_by_role_and_label(Role::Button, &format!("Track colour ({name})")).is_some(), "{name}");
        assert!(h.query_by_role_and_label(Role::Button, &format!("Remove track ({name})")).is_some(), "{name}");
        assert!(h.query_by_role_and_label(Role::ComboBox, &format!("Output bus ({name})")).is_some(), "{name}");
        assert!(h.query_by_role_and_label(Role::Button, &format!("Mute group 1 ({name})")).is_some(), "{name}");
    }
}

#[test]
fn picked_songs_in_a_set_move_up_together() {
    let (mut h, engine) = window(|e| {
        for t in ["A", "B", "C"] {
            let _ = e.run("song.add", &json!({"title": t}));
        }
        let ids: Vec<u64> =
            ["A", "B", "C"].iter().filter_map(|t| e.show.songs.iter().find(|s| s.title == *t).map(|s| s.id)).collect();
        let _ = e.run("playlist.create", &json!({"name": "Gig"}));
        let _ = e.run("playlist.add_songs", &json!({"playlist": 0, "songs": ids}));
    });
    frames(&mut h, 3);
    // The playlist's songs show once it is opened.
    if let Some(n) = h.query_by_label_contains("Gig") {
        n.click_accesskit();
    }
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::CheckBox, "Pick (2. B)").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::CheckBox, "Pick (3. C)").click_accesskit();
    frames(&mut h, 2);
    assert!(h.query_by_label("2 picked:").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Up (2. B)").is_some(), "row buttons say which song");
    h.get_by_role_and_label(Role::Button, "Move picked up").click_accesskit();
    frames(&mut h, 3);
    let order = with(&engine, |e| {
        e.show.playlists[0].sets[0]
            .songs
            .iter()
            .filter_map(|id| e.show.song(*id).map(|s| s.title.clone()))
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec!["B", "C", "A"]);
}

#[test]
fn a_mixed_channel_is_set_by_picking_its_range() {
    let (mut h, engine) = window(|e| {
        let _ = e.run(
            "fixture_type.set",
            &json!({"name": "Spot", "modes": [{"channels": ["pan", "other", "red"], "ranges": [
                {"channel": 2, "from": 0, "to": 7, "name": "Off"},
                {"channel": 2, "from": 8, "to": 134, "name": "Dimmer"},
                {"channel": 2, "from": 240, "to": 255, "name": "Open"}]}]}),
        );
        let _ = e.run("fixture.add", &json!({"type": 0, "universe": 1, "address": 1, "name": "Head"}));
    });
    h.state_mut().show_fixtures();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::ComboBox, "Channel 2 (Head)").click_accesskit();
    frames(&mut h, 2);
    h.get_by_label("Open (240\u{2013}255)").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.dmx.universe(1).map(|u| u[1])), Some(240));
    h.get_by_role_and_label(Role::Slider, "Open (Head)").set_value_accesskit(1.0);
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.dmx.universe(1).map(|u| u[1])), Some(255), "through the range");
}

#[test]
fn a_song_gets_a_sheet_for_a_musician_from_the_sheet_music_tab() {
    let (mut h, engine) = window(|e| {
        let _ = e.run("song.add", &serde_json::Value::Null);
    });
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::Button, "Sheet music").click_accesskit();
    frames(&mut h, 3);
    // A musician first, from the Musicians list.
    h.get_by_label("Musicians (0)").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::TextInput, "New musician").focus();
    frames(&mut h, 1);
    h.get_by_role_and_label(Role::TextInput, "New musician").type_text("Kim");
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Add musician").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.show.musicians.clone()), vec!["Kim".to_owned()]);
    assert!(h.query_by_role_and_label(Role::Button, "Remove Kim").is_some(), "the remove button says whom");
    // Then a sheet, which is picked once added.
    h.get_by_role_and_label(Role::TextInput, "New sheet").focus();
    frames(&mut h, 1);
    h.get_by_role_and_label(Role::TextInput, "New sheet").type_text("Bass");
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Add sheet").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.current().map(|s| s.sheets.len())), Some(1));
    assert!(h.query_by_role_and_label(Role::TextInput, "Sheet name").is_some(), "the picked sheet's fields show");
    // It is for Kim.
    h.get_by_role_and_label(Role::ComboBox, "For").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Kim").click_accesskit();
    frames(&mut h, 3);
    let musician = with(&engine, |e| e.current().and_then(|s| s.sheets.first().map(|s| s.musician.clone())));
    assert_eq!(musician.as_deref(), Some("Kim"));
    // Back to the song's own sheet: the sheet's fields go.
    h.get_by_role_and_label(Role::ComboBox, "Sheet").click_accesskit();
    frames(&mut h, 2);
    h.get_by_role_and_label(Role::Button, "Main sheet").click_accesskit();
    frames(&mut h, 3);
    assert!(h.query_by_role_and_label(Role::TextInput, "Sheet name").is_none());
}

#[test]
fn sheets_in_the_browser_are_switched_on_from_the_band_tab() {
    let (mut h, _engine) = window(|_| {});
    h.state_mut().open_settings_at("band");
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::CheckBox, "Let musicians see their sheets in a browser").click_accesskit();
    frames(&mut h, 2);
    assert_eq!(h.state_mut().take_band_request(), Some(crate::BandRequest::Sheets(true)));
    // Once on, the tab says where musicians go and the code they type.
    h.state_mut().platform.sheets_web = Some(("http://192.168.1.20:47392".into(), "123456".into(), 2));
    frames(&mut h, 2);
    assert!(h.query_by_label("Open http://192.168.1.20:47392 \u{b7} code: 123456 \u{b7} pages open: 2").is_some());
    use egui_kittest::kittest::NodeT;
    let on = h.get_by_role_and_label(Role::CheckBox, "Let musicians see their sheets in a browser");
    assert_eq!(on.accesskit_node().toggled(), Some(egui::accesskit::Toggled::True));
}

#[test]
fn a_setlist_note_is_written_before_a_song_of_a_set() {
    let (mut h, engine) = window(|e| {
        for t in ["A", "B"] {
            let _ = e.run("song.add", &json!({"title": t}));
        }
        let ids: Vec<u64> = e.show.songs.iter().map(|s| s.id).collect();
        let _ = e.run("playlist.create", &json!({"name": "Gig"}));
        let _ = e.run("playlist.add_songs", &json!({"playlist": 0, "songs": ids}));
    });
    frames(&mut h, 3);
    if let Some(n) = h.query_by_label_contains("Gig") {
        n.click_accesskit();
    }
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::Button, "Note (2. B)").click_accesskit();
    frames(&mut h, 2);
    let field = h.get_by_role_and_label(Role::TextInput, "Note before (B)");
    assert!(field.is_focused(), "the field takes focus");
    field.type_text("Acoustic guitar");
    frames(&mut h, 2);
    h.key_press(egui::Key::Enter);
    frames(&mut h, 3);
    let notes = with(&engine, |e| e.show.playlists[0].sets[0].notes.clone());
    assert_eq!(notes.iter().map(|n| n.text.as_str()).collect::<Vec<_>>(), vec!["Acoustic guitar"]);
    assert!(h.query_by_label("Acoustic guitar").is_some(), "the note shows above the song");
    assert!(h.query_by_role_and_label(Role::TextInput, "Note before (B)").is_none(), "the field closes");
}

#[test]
fn a_fixture_type_keeps_and_gains_named_ranges_in_its_editor() {
    let (mut h, engine) = window(|e| {
        let mode = json!({"name": "3ch", "channels": ["intensity", "other", "red"],
            "ranges": [{"channel": 2, "from": 0, "to": 127, "name": "Off"}]});
        let _ = e.run("fixture_type.set", &json!({"name": "Spot", "modes": [mode]}));
    });
    h.state_mut().show_fixtures();
    h.state_mut().fixture_draft.kind = actcraft_engine::fixtures::KINDS.len();
    frames(&mut h, 3);
    h.get_by_role_and_label(Role::Button, "Change this fixture type").click_accesskit();
    frames(&mut h, 3);
    // Channel 2 says it has one range; its button opens them.
    h.get_by_role_and_label(Role::Button, "Ranges (1), Channel 2").click_accesskit();
    frames(&mut h, 3);
    assert!(h.query_by_label("Ranges of channel 2").is_some());
    assert!(h.query_by_role_and_label(Role::TextInput, "Name, Range 1").is_some());
    h.get_by_role_and_label(Role::Button, "Add range").click_accesskit();
    frames(&mut h, 3);
    assert!(h.query_by_role_and_label(Role::Button, "Remove (Range 2)").is_some(), "rows are named apart");
    h.get_by_role_and_label(Role::Button, "Save fixture type").click_accesskit();
    frames(&mut h, 3);
    let ranges = with(&engine, |e| e.show.fixture_types[0].modes[0].ranges.clone());
    let got: Vec<_> = ranges.iter().map(|r| (r.channel, r.from, r.to, r.name.as_str())).collect();
    assert_eq!(got, vec![(2, 0, 127, "Off"), (2, 128, 255, "Range 2")], "the old range kept, the new one after it");
}
