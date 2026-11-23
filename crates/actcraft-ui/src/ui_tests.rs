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
    let field = h.get_by_role_and_label(Role::TextInput, "Name");
    field.focus();
    frames(&mut h, 1);
    h.get_by_role_and_label(Role::TextInput, "Name").type_text("Warm");
    frames(&mut h, 3);
    assert!(h.get_by_role_and_label(Role::TextInput, "Name").is_focused(), "focus stays in the field");
    h.get_by_role_and_label(Role::Button, "Store preset").click_accesskit();
    frames(&mut h, 3);
    assert_eq!(with(&engine, |e| e.show.presets.first().map(|p| p.name.clone())), Some("Warm".to_owned()));
}
