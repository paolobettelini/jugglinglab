use crate::canvas::{self, RenderSettings};
use juggling_core::animation::{AnimationKind, AnimationSpec, TransitionKind};
use juggling_core::jml::{self, PatternRecord};
use juggling_core::ladder::{
    LadderDiagram, LadderEdge, LadderEndpoint, LadderEvent, LadderHand, LadderPosition,
    LadderTransition, build_ladder_diagram,
};
use juggling_core::mhn_body::BodyPosition;
use juggling_core::mhn_jml::{MhnJmlEvent, MhnJmlPattern, MhnJmlProp, MhnJmlTransitionType};
use juggling_core::mhn_matrix::MhnMatrix;
use juggling_core::prop::PropSpec;
use juggling_core::{library, siteswap};
use leptos::ev;
use leptos::prelude::*;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{
    Blob, BlobPropertyBag, Event, FileReader, HtmlAnchorElement, HtmlCanvasElement,
    HtmlInputElement, window,
};

const THEME_STORAGE_KEY: &str = "jugglinglab.theme";
const DEFAULT_THEME: &str = "midnight";
const LADDER_TOP_Y: f64 = 8.0;
const LADDER_HEIGHT: f64 = 86.0;
const PATTERN_SOURCE_BASE: &str = "base";
const PATTERN_SOURCE_JML: &str = "jml";
const HISTORY_LIMIT: usize = 64;

#[derive(Clone, Debug, PartialEq)]
struct EditorSnapshot {
    records: Vec<PatternRecord>,
    selected: usize,
    pattern_source: String,
    pattern_text: String,
    draft: String,
    selected_ladder: String,
}

#[derive(Clone, Debug, PartialEq)]
struct LadderDrag {
    kind: LadderDragKind,
    pointer_id: i32,
    selected_id: String,
    start_time: f64,
    preview_time: f64,
    was_selected: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct LadderInsertTarget {
    juggler: usize,
    time: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct LadderContextMenu {
    x: f64,
    y: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct PositionCanvasDrag {
    hit: canvas::PositionEditorHit,
    start_client_x: f64,
    start_client_y: f64,
    start_position: BodyPosition,
    original_record: PatternRecord,
    checkpointed: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct DefineThrowDraft {
    event_index: usize,
    transition_index: usize,
    selected_id: String,
    throw_type: String,
    throw_mod: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct DefinePropDraft {
    path: usize,
    selected_id: String,
    prop_assignment: Vec<usize>,
    playback_time: f64,
    prop_type: String,
    prop_mod: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum LadderDragKind {
    Event {
        primary_index: usize,
        primary_time: f64,
    },
    Position(usize),
    Tracker {
        was_playing: bool,
        prop_cycle: i64,
    },
}

#[component]
pub fn App() -> impl IntoView {
    let initial_records = library::builtin_records();
    let first_playable = initial_records
        .iter()
        .position(PatternRecord::is_playable)
        .unwrap_or(0);
    let initial_draft = initial_records
        .get(first_playable)
        .and_then(|record| record.config.clone())
        .unwrap_or_else(|| "pattern=3".to_string());

    let (records, set_records) = signal(initial_records);
    let (selected, set_selected) = signal(first_playable);
    let (active_tab, set_active_tab) = signal("entry".to_string());
    let (view_mode, set_view_mode) = signal("edit".to_string());
    let (theme, set_theme) = signal(initial_theme());
    let (playing, set_playing) = signal(true);
    let (playhead_time, set_playhead_time) = signal(0.0);
    let (speed, set_speed) = signal(1.0);
    let (zoom, set_zoom) = signal(1.15);
    let (camera_yaw, set_camera_yaw) = signal(0.18);
    let (camera_pitch, set_camera_pitch) = signal(std::f64::consts::FRAC_PI_2);
    let (camera_pan_x, set_camera_pan_x) = signal(0.0);
    let (camera_pan_y, set_camera_pan_y) = signal(0.0);
    let (camera_pan_z, set_camera_pan_z) = signal(0.0);
    let (show_trails, set_show_trails) = signal(true);
    let (show_grid, set_show_grid) = signal(true);
    let (draft, set_draft) = signal(initial_draft);
    let (pattern_text, set_pattern_text) = signal(String::new());
    let (pattern_source, set_pattern_source) = signal(PATTERN_SOURCE_BASE.to_string());
    let (selected_object, set_selected_object) = signal(String::new());
    let (selected_ladder, set_selected_ladder) = signal(String::new());
    let (ladder_drag, set_ladder_drag) = signal(None::<LadderDrag>);
    let (ladder_insert_target, set_ladder_insert_target) = signal(None::<LadderInsertTarget>);
    let (ladder_context_menu, set_ladder_context_menu) = signal(None::<LadderContextMenu>);
    let (ladder_popup_was_playing, set_ladder_popup_was_playing) = signal(None::<bool>);
    let (ladder_prop_edit_time, set_ladder_prop_edit_time) = signal(0.0);
    let (define_throw_dialog, set_define_throw_dialog) = signal(None::<DefineThrowDraft>);
    let (define_prop_dialog, set_define_prop_dialog) = signal(None::<DefinePropDraft>);
    let (undo_stack, set_undo_stack) = signal(Vec::<EditorSnapshot>::new());
    let (redo_stack, set_redo_stack) = signal(Vec::<EditorSnapshot>::new());
    let (view_drag_start, set_view_drag_start) = signal(None::<(f64, f64)>);
    let (position_canvas_drag, set_position_canvas_drag) = signal(None::<PositionCanvasDrag>);
    let (view_dragged, set_view_dragged) = signal(false);
    let (pressed_camera_keys, set_pressed_camera_keys) = signal(Vec::<String>::new());
    let (status, set_status) = signal("Ready".to_string());

    Effect::new(move |_| {
        let theme_value = theme.get();
        let theme_value = if is_known_theme(&theme_value) {
            theme_value
        } else {
            DEFAULT_THEME.to_string()
        };
        if let Some(document) = window().and_then(|win| win.document()) {
            if let Some(root) = document.document_element() {
                root.set_attribute("data-theme", &theme_value).ok();
            }
        }
        save_theme(&theme_value);
    });

    let current_record = Memo::new(move |_| {
        records.with(|records| {
            records
                .get(selected.get())
                .cloned()
                .or_else(|| records.iter().find(|record| record.is_playable()).cloned())
        })
    });

    let current_spec = Memo::new(move |_| {
        current_record
            .get()
            .and_then(|record| AnimationSpec::from_record(&record).ok())
            .unwrap_or_else(AnimationSpec::fallback)
    });

    Effect::new(move |_| {
        let settings = RenderSettings {
            theme: theme.get(),
            speed: speed.get(),
            zoom: zoom.get(),
            camera_yaw: camera_yaw.get(),
            camera_pitch: camera_pitch.get(),
            camera_pan_x: camera_pan_x.get(),
            camera_pan_y: camera_pan_y.get(),
            camera_pan_z: camera_pan_z.get(),
            paused: !playing.get() || view_drag_start.get().is_some(),
            show_trails: show_trails.get(),
            show_grid: show_grid.get(),
            selected_position: selected_ladder_position_index(
                &current_spec.get(),
                &selected_ladder.get(),
            ),
        };
        canvas::start_by_id("juggling-stage", current_spec.get(), settings);
    });

    let seek_renderer = move |time: f64| {
        let spec = current_spec.get_untracked();
        canvas::set_playback_time(&spec, time);
        canvas::start_by_id(
            "juggling-stage",
            spec,
            RenderSettings {
                theme: theme.get_untracked(),
                speed: speed.get_untracked(),
                zoom: zoom.get_untracked(),
                camera_yaw: camera_yaw.get_untracked(),
                camera_pitch: camera_pitch.get_untracked(),
                camera_pan_x: camera_pan_x.get_untracked(),
                camera_pan_y: camera_pan_y.get_untracked(),
                camera_pan_z: camera_pan_z.get_untracked(),
                paused: true,
                show_trails: show_trails.get_untracked(),
                show_grid: show_grid.get_untracked(),
                selected_position: selected_ladder_position_index(
                    &current_spec.get_untracked(),
                    &selected_ladder.get_untracked(),
                ),
            },
        );
    };

    Effect::new(move |_| {
        if let Some(record) = current_record.get() {
            let requested_source = pattern_source.get();
            let source = if requested_source == PATTERN_SOURCE_BASE && record.config.is_none() {
                PATTERN_SOURCE_JML
            } else {
                requested_source.as_str()
            };
            if source != requested_source {
                set_pattern_source.set(source.to_string());
            }
            set_pattern_text.set(record_text_for_source(&record, source));
        }
    });

    {
        let tick = Closure::wrap(Box::new(move || {
            let keys = pressed_camera_keys.get_untracked();
            set_playhead_time.set(canvas::playback_time(&current_spec.get_untracked()));
            if keys.is_empty() {
                return;
            }

            let fast = keys.iter().any(|key| key == "shift");
            let step = if fast { 8.0 } else { 3.0 };
            let yaw = camera_yaw.get_untracked();
            let pitch = camera_pitch.get_untracked();
            let forward_x = -yaw.sin() * pitch.cos();
            let forward_y = -yaw.cos() * pitch.cos();
            let forward_z = pitch.sin();
            let right_x = -yaw.cos();
            let right_y = yaw.sin();
            let mut dx = 0.0;
            let mut dy = 0.0;
            let mut dz = 0.0;

            if keys.iter().any(|key| key == "w" || key == "arrowup") {
                dx += forward_x * step;
                dy += forward_y * step;
                dz += forward_z * step;
            }
            if keys.iter().any(|key| key == "s" || key == "arrowdown") {
                dx -= forward_x * step;
                dy -= forward_y * step;
                dz -= forward_z * step;
            }
            if keys.iter().any(|key| key == "a" || key == "arrowleft") {
                dx -= right_x * step;
                dy -= right_y * step;
            }
            if keys.iter().any(|key| key == "d" || key == "arrowright") {
                dx += right_x * step;
                dy += right_y * step;
            }
            if keys.iter().any(|key| key == "q") {
                dz -= step;
            }
            if keys.iter().any(|key| key == "e") {
                dz += step;
            }

            if dx != 0.0 || dy != 0.0 || dz != 0.0 {
                set_camera_pan_x.update(|value| *value += dx);
                set_camera_pan_y.update(|value| *value += dy);
                set_camera_pan_z.update(|value| *value += dz);
            }
        }) as Box<dyn FnMut()>);

        if let Some(window) = window() {
            window
                .set_interval_with_callback_and_timeout_and_arguments_0(
                    tick.as_ref().unchecked_ref(),
                    16,
                )
                .ok();
        }
        tick.forget();
    }

    let checkpoint_editor = move || {
        push_editor_history(
            records,
            selected,
            pattern_source,
            pattern_text,
            draft,
            selected_ladder,
            set_undo_stack,
            set_redo_stack,
        );
    };

    let commit_ladder_record = move |edited: PatternRecord| {
        checkpoint_editor();
        replace_current_ladder_record(
            edited,
            selected,
            set_selected,
            set_records,
            set_pattern_source,
            set_pattern_text,
            set_draft,
        );
    };

    let perform_undo = move || {
        let mut previous = None;
        set_undo_stack.update(|stack| {
            previous = stack.pop();
        });
        let Some(snapshot) = previous else {
            set_status.set("Nothing to undo".to_string());
            return;
        };
        push_redo_snapshot(
            records,
            selected,
            pattern_source,
            pattern_text,
            draft,
            selected_ladder,
            set_redo_stack,
        );
        restore_editor_snapshot(
            snapshot,
            set_records,
            set_selected,
            set_pattern_source,
            set_pattern_text,
            set_draft,
            set_selected_ladder,
        );
        set_status.set("Undo".to_string());
    };

    let undo_edit = move |_| perform_undo();

    let perform_redo = move || {
        let mut next = None;
        set_redo_stack.update(|stack| {
            next = stack.pop();
        });
        let Some(snapshot) = next else {
            set_status.set("Nothing to redo".to_string());
            return;
        };
        push_undo_snapshot(
            records,
            selected,
            pattern_source,
            pattern_text,
            draft,
            selected_ladder,
            set_undo_stack,
        );
        restore_editor_snapshot(
            snapshot,
            set_records,
            set_selected,
            set_pattern_source,
            set_pattern_text,
            set_draft,
            set_selected_ladder,
        );
        set_status.set("Redo".to_string());
    };

    let redo_edit = move |_| perform_redo();

    {
        let keydown = Closure::wrap(Box::new(move |event: ev::KeyboardEvent| {
            if !(event.ctrl_key() || event.meta_key()) || editor_shortcut_target_is_editable(&event)
            {
                return;
            }
            let key = event.key().to_ascii_lowercase();
            if key == "z" && event.shift_key() || key == "y" {
                event.prevent_default();
                perform_redo();
            } else if key == "z" {
                event.prevent_default();
                perform_undo();
            }
        }) as Box<dyn FnMut(ev::KeyboardEvent)>);
        if let Some(window) = window() {
            window
                .add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref())
                .ok();
        }
        keydown.forget();
    }

    let import_jml = move |xml: String| match jml::parse_jml(&xml) {
        Ok(library) => {
            let added = library.records.len();
            if added > 0 {
                checkpoint_editor();
            }
            set_records.update(|records| {
                let insert_at = records.len();
                records.extend(library.records);
                if added > 0 {
                    set_selected.set(insert_at);
                }
            });
            set_status.set(format!("Imported {added} JML lines"));
            set_pattern_text.set(String::new());
        }
        Err(err) => set_status.set(err),
    };

    let run_pattern = move |_| {
        let config = draft.get_untracked();
        match record_from_config_or_current_jml(&config, current_record.get_untracked()) {
            Ok((record, message)) => {
                checkpoint_editor();
                set_records.update(|records| {
                    records.push(record);
                    set_selected.set(records.len() - 1);
                });
                set_status.set(message);
                set_pattern_text.set(config);
            }
            Err(err) => set_status.set(err),
        }
    };

    let select_library_pattern = move |event: ev::Event| {
        let Ok(idx) = event_target_value(&event).parse::<usize>() else {
            return;
        };
        let Some(record) = records.with_untracked(|records| records.get(idx).cloned()) else {
            return;
        };
        if !record.is_playable() {
            return;
        }

        set_selected.set(idx);
        set_status.set(format!("Loaded {}", record.display));
        if let Some(config) = record.config.clone() {
            set_draft.set(config);
        }
        let source = default_pattern_source(&record);
        set_pattern_source.set(source.to_string());
        set_pattern_text.set(record_text_for_source(&record, source));
    };

    let export_current = move |_| {
        if let Some(record) = current_record.get_untracked() {
            match record_to_pattern_jml(&record) {
                Ok(xml) => {
                    download_text("jugglinglab-pattern.jml", &xml);
                    set_status.set("Current pattern exported as full JML".to_string());
                }
                Err(err) => set_status.set(err),
            }
        }
    };

    let export_all = move |_| {
        records.with_untracked(|records| {
            let playable = records
                .iter()
                .filter(|record| record.is_playable())
                .cloned()
                .collect::<Vec<_>>();
            download_text(
                "jugglinglab-library.jml",
                &jml::write_pattern_list("JugglingLab Web Library", &playable),
            );
        });
        set_status.set("Playable library exported as JML".to_string());
    };

    let handle_file = move |event: Event| {
        let input = event
            .target()
            .and_then(|target| target.dyn_into::<HtmlInputElement>().ok());
        let Some(file) = input
            .and_then(|input| input.files())
            .and_then(|files| files.get(0))
        else {
            return;
        };

        let Ok(reader) = FileReader::new() else {
            set_status.set("FileReader is not available in this browser".to_string());
            return;
        };

        let reader_clone = reader.clone();
        let onload = Closure::wrap(Box::new(move |_event: Event| {
            if let Ok(result) = reader_clone.result() {
                if let Some(text) = result.as_string() {
                    import_jml(text);
                }
            }
        }) as Box<dyn FnMut(_)>);
        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
        reader.read_as_text(&file).ok();
        onload.forget();
    };

    let compile_pattern_text = move |_| {
        let text = pattern_text.get_untracked();
        if pattern_source.get_untracked() == PATTERN_SOURCE_JML {
            match parse_editor_jml(&text) {
                Ok(library) => {
                    if let Some(record) =
                        library.records.into_iter().find(PatternRecord::is_playable)
                    {
                        checkpoint_editor();
                        set_records.update(|records| {
                            records.push(record);
                            set_selected.set(records.len() - 1);
                        });
                        set_status.set("JML pattern compiled".to_string());
                        set_pattern_source.set(PATTERN_SOURCE_JML.to_string());
                    } else {
                        set_status.set("No playable pattern found in JML text".to_string());
                    }
                }
                Err(err) => set_status.set(err),
            }
        } else {
            let config = text;
            match record_from_config_or_current_jml(&config, current_record.get_untracked()) {
                Ok((record, message)) => {
                    checkpoint_editor();
                    set_records.update(|records| {
                        records.push(record);
                        set_selected.set(records.len() - 1);
                    });
                    set_status.set(message);
                    set_pattern_text.set(config);
                    set_pattern_source.set(PATTERN_SOURCE_BASE.to_string());
                }
                Err(err) => set_status.set(err),
            }
        }
    };

    let revert_pattern_text = move |_| {
        if let Some(record) = current_record.get_untracked() {
            set_pattern_text.set(record_text_for_source(
                &record,
                &pattern_source.get_untracked(),
            ));
            set_status.set("Pattern text reverted".to_string());
        }
    };

    let choose_pattern_source = move |source: &'static str| {
        set_pattern_source.set(source.to_string());
        if let Some(record) = current_record.get_untracked() {
            set_pattern_text.set(record_text_for_source(&record, source));
        }
    };

    let select_canvas_object = move |event: ev::MouseEvent| {
        if view_dragged.get_untracked() {
            set_view_dragged.set(false);
            return;
        }
        if let Some(label) = canvas::hit_test_by_id(
            "juggling-stage",
            event.client_x() as f64,
            event.client_y() as f64,
        ) {
            set_selected_object.set(label.clone());
            set_status.set(format!("Selected {label}"));
        }
    };

    let start_canvas_drag = move |event: ev::MouseEvent| {
        event.prevent_default();
        if let Some(canvas) = event
            .target()
            .and_then(|target| target.dyn_into::<HtmlCanvasElement>().ok())
        {
            canvas.focus().ok();
        }
        set_view_drag_start.set(Some((event.client_x() as f64, event.client_y() as f64)));
        set_view_dragged.set(false);
    };

    let drag_canvas_view = move |event: ev::MouseEvent| {
        let Some((last_x, last_y)) = view_drag_start.get_untracked() else {
            return;
        };
        event.prevent_default();
        let x = event.client_x() as f64;
        let y = event.client_y() as f64;
        let dx = x - last_x;
        let dy = y - last_y;
        if dx.abs() + dy.abs() > 0.0 {
            set_view_dragged.set(true);
            set_camera_yaw.update(|yaw| {
                *yaw = (*yaw + dx * 0.008).rem_euclid(std::f64::consts::TAU);
            });
            set_camera_pitch.update(|pitch| {
                *pitch = (*pitch - dy * 0.008).clamp(0.1, 3.04);
            });
            set_view_drag_start.set(Some((x, y)));
        }
    };

    let end_canvas_drag = move |event: ev::MouseEvent| {
        event.prevent_default();
        if view_drag_start.get_untracked().is_some() {
            set_status.set("View adjusted".to_string());
        }
        set_view_drag_start.set(None);
    };

    let zoom_canvas_view = move |event: ev::WheelEvent| {
        event.prevent_default();
        let factor = (-event.delta_y() * 0.0012).exp();
        set_zoom.update(|zoom| {
            *zoom = (*zoom * factor).clamp(0.35, 4.0);
        });
        set_status.set("Zoom adjusted".to_string());
    };

    let reset_view = move |_| {
        set_zoom.set(1.15);
        set_camera_yaw.set(0.18);
        set_camera_pitch.set(std::f64::consts::FRAC_PI_2);
        set_camera_pan_x.set(0.0);
        set_camera_pan_y.set(0.0);
        set_camera_pan_z.set(0.0);
        set_status.set("View reset".to_string());
    };

    let start_camera_move = move |event: ev::KeyboardEvent| {
        let key = event.key().to_ascii_lowercase();
        let key = if key.starts_with("shift") {
            "shift".to_string()
        } else {
            key
        };
        if !is_camera_key(&key) {
            return;
        }

        event.prevent_default();
        set_pressed_camera_keys.update(|keys| {
            if !keys.iter().any(|existing| existing == &key) {
                keys.push(key);
            }
        });
        set_status.set("Camera moving".to_string());
    };

    let stop_camera_move = move |event: ev::KeyboardEvent| {
        let key = event.key().to_ascii_lowercase();
        let key = if key.starts_with("shift") {
            "shift".to_string()
        } else {
            key
        };
        if !is_camera_key(&key) {
            return;
        }

        event.prevent_default();
        set_pressed_camera_keys.update(|keys| keys.retain(|existing| existing != &key));
        if pressed_camera_keys.with_untracked(Vec::is_empty) {
            set_status.set("Camera moved".to_string());
        }
    };

    let clear_camera_move = move |_| set_pressed_camera_keys.set(Vec::new());

    let preview_ladder_drag = move |event: ev::PointerEvent| {
        let Some(drag) = ladder_drag.get_untracked() else {
            return;
        };
        if drag.pointer_id != event.pointer_id() {
            return;
        }
        event.prevent_default();
        let Some(diagram) = ladder_diagram(&current_spec.get_untracked()) else {
            return;
        };
        if let Some(time) = ladder_time_from_client_y(event.client_y(), &diagram) {
            let time = constrain_ladder_drag_time(&diagram, &drag, time);
            set_ladder_drag.set(Some(LadderDrag {
                kind: drag.kind.clone(),
                pointer_id: drag.pointer_id,
                selected_id: drag.selected_id.clone(),
                start_time: drag.start_time,
                preview_time: time,
                was_selected: drag.was_selected,
            }));
            if let LadderDragKind::Tracker { prop_cycle, .. } = drag.kind {
                let absolute_time = ladder_time_in_cycle(&diagram, prop_cycle, time);
                seek_renderer(absolute_time);
                set_playhead_time.set(absolute_time);
                if let Some(juggler) = ladder_juggler_from_client_x(event.client_x(), &diagram) {
                    set_ladder_insert_target.set(Some(LadderInsertTarget { juggler, time }));
                }
                set_status.set(format!("Move tracker to {time:.3}s"));
            } else {
                set_status.set(format!("Move ladder item to {time:.3}s"));
            }
        }
    };

    let finish_ladder_drag = move |event: ev::PointerEvent| {
        let Some(drag) = ladder_drag.get_untracked() else {
            return;
        };
        if drag.pointer_id != event.pointer_id() {
            return;
        }
        event.prevent_default();
        release_ladder_pointer(drag.pointer_id);
        set_ladder_drag.set(None);

        let Some(diagram) = ladder_diagram(&current_spec.get_untracked()) else {
            set_status.set("No ladder data available for this pattern".to_string());
            return;
        };
        let time = if let Some(raw_time) = ladder_time_from_client_y(event.client_y(), &diagram) {
            constrain_ladder_drag_time(&diagram, &drag, raw_time)
        } else {
            drag.preview_time
        };
        if let LadderDragKind::Tracker {
            was_playing,
            prop_cycle,
        } = drag.kind
        {
            let absolute_time = ladder_time_in_cycle(&diagram, prop_cycle, time);
            seek_renderer(absolute_time);
            set_playhead_time.set(absolute_time);
            set_playing.set(was_playing);
            set_status.set(format!("Tracker moved to {time:.3}s"));
            return;
        }

        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };

        if (time - drag.start_time).abs() < 1e-9 {
            if drag.was_selected {
                set_selected_ladder.set(String::new());
                set_status.set("Ladder selection cleared".to_string());
            } else {
                set_status.set("Ladder item selected".to_string());
            }
            return;
        }

        let selected_id = drag.selected_id.clone();
        let edit_result = match drag.kind {
            LadderDragKind::Event {
                primary_index,
                primary_time,
            } => {
                let new_primary_time = primary_time + time - drag.start_time;
                move_ladder_event_in_record(&record, primary_index, new_primary_time)
            }
            LadderDragKind::Position(position_index) => {
                move_ladder_position_in_record(&record, position_index, time)
            }
            LadderDragKind::Tracker { .. } => unreachable!(),
        };

        match edit_result {
            Ok(edited) => {
                commit_ladder_record(edited);
                set_selected_ladder.set(selected_id);
                set_status.set(format!("Moved ladder item to {time:.3}s"));
            }
            Err(err) => set_status.set(err),
        }
    };

    let cancel_ladder_drag = move |event: ev::PointerEvent| {
        if let Some(drag) = ladder_drag.get_untracked() {
            if drag.pointer_id != event.pointer_id() {
                return;
            }
            event.prevent_default();
            release_ladder_pointer(drag.pointer_id);
            if let Some(LadderDrag {
                kind: LadderDragKind::Tracker { was_playing, .. },
                ..
            }) = ladder_drag.get_untracked()
            {
                set_playing.set(was_playing);
            }
            set_ladder_drag.set(None);
            set_status.set("Ladder edit cancelled".to_string());
        }
    };

    let start_ladder_tracker_drag = move |event: ev::PointerEvent| {
        if event.button() != 0 {
            return;
        }
        event.prevent_default();
        event.stop_propagation();
        let Some(diagram) = ladder_diagram(&current_spec.get_untracked()) else {
            set_status.set("No ladder data available for this pattern".to_string());
            return;
        };
        let Some(time) = ladder_time_from_client_y(event.client_y(), &diagram) else {
            return;
        };
        let juggler = ladder_juggler_from_client_x(event.client_x(), &diagram).unwrap_or(1);
        let was_playing = playing.get_untracked();
        let prop_cycle = ladder_playback_cycle(
            &diagram,
            canvas::playback_time(&current_spec.get_untracked()),
        );
        capture_ladder_pointer(event.pointer_id());
        set_playing.set(false);
        let absolute_time = ladder_time_in_cycle(&diagram, prop_cycle, time);
        seek_renderer(absolute_time);
        set_playhead_time.set(absolute_time);
        set_ladder_insert_target.set(Some(LadderInsertTarget { juggler, time }));
        set_ladder_drag.set(Some(LadderDrag {
            kind: LadderDragKind::Tracker {
                was_playing,
                prop_cycle,
            },
            pointer_id: event.pointer_id(),
            selected_id: String::new(),
            start_time: time,
            preview_time: time,
            was_selected: false,
        }));
        set_selected_ladder.set(String::new());
        set_status.set(format!("Move tracker to {time:.3}s"));
    };

    let finish_ladder_popup = move || {
        set_ladder_context_menu.set(None);
        let mut was_playing = None;
        set_ladder_popup_was_playing.update(|saved| was_playing = saved.take());
        if let Some(was_playing) = was_playing {
            set_playing.set(was_playing);
        }
    };

    let open_ladder_context = move |event: ev::MouseEvent, selected_id: String| {
        event.prevent_default();
        event.stop_propagation();
        let Some(diagram) = ladder_diagram(&current_spec.get_untracked()) else {
            set_status.set("No ladder data available for this pattern".to_string());
            return;
        };
        set_selected_ladder.set(selected_id.clone());
        if !selected_ladder_has_context_actions(&current_spec.get_untracked(), &selected_id) {
            set_ladder_context_menu.set(None);
            set_status.set("No actions available for this ladder item".to_string());
            return;
        }
        if ladder_popup_was_playing.get_untracked().is_none() {
            set_ladder_popup_was_playing.set(Some(playing.get_untracked()));
        }
        set_ladder_prop_edit_time.set(canvas::playback_time(&current_spec.get_untracked()));
        set_playing.set(false);
        if let (Some(time), Some(juggler)) = (
            ladder_time_from_mouse(&event, &diagram),
            ladder_juggler_from_mouse(&event, &diagram),
        ) {
            set_ladder_insert_target.set(Some(LadderInsertTarget { juggler, time }));
            let prop_cycle = ladder_playback_cycle(&diagram, ladder_prop_edit_time.get_untracked());
            let absolute_time = ladder_time_in_cycle(&diagram, prop_cycle, time);
            seek_renderer(absolute_time);
            set_playhead_time.set(absolute_time);
        }
        let (x, y) = ladder_context_position(event.client_x() as f64, event.client_y() as f64);
        set_ladder_context_menu.set(Some(LadderContextMenu { x, y }));
        set_status.set("Ladder actions".to_string());
    };

    let add_ladder_position_from_target = move |_| {
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        let spec = current_spec.get_untracked();
        let target = ladder_insert_target
            .get_untracked()
            .or_else(|| selected_ladder_insert_target(&spec, &selected_ladder.get_untracked()))
            .unwrap_or_else(|| LadderInsertTarget {
                juggler: 1,
                time: canvas::playback_time(&spec).rem_euclid(spec.period_secs.max(0.1)),
            });

        match add_ladder_position_in_record(&record, &spec, target.juggler, target.time) {
            Ok((edited, position_index)) => {
                commit_ladder_record(edited);
                set_selected_ladder.set(format!("position-{}", position_index + 1));
                set_status.set(format!(
                    "Added position for juggler {} at {:.3}s",
                    target.juggler, target.time
                ));
            }
            Err(err) => set_status.set(err),
        }
    };

    let add_ladder_event_from_target = move |hand: usize| {
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        let spec = current_spec.get_untracked();
        let target = ladder_insert_target
            .get_untracked()
            .or_else(|| selected_ladder_insert_target(&spec, &selected_ladder.get_untracked()))
            .unwrap_or_else(|| LadderInsertTarget {
                juggler: 1,
                time: canvas::playback_time(&spec).rem_euclid(spec.period_secs.max(0.1)),
            });

        match add_ladder_event_in_record(&record, &spec, target.juggler, hand, target.time) {
            Ok((edited, event_index)) => {
                commit_ladder_record(edited);
                set_selected_ladder.set(format!("event-{}", event_index + 1));
                set_status.set(format!(
                    "Added {} event for juggler {} at {:.3}s",
                    if hand == 1 { "left" } else { "right" },
                    target.juggler,
                    target.time
                ));
            }
            Err(err) => set_status.set(err),
        }
    };

    let open_define_throw_dialog = move |_| {
        let selected_id = selected_ladder.get_untracked();
        match selected_ladder_throw_draft(&current_spec.get_untracked(), &selected_id) {
            Some(draft) => {
                set_define_throw_dialog.set(Some(draft));
                set_status.set("Editing throw definition".to_string());
            }
            None => set_status.set("Select a throw transition first".to_string()),
        }
    };

    let confirm_define_throw_dialog = move |_| {
        let Some(dialog) = define_throw_dialog.get_untracked() else {
            return;
        };
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };

        match define_ladder_throw_in_record(
            &record,
            dialog.event_index,
            dialog.transition_index,
            &dialog.throw_type,
            dialog.throw_mod.as_deref(),
        ) {
            Ok(edited) => {
                commit_ladder_record(edited);
                set_selected_ladder.set(dialog.selected_id);
                set_define_throw_dialog.set(None);
                finish_ladder_popup();
                set_status.set("Throw definition changed".to_string());
            }
            Err(err) => set_status.set(err),
        }
    };

    let open_define_prop_dialog = move |_| {
        let selected_id = selected_ladder.get_untracked();
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        match selected_ladder_prop_draft(
            &record,
            &current_spec.get_untracked(),
            &selected_id,
            ladder_prop_edit_time.get_untracked(),
        ) {
            Ok(Some(draft)) => {
                set_define_prop_dialog.set(Some(draft));
                set_status.set("Editing prop definition".to_string());
            }
            Ok(None) => set_status.set("Select a path or transition first".to_string()),
            Err(err) => set_status.set(err),
        }
    };

    let confirm_define_prop_dialog = move |_| {
        let Some(dialog) = define_prop_dialog.get_untracked() else {
            return;
        };
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };

        match define_ladder_prop_in_record(
            &record,
            dialog.path,
            &dialog.prop_assignment,
            &dialog.prop_type,
            dialog.prop_mod.as_deref(),
        ) {
            Ok(edited) => {
                commit_ladder_record(edited);
                set_selected_ladder.set(dialog.selected_id);
                set_define_prop_dialog.set(None);
                seek_renderer(dialog.playback_time);
                set_playhead_time.set(dialog.playback_time);
                finish_ladder_popup();
                set_status.set("Prop definition changed".to_string());
            }
            Err(err) => set_status.set(err),
        }
    };

    let remove_selected_ladder_item = move |_| {
        let selected_id = selected_ladder.get_untracked();
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        let Some(diagram) = ladder_diagram(&current_spec.get_untracked()) else {
            set_status.set("No ladder data available for this pattern".to_string());
            return;
        };

        let result = if let Some(event) =
            diagram.events.iter().find(|event| event.id == selected_id)
        {
            if !ladder_event_can_remove(&diagram, event) {
                Err("This event cannot be removed: it has throw/catch transitions or is the last event for its hand".to_string())
            } else {
                remove_ladder_event_in_record(&record, event.event_index)
            }
        } else if let Some(position) = diagram
            .positions
            .iter()
            .find(|position| position.id == selected_id)
        {
            remove_ladder_position_in_record(&record, position.position_index)
        } else {
            Err("Select an event or position to remove".to_string())
        };

        match result {
            Ok(edited) => {
                commit_ladder_record(edited);
                set_selected_ladder.set(String::new());
                set_status.set("Ladder item removed".to_string());
            }
            Err(err) => set_status.set(err),
        }
    };

    let change_selected_ladder_catch = move |target: MhnJmlTransitionType| {
        let selected_id = selected_ladder.get_untracked();
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        let Some(transition) =
            selected_ladder_transition(&current_spec.get_untracked(), &selected_id)
        else {
            set_status.set("Select a catch transition first".to_string());
            return;
        };

        match change_ladder_transition_type_in_record(
            &record,
            transition.event_index,
            transition.transition_index,
            target,
        ) {
            Ok(edited) => {
                commit_ladder_record(edited);
                set_selected_ladder.set(String::new());
                set_status.set("Catch style changed".to_string());
            }
            Err(err) => set_status.set(err),
        }
    };

    let make_selected_ladder_transition_last = move |_| {
        let selected_id = selected_ladder.get_untracked();
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        let Some(transition) =
            selected_ladder_transition(&current_spec.get_untracked(), &selected_id)
        else {
            set_status.set("Select a transition first".to_string());
            return;
        };

        match make_ladder_transition_last_in_record(
            &record,
            transition.event_index,
            transition.transition_index,
        ) {
            Ok(edited) => {
                commit_ladder_record(edited);
                set_selected_ladder.set(String::new());
                set_status.set("Transition moved to end of event".to_string());
            }
            Err(err) => set_status.set(err),
        }
    };

    view! {
        <main class="jl-root">
            <header class="jl-menu-bar">
                <div class="menu-group">
                    <label class="menu-file">
                        "Open JML"
                        <input type="file" accept=".jml,.xml,text/xml" on:change=handle_file />
                    </label>
                    <button type="button" on:click=export_current>"Save Pattern"</button>
                    <button type="button" on:click=export_all>"Save List"</button>
                    <button
                        type="button"
                        prop:disabled=move || undo_stack.with(Vec::is_empty)
                        on:click=undo_edit
                    >
                        "Undo"
                    </button>
                    <button
                        type="button"
                        prop:disabled=move || redo_stack.with(Vec::is_empty)
                        on:click=redo_edit
                    >
                        "Redo"
                    </button>
                </div>
                <div class="menu-group">
                    <span class="toolbar-label">"Notation"</span>
                    <button type="button" class="pressed">"Siteswap"</button>
                    <span class="toolbar-label">"Theme"</span>
                    <select
                        class="menu-select"
                        prop:value=move || theme.get()
                        on:change=move |ev| set_theme.set(event_target_value(&ev))
                    >
                        <option value="midnight">"Dark"</option>
                        <option value="aurora">"Aurora"</option>
                        <option value="contrast">"Contrast"</option>
                        <option value="atelier">"Atelier"</option>
                        <option value="light">"Light"</option>
                    </select>
                </div>
                <div class="status-line">{move || status.get()}</div>
            </header>

            <section class="jl-workbench">
                <section class="control-window">
                    <div class="window-caption">"Juggling Lab"</div>
                    <nav class="tabs">
                        <button
                            type="button"
                            class=move || tab_class(&active_tab.get(), "entry")
                            on:click=move |_| set_active_tab.set("entry".to_string())
                        >
                            "Pattern Entry"
                        </button>
                        <button
                            type="button"
                            class=move || tab_class(&active_tab.get(), "transitions")
                            on:click=move |_| set_active_tab.set("transitions".to_string())
                        >
                            "Transitions"
                        </button>
                        <button
                            type="button"
                            class=move || tab_class(&active_tab.get(), "generator")
                            on:click=move |_| set_active_tab.set("generator".to_string())
                        >
                            "Generator"
                        </button>
                    </nav>

                    <div class="tab-page">
                        {move || match active_tab.get().as_str() {
                            "transitions" => view! {
                                <div class="form-grid">
                                    <label>"From pattern"</label>
                                    <input value="3" />
                                    <label>"To pattern"</label>
                                    <input value="441" />
                                    <label>"Maximum throws"</label>
                                    <input type="number" value="8" />
                                    <label class="check-row">
                                        <input type="checkbox" checked />
                                        <span>"Allow multiplex"</span>
                                    </label>
                                    <div class="button-row">
                                        <button type="button" on:click=move |_| set_status.set("Transitioner UI mapped; engine port pending".to_string())>
                                            "Run"
                                        </button>
                                    </div>
                                </div>
                            }.into_any(),
                            "generator" => view! {
                                <div class="form-grid">
                                    <label>"Objects"</label>
                                    <input type="number" value="3" />
                                    <label>"Period"</label>
                                    <input type="number" value="5" />
                                    <label>"Maximum throw"</label>
                                    <input type="number" value="7" />
                                    <label class="check-row">
                                        <input type="checkbox" />
                                        <span>"Prime only"</span>
                                    </label>
                                    <label class="check-row">
                                        <input type="checkbox" />
                                        <span>"Connected patterns"</span>
                                    </label>
                                    <div class="button-row">
                                        <button type="button" on:click=move |_| set_status.set("Generator UI mapped; generator engine port pending".to_string())>
                                            "Run"
                                        </button>
                                    </div>
                                </div>
                            }.into_any(),
                            _ => view! {
                                <div class="form-grid">
                                    <label for="sample-select">"Pattern library"</label>
                                    <select
                                        id="sample-select"
                                        prop:value=move || selected.get().to_string()
                                        on:change=select_library_pattern
                                    >
                                        {move || records
                                            .get()
                                            .into_iter()
                                            .enumerate()
                                            .filter(|(_, record)| record.is_playable())
                                            .map(|(idx, record)| view! {
                                                <option value=idx.to_string()>{record.display}</option>
                                            })
                                            .collect::<Vec<_>>()
                                        }
                                    </select>

                                    <label for="pattern-entry">"Pattern"</label>
                                    <textarea
                                        id="pattern-entry"
                                        class="pattern-entry"
                                        spellcheck="false"
                                        prop:value=move || draft.get()
                                        on:input=move |ev| set_draft.set(event_target_value(&ev))
                                    ></textarea>

                                    <div class="button-row">
                                        <button type="button" on:click=run_pattern>"Run"</button>
                                        <button type="button" on:click=move |_| set_draft.set("pattern=3".to_string())>
                                            "Defaults"
                                        </button>
                                    </div>
                                </div>
                            }.into_any(),
                        }}
                    </div>
                </section>

                <section class="animation-window">
                    <div class="window-caption">
                        {move || current_spec.get().title}
                    </div>

                    <div class="view-tabs">
                        <button
                            type="button"
                            class=move || tab_class(&view_mode.get(), "simple")
                            on:click=move |_| set_view_mode.set("simple".to_string())
                        >
                            "Simple"
                        </button>
                        <button
                            type="button"
                            class=move || tab_class(&view_mode.get(), "edit")
                            on:click=move |_| set_view_mode.set("edit".to_string())
                        >
                            "Edit"
                        </button>
                        <button
                            type="button"
                            class=move || tab_class(&view_mode.get(), "pattern")
                            on:click=move |_| set_view_mode.set("pattern".to_string())
                        >
                            "Pattern"
                        </button>
                    </div>

                    <div class=move || match view_mode.get().as_str() {
                        "pattern" => "animation-split with-editor",
                        "edit" => "animation-split with-graph",
                        _ => "animation-split",
                    }>
                        <div class="stage-pane">
                            <canvas
                                id="juggling-stage"
                                class="stage-canvas"
                                tabindex="0"
                                on:mousedown=start_canvas_drag
                                on:mousemove=drag_canvas_view
                                on:mouseup=end_canvas_drag
                                on:mouseleave=end_canvas_drag
                                on:wheel=zoom_canvas_view
                                on:keydown=start_camera_move
                                on:keyup=stop_camera_move
                                on:blur=clear_camera_move
                                on:click=select_canvas_object
                            ></canvas>
                            <div class="selection-readout">
                                {move || {
                                    let selected = selected_object.get();
                                    if selected.is_empty() {
                                        String::new()
                                    } else {
                                        format!("Selected: {selected}")
                                    }
                                }}
                            </div>
                        </div>
                        <div class="pattern-editor">
                            <div class="radio-row">
                                <label>
                                    <input
                                        type="radio"
                                        name="pattern-source"
                                        prop:checked=move || pattern_source.get() == PATTERN_SOURCE_BASE
                                        prop:disabled=move || current_record.get().is_none_or(|record| record.config.is_none())
                                        on:change=move |_| choose_pattern_source(PATTERN_SOURCE_BASE)
                                    />
                                    " Base pattern"
                                </label>
                                <label>
                                    <input
                                        type="radio"
                                        name="pattern-source"
                                        prop:checked=move || pattern_source.get() == PATTERN_SOURCE_JML
                                        on:change=move |_| choose_pattern_source(PATTERN_SOURCE_JML)
                                    />
                                    " JML"
                                </label>
                            </div>
                            <textarea
                                spellcheck="false"
                                prop:value=move || pattern_text.get()
                                on:input=move |ev| set_pattern_text.set(event_target_value(&ev))
                            ></textarea>
                            <div class="button-row">
                                <button type="button" on:click=compile_pattern_text>"Compile"</button>
                                <button type="button" on:click=revert_pattern_text>"Revert"</button>
                            </div>
                        </div>
                        <aside class="graph-panel">
                            <div class="graph-title">"Ladder Diagram"</div>
                            <svg
                                id="ladder-svg"
                                viewBox="0 0 100 100"
                                preserveAspectRatio="none"
                                class=move || if ladder_drag.get().is_some() { "ladder-svg dragging" } else { "ladder-svg" }
                                on:pointermove=preview_ladder_drag
                                on:pointerup=finish_ladder_drag
                                on:pointercancel=cancel_ladder_drag
                            >
                                <defs>
                                    <clipPath id="ladder-period-clip">
                                        <rect x="0" y=LADDER_TOP_Y width="100" height=LADDER_HEIGHT />
                                    </clipPath>
                                </defs>
                                <rect
                                    x="0"
                                    y="5"
                                    width="100"
                                    height="90"
                                    class="ladder-hotzone"
                                    on:pointerdown=start_ladder_tracker_drag
                                    on:contextmenu=move |event| open_ladder_context(event, String::new())
                                />
                                {move || ladder_track_views(&current_spec.get())}
                                {move || {
                                    let spec = current_spec.get();
                                    let Some(diagram) = ladder_diagram(&spec) else {
                                        return Vec::new();
                                    };
                                    let drag = ladder_drag.get();
                                    diagram
                                        .edges
                                        .iter()
                                        .map(|edge| {
                                            let edge_id = edge.id.clone();
                                            let context_edge_id = edge.id.clone();
                                            let status_label = ladder_edge_label(edge);
                                            let selected = selected_ladder.get() == edge_id;
                                            let shapes = ladder_edge_shapes(&diagram, edge, drag.as_ref());
                                            let start_x = ladder_endpoint_x(&diagram, &edge.start);
                                            let start_y = ladder_absolute_time_y(
                                                &diagram,
                                                ladder_endpoint_preview_time(&edge.start, drag.as_ref()),
                                            );
                                            let end_x = ladder_endpoint_x(&diagram, &edge.end);
                                            let end_y = ladder_absolute_time_y(
                                                &diagram,
                                                ladder_endpoint_preview_time(&edge.end, drag.as_ref()),
                                            );
                                            let prop_style = ladder_prop_style(
                                                &spec,
                                                edge.path,
                                                playhead_time.get(),
                                            );
                                            view! {
                                                <g
                                                    class=if selected { "ladder-item selected" } else { "ladder-item" }
                                                    style=prop_style
                                                    clip-path="url(#ladder-period-clip)"
                                                    on:click=move |_| {
                                                        set_selected_ladder.set(edge_id.clone());
                                                        set_status.set(format!("Selected timing: {status_label}"));
                                                    }
                                                    on:contextmenu=move |event| {
                                                        open_ladder_context(event, context_edge_id.clone());
                                                    }
                                                >
                                                    {shapes
                                                        .iter()
                                                        .cloned()
                                                        .map(ladder_edge_hit_shape_view)
                                                        .collect::<Vec<_>>()
                                                    }
                                                    {shapes
                                                        .into_iter()
                                                        .map(ladder_edge_shape_view)
                                                        .collect::<Vec<_>>()
                                                    }
                                                    <circle class="edge-endpoint" cx=start_x cy=start_y r="1.4" />
                                                    <circle class="edge-endpoint" cx=end_x cy=end_y r="1.4" />
                                                </g>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                                {move || {
                                    let spec = current_spec.get();
                                    let Some(diagram) = ladder_diagram(&spec) else {
                                        return Vec::new();
                                    };
                                    diagram
                                        .transitions
                                        .clone()
                                        .into_iter()
                                        .map(|transition| {
                                            let transition_id = transition.id.clone();
                                            let context_transition_id = transition.id.clone();
                                            let status_label = ladder_transition_label(&transition);
                                            let selected = selected_ladder.get() == transition_id;
                                            let x = ladder_transition_x(&diagram, &transition);
                                            let y = ladder_time_y(
                                                &diagram,
                                                ladder_transition_preview_time(
                                                    &transition,
                                                    ladder_drag.get().as_ref(),
                                                ),
                                            );
                                            let class_name = if selected {
                                                format!("ladder-transition selected {}", ladder_transition_class(&transition))
                                            } else {
                                                format!("ladder-transition {}", ladder_transition_class(&transition))
                                            };
                                            let prop_style = ladder_prop_style(
                                                &spec,
                                                transition.path,
                                                playhead_time.get(),
                                            );
                                            view! {
                                                <g
                                                    class=class_name
                                                    style=prop_style
                                                    on:click=move |_| {
                                                        set_selected_ladder.set(transition_id.clone());
                                                        set_status.set(format!("Selected transition: {status_label}"));
                                                    }
                                                    on:contextmenu=move |event| {
                                                        open_ladder_context(event, context_transition_id.clone());
                                                    }
                                                >
                                                    <circle class="ladder-node-hitbox" cx=x cy=y r="3.2" />
                                                    <circle cx=x cy=y r="2.15" />
                                                </g>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                                {move || {
                                    let Some(diagram) = ladder_diagram(&current_spec.get()) else {
                                        return Vec::new();
                                    };
                                    diagram
                                        .positions
                                        .clone()
                                        .into_iter()
                                        .map(|position| {
                                            let position_id = position.id.clone();
                                            let status_label = ladder_position_label(&position);
                                            let selected = selected_ladder.get() == position_id;
                                            let position_index = position.position_index;
                                            let x = ladder_position_x(&diagram, position.juggler);
                                            let preview_time = ladder_drag
                                                .get()
                                                .filter(|drag| {
                                                    drag.kind == LadderDragKind::Position(position_index)
                                                })
                                                .map(|drag| drag.preview_time)
                                                .unwrap_or(position.time);
                                            let y = ladder_time_y(&diagram, preview_time);
                                            let side = 4.6;
                                            let top_left_x = x - side / 2.0;
                                            let top_left_y = y - side / 2.0;
                                            let drag_position_id = position_id.clone();
                                            let context_position_id = position_id.clone();
                                            let drag_status_label = status_label.clone();
                                            view! {
                                                <g
                                                    class=if selected { "ladder-position selected" } else { "ladder-position" }
                                                    on:pointerdown=move |pointer_event: ev::PointerEvent| {
                                                        if pointer_event.button() != 0 {
                                                            return;
                                                        }
                                                        pointer_event.prevent_default();
                                                        pointer_event.stop_propagation();
                                                        capture_ladder_pointer(pointer_event.pointer_id());
                                                        let was_selected = selected_ladder.get_untracked() == drag_position_id;
                                                        set_selected_ladder.set(drag_position_id.clone());
                                                        set_ladder_drag.set(Some(LadderDrag {
                                                            kind: LadderDragKind::Position(position_index),
                                                            pointer_id: pointer_event.pointer_id(),
                                                            selected_id: drag_position_id.clone(),
                                                            start_time: position.time,
                                                            preview_time: position.time,
                                                            was_selected,
                                                        }));
                                                        set_status.set(format!("Dragging position: {drag_status_label}"));
                                                    }
                                                    on:contextmenu=move |event| {
                                                        open_ladder_context(event, context_position_id.clone());
                                                    }
                                                >
                                                    <rect
                                                        class="ladder-node-hitbox"
                                                        x=x - 3.4
                                                        y=y - 3.4
                                                        width="6.8"
                                                        height="6.8"
                                                    />
                                                    <rect x=top_left_x y=top_left_y width=side height=side />
                                                </g>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                                {move || {
                                    let Some(diagram) = ladder_diagram(&current_spec.get()) else {
                                        return Vec::new();
                                    };
                                    diagram
                                        .events
                                        .clone()
                                        .into_iter()
                                        .map(|event| {
                                            let event_id = event.id.clone();
                                            let status_label = ladder_event_label(&event);
                                            let selected = selected_ladder.get() == event_id;
                                            let event_index = event.event_index;
                                            let symmetry_linked = ladder_drag.get().is_some_and(|drag| {
                                                matches!(
                                                    drag.kind,
                                                    LadderDragKind::Event { primary_index, .. }
                                                        if primary_index == event_index
                                                ) && drag.selected_id != event_id
                                            });
                                            let x = ladder_track_x(&diagram, event.track_index);
                                            let preview_time = ladder_event_preview_time(
                                                &event,
                                                ladder_drag.get().as_ref(),
                                            );
                                            let y = ladder_time_y(&diagram, preview_time);
                                            let x_left = x - 2.1;
                                            let x_right = x + 2.1;
                                            let y_top = y - 2.1;
                                            let y_bottom = y + 2.1;
                                            let drag_event_id = event_id.clone();
                                            let context_event_id = event_id.clone();
                                            let drag_status_label = status_label.clone();
                                            view! {
                                                <g
                                                    class=if selected {
                                                        "ladder-event selected"
                                                    } else if symmetry_linked {
                                                        "ladder-event symmetry-linked"
                                                    } else {
                                                        "ladder-event"
                                                    }
                                                    on:pointerdown=move |pointer_event: ev::PointerEvent| {
                                                        if pointer_event.button() != 0 {
                                                            return;
                                                        }
                                                        pointer_event.prevent_default();
                                                        pointer_event.stop_propagation();
                                                        capture_ladder_pointer(pointer_event.pointer_id());
                                                        let was_selected = selected_ladder.get_untracked() == drag_event_id;
                                                        set_selected_ladder.set(drag_event_id.clone());
                                                        set_ladder_drag.set(Some(LadderDrag {
                                                            kind: LadderDragKind::Event {
                                                                primary_index: event_index,
                                                                primary_time: event.primary_time,
                                                            },
                                                            pointer_id: pointer_event.pointer_id(),
                                                            selected_id: drag_event_id.clone(),
                                                            start_time: event.time,
                                                            preview_time: event.time,
                                                            was_selected,
                                                        }));
                                                        set_status.set(format!("Dragging event: {drag_status_label}"));
                                                    }
                                                    on:contextmenu=move |context_event| {
                                                        open_ladder_context(context_event, context_event_id.clone());
                                                    }
                                                >
                                                    <circle class="ladder-node-hitbox" cx=x cy=y r="3.2" />
                                                    <circle cx=x cy=y r="2.7" />
                                                    <line x1=x_left y1=y x2=x_right y2=y />
                                                    <line x1=x y1=y_top x2=x y2=y_bottom />
                                                </g>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                                {move || ladder_tracker_view(&current_spec.get(), playhead_time.get())}
                            </svg>
                            <p>
                                {move || {
                                    ladder_selection_text(&current_spec.get(), &selected_ladder.get())
                                }}
                            </p>
                        </aside>
                    </div>

                    <div class="animation-controls">
                        <button type="button" class=move || if playing.get() { "playback-button active" } else { "playback-button" } on:click=move |_| {
                            set_playing.update(|playing| *playing = !*playing);
                            set_status.set(if playing.get_untracked() { "Animation resumed".to_string() } else { "Animation stopped".to_string() });
                        }>
                            {move || if playing.get() { "Stop" } else { "Resume" }}
                        </button>
                        <label class="speed-control"><span>"Speed"</span><input type="range" min="0.25" max="2.25" step="0.05" prop:value=move || speed.get().to_string() on:input=move |ev| set_speed.set(event_target_value(&ev).parse().unwrap_or(1.0)) /></label>
                        <button type="button" on:click=reset_view>"Reset View"</button>
                        <label class="check-row"><input type="checkbox" prop:checked=move || show_trails.get() on:change=move |ev: ev::Event| set_show_trails.set(event_target::<HtmlInputElement>(&ev).checked()) /> <span>"Trails"</span></label>
                        <label class="check-row"><input type="checkbox" prop:checked=move || show_grid.get() on:change=move |ev: ev::Event| set_show_grid.set(event_target::<HtmlInputElement>(&ev).checked()) /> <span>"Ground"</span></label>
                    </div>
                </section>
            </section>
            {move || {
                let Some(menu) = ladder_context_menu.get() else {
                    return view! {}.into_any();
                };
                let spec = current_spec.get();
                let selected_id = selected_ladder.get();
                let can_add = selected_ladder_can_add_at_context(&spec, &selected_id);
                let can_remove = selected_ladder_can_remove(&spec, &selected_id);
                let can_define_prop = selected_ladder_can_define_prop(&spec, &selected_id);
                let can_define_throw = selected_ladder_can_define_throw(&spec, &selected_id);
                let can_catch = selected_ladder_can_change_catch(
                    &spec,
                    &selected_id,
                    MhnJmlTransitionType::Catch,
                );
                let can_soft = selected_ladder_can_change_catch(
                    &spec,
                    &selected_id,
                    MhnJmlTransitionType::SoftCatch,
                );
                let can_grab = selected_ladder_can_change_catch(
                    &spec,
                    &selected_id,
                    MhnJmlTransitionType::GrabCatch,
                );
                let can_make_last = selected_ladder_can_make_last(&spec, &selected_id);
                let menu_style = format!("left: {:.0}px; top: {:.0}px;", menu.x, menu.y);
                view! {
                    <div
                        class="ladder-context-backdrop"
                        on:click=move |_| finish_ladder_popup()
                        on:contextmenu=move |event| {
                            event.prevent_default();
                            finish_ladder_popup();
                        }
                    >
                        <div
                            class="ladder-context-menu"
                            style=menu_style
                            on:click=move |event| event.stop_propagation()
                            on:contextmenu=move |event| {
                                event.prevent_default();
                                event.stop_propagation();
                            }
                        >
                            <button
                                type="button"
                                class=if can_add { "context-action" } else { "context-action hidden" }
                                on:click=move |_| {
                                    add_ladder_event_from_target(1);
                                    finish_ladder_popup();
                                }
                            >"Add Left Event"</button>
                            <button
                                type="button"
                                class=if can_add { "context-action" } else { "context-action hidden" }
                                on:click=move |_| {
                                    add_ladder_event_from_target(0);
                                    finish_ladder_popup();
                                }
                            >"Add Right Event"</button>
                            <button
                                type="button"
                                class=if can_remove { "context-action" } else { "context-action hidden" }
                                on:click=move |event| {
                                    remove_selected_ladder_item(event);
                                    finish_ladder_popup();
                                }
                            >{selected_ladder_remove_label(&spec, &selected_id)}</button>
                            <button
                                type="button"
                                class=if can_add { "context-action" } else { "context-action hidden" }
                                on:click=move |event| {
                                    add_ladder_position_from_target(event);
                                    finish_ladder_popup();
                                }
                            >"Add Position"</button>
                            <div class=if can_define_prop || can_define_throw || can_catch || can_soft || can_grab || can_make_last {
                                "ladder-context-divider"
                            } else {
                                "ladder-context-divider hidden"
                            }></div>
                            <button
                                type="button"
                                class=if can_define_prop { "context-action" } else { "context-action hidden" }
                                on:click=move |event| {
                                    set_ladder_context_menu.set(None);
                                    open_define_prop_dialog(event);
                                }
                            >"Define Prop"</button>
                            <button
                                type="button"
                                class=if can_define_throw { "context-action" } else { "context-action hidden" }
                                on:click=move |event| {
                                    set_ladder_context_menu.set(None);
                                    open_define_throw_dialog(event);
                                }
                            >"Define Throw"</button>
                            <button
                                type="button"
                                class=if can_catch { "context-action" } else { "context-action hidden" }
                                on:click=move |_| {
                                    change_selected_ladder_catch(MhnJmlTransitionType::Catch);
                                    finish_ladder_popup();
                                }
                            >"Change to Normal Catch"</button>
                            <button
                                type="button"
                                class=if can_soft { "context-action" } else { "context-action hidden" }
                                on:click=move |_| {
                                    change_selected_ladder_catch(MhnJmlTransitionType::SoftCatch);
                                    finish_ladder_popup();
                                }
                            >"Change to Soft Catch"</button>
                            <button
                                type="button"
                                class=if can_grab { "context-action" } else { "context-action hidden" }
                                on:click=move |_| {
                                    change_selected_ladder_catch(MhnJmlTransitionType::GrabCatch);
                                    finish_ladder_popup();
                                }
                            >"Change to Grab Catch"</button>
                            <button
                                type="button"
                                class=if can_make_last { "context-action" } else { "context-action hidden" }
                                on:click=move |event| {
                                    make_selected_ladder_transition_last(event);
                                    finish_ladder_popup();
                                }
                            >"Make Last in Event"</button>
                        </div>
                    </div>
                }
                .into_any()
            }}
            {move || {
                if define_throw_dialog.get().is_none() {
                    return view! {}.into_any();
                }
                view! {
                    <div class="dialog-backdrop">
                        <section class="dialog-panel">
                            <div class="dialog-title">"Define Throw"</div>
                            <div class="dialog-grid">
                                <label for="throw-type">"Type"</label>
                                <select
                                    id="throw-type"
                                    prop:value=move || {
                                        define_throw_dialog
                                            .get()
                                            .map(|dialog| dialog.throw_type)
                                            .unwrap_or_else(|| "toss".to_string())
                                    }
                                    on:change=move |ev| {
                                        let value = event_target_value(&ev).to_ascii_lowercase();
                                        set_define_throw_dialog.update(|dialog| {
                                            if let Some(dialog) = dialog {
                                                dialog.throw_type = value;
                                            }
                                        });
                                    }
                                >
                                    <option value="toss">"toss"</option>
                                    <option value="bounce">"bounce"</option>
                                </select>
                                <label for="throw-mod">"Modifier"</label>
                                <input
                                    id="throw-mod"
                                    type="text"
                                    prop:value=move || {
                                        define_throw_dialog
                                            .get()
                                            .and_then(|dialog| dialog.throw_mod)
                                            .unwrap_or_default()
                                    }
                                    on:input=move |ev| {
                                        let value = event_target_value(&ev);
                                        set_define_throw_dialog.update(|dialog| {
                                            if let Some(dialog) = dialog {
                                                dialog.throw_mod = non_empty_trimmed(&value);
                                            }
                                        });
                                    }
                                />
                            </div>
                            <div class="dialog-actions">
                                <button type="button" on:click=move |_| {
                                    set_define_throw_dialog.set(None);
                                    finish_ladder_popup();
                                }>"Cancel"</button>
                                <button type="button" class="primary" on:click=confirm_define_throw_dialog>"Apply"</button>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
            {move || {
                if define_prop_dialog.get().is_none() {
                    return view! {}.into_any();
                }
                view! {
                    <div class="dialog-backdrop">
                        <section class="dialog-panel">
                            <div class="dialog-title">
                                {move || {
                                    define_prop_dialog
                                        .get()
                                        .map(|dialog| format!("Define Prop - Path {}", dialog.path))
                                        .unwrap_or_else(|| "Define Prop".to_string())
                                }}
                            </div>
                            <div class="dialog-grid">
                                <label for="prop-type">"Type"</label>
                                <select
                                    id="prop-type"
                                    prop:value=move || {
                                        define_prop_dialog
                                            .get()
                                            .map(|dialog| dialog.prop_type)
                                            .unwrap_or_else(|| "ball".to_string())
                                    }
                                    on:change=move |ev| {
                                        let value = event_target_value(&ev).to_ascii_lowercase();
                                        set_define_prop_dialog.update(|dialog| {
                                            if let Some(dialog) = dialog {
                                                dialog.prop_type = value;
                                            }
                                        });
                                    }
                                >
                                    <option value="ball">"ball"</option>
                                    <option value="ring">"ring"</option>
                                    <option value="image">"image"</option>
                                    <option value="square">"square"</option>
                                </select>
                                <label for="prop-mod">"Modifier"</label>
                                <input
                                    id="prop-mod"
                                    type="text"
                                    prop:value=move || {
                                        define_prop_dialog
                                            .get()
                                            .and_then(|dialog| dialog.prop_mod)
                                            .unwrap_or_default()
                                    }
                                    on:input=move |ev| {
                                        let value = event_target_value(&ev);
                                        set_define_prop_dialog.update(|dialog| {
                                            if let Some(dialog) = dialog {
                                                dialog.prop_mod = non_empty_trimmed(&value);
                                            }
                                        });
                                    }
                                />
                            </div>
                            <div class="dialog-actions">
                                <button type="button" on:click=move |_| {
                                    set_define_prop_dialog.set(None);
                                    finish_ladder_popup();
                                }>"Cancel"</button>
                                <button type="button" class="primary" on:click=confirm_define_prop_dialog>"Apply"</button>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
        </main>
    }
}

fn tab_class(current: &str, expected: &str) -> &'static str {
    if current == expected {
        "tab selected"
    } else {
        "tab"
    }
}

fn record_text(record: &PatternRecord) -> String {
    record
        .raw_pattern
        .clone()
        .or_else(|| record.config.clone())
        .unwrap_or_default()
}

fn default_pattern_source(record: &PatternRecord) -> &'static str {
    if record.config.is_some() {
        PATTERN_SOURCE_BASE
    } else {
        PATTERN_SOURCE_JML
    }
}

fn record_text_for_source(record: &PatternRecord, source: &str) -> String {
    if source == PATTERN_SOURCE_JML {
        record_to_pattern_jml(record).unwrap_or_else(|_| record_text(record))
    } else {
        record.config.clone().unwrap_or_else(|| record_text(record))
    }
}

fn parse_editor_jml(text: &str) -> Result<jml::PatternLibrary, String> {
    let trimmed = text.trim_start();
    if trimmed.starts_with("<pattern") {
        jml::parse_jml(&format!("<jml version=\"3\">{trimmed}</jml>"))
    } else {
        jml::parse_jml(trimmed)
    }
}

fn record_from_config_or_current_jml(
    config: &str,
    current: Option<PatternRecord>,
) -> Result<(PatternRecord, String), String> {
    match siteswap::parse_config(config) {
        Ok(spec) => {
            let display = siteswap::display_title(&spec);
            Ok((
                PatternRecord::siteswap(display, config.to_string()),
                "Pattern compiled".to_string(),
            ))
        }
        Err(err) => {
            if let Some(record) = current {
                if record
                    .notation
                    .as_deref()
                    .is_some_and(|notation| notation.eq_ignore_ascii_case("jml"))
                    && record.raw_pattern.is_some()
                    && record
                        .config
                        .as_deref()
                        .is_some_and(|base| same_config(base, config))
                {
                    return Ok((record, "Pattern compiled from JML source".to_string()));
                }
            }
            Err(err)
        }
    }
}

fn same_config(left: &str, right: &str) -> bool {
    normalize_config(left).eq_ignore_ascii_case(&normalize_config(right))
}

fn normalize_config(value: &str) -> String {
    value
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

fn non_empty_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn editor_shortcut_target_is_editable(event: &ev::KeyboardEvent) -> bool {
    let Some(target) = event.target() else {
        return false;
    };
    let Ok(element) = target.dyn_into::<web_sys::Element>() else {
        return false;
    };
    matches!(element.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
        || element
            .get_attribute("contenteditable")
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn is_camera_key(key: &str) -> bool {
    matches!(
        key,
        "w" | "a"
            | "s"
            | "d"
            | "q"
            | "e"
            | "arrowup"
            | "arrowdown"
            | "arrowleft"
            | "arrowright"
            | "shift"
    )
}

fn record_to_pattern_jml(record: &PatternRecord) -> Result<String, String> {
    if record
        .notation
        .as_deref()
        .is_some_and(|notation| notation.eq_ignore_ascii_case("jml"))
    {
        if let Some(raw) = &record.raw_pattern {
            if raw.trim_start().starts_with("<jml") {
                return Ok(raw.clone());
            }
            return Ok(format!(
                "<?xml version=\"1.0\"?>\n<!DOCTYPE jml SYSTEM \"file://jml.dtd\">\n<jml version=\"3\">\n{}\n</jml>\n",
                raw.trim()
            ));
        }
    }

    let config = record
        .config
        .as_deref()
        .ok_or_else(|| "Current pattern has no exportable config".to_string())?;
    let spec = siteswap::parse_config(config)?;
    let mut matrix = MhnMatrix::from_siteswap(&spec)?;
    let model = matrix.to_jml_pattern(&spec)?;
    model.assert_valid()?;
    Ok(model.write_jml(true, true))
}

fn move_ladder_event_in_record(
    record: &PatternRecord,
    event_index: usize,
    time: f64,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    if event_index >= model.events.len() {
        return Err("Selected ladder event is no longer available".to_string());
    }

    let period_secs = model.period_secs.max(0.1);
    model.events[event_index].t = time.rem_euclid(period_secs);
    model.sort_events();
    model.rebuild_path_events();
    record_from_edited_jml_model(record, model, "Ladder edit rejected")
}

fn move_ladder_position_in_record(
    record: &PatternRecord,
    position_index: usize,
    time: f64,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    if position_index >= model.positions.len() {
        return Err("Selected ladder position is no longer available".to_string());
    }

    let period_secs = model.period_secs.max(0.1);
    model.positions[position_index].t = time.rem_euclid(period_secs);
    model.positions.sort_by(|left, right| {
        left.t
            .total_cmp(&right.t)
            .then(left.juggler.cmp(&right.juggler))
    });
    record_from_edited_jml_model(record, model, "Ladder position edit rejected")
}

fn edit_ladder_position_spatial_in_record(
    record: &PatternRecord,
    position_index: usize,
    position: BodyPosition,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    let target = model
        .positions
        .get_mut(position_index)
        .ok_or_else(|| "Selected ladder position is no longer available".to_string())?;
    target.x = position.x;
    target.y = position.y;
    target.z = position.z;
    target.angle = position.angle;
    record_from_edited_jml_model(record, model, "Position spatial edit rejected")
}

fn add_ladder_position_in_record(
    record: &PatternRecord,
    spec: &AnimationSpec,
    juggler: usize,
    time: f64,
) -> Result<(PatternRecord, usize), String> {
    let AnimationKind::Jml(jml) = &spec.kind else {
        return Err("No ladder data available for this pattern".to_string());
    };
    let layout = jml
        .layout
        .as_ref()
        .ok_or_else(|| "No physical layout available for this pattern".to_string())?;
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    let period_secs = model.period_secs.max(0.1);
    let time = time.rem_euclid(period_secs);
    let juggler = juggler.clamp(1, model.number_of_jugglers.max(1));
    let position = layout.juggler_position(juggler, time)?;
    let angle = layout.juggler_angle(juggler, time)?;
    let position_index = model.positions.len();
    let target_x = position.x;
    let target_y = position.y;
    let target_z = position.z;

    model.positions.push(BodyPosition {
        x: target_x,
        y: target_y,
        z: target_z,
        t: time,
        angle,
        juggler,
    });
    model.positions.sort_by(|left, right| {
        left.t
            .total_cmp(&right.t)
            .then(left.juggler.cmp(&right.juggler))
    });
    let position_index = model
        .positions
        .iter()
        .position(|position| {
            position.juggler == juggler
                && (position.t - time).abs() < 1e-9
                && (position.x - target_x).abs() < 1e-9
                && (position.y - target_y).abs() < 1e-9
                && (position.z - target_z).abs() < 1e-9
        })
        .unwrap_or(position_index);
    let edited = record_from_edited_jml_model(record, model, "Add position rejected")?;
    Ok((edited, position_index))
}

fn add_ladder_event_in_record(
    record: &PatternRecord,
    spec: &AnimationSpec,
    juggler: usize,
    hand: usize,
    time: f64,
) -> Result<(PatternRecord, usize), String> {
    let AnimationKind::Jml(jml) = &spec.kind else {
        return Err("No ladder data available for this pattern".to_string());
    };
    let layout = jml
        .layout
        .as_ref()
        .ok_or_else(|| "No physical layout available for this pattern".to_string())?;
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    let period_secs = model.period_secs.max(0.1);
    let time = time.rem_euclid(period_secs);
    let juggler = juggler.clamp(1, model.number_of_jugglers.max(1));
    let hand = hand.min(1);
    let global = layout.hand_coordinate(juggler, hand, time)?;
    let local = layout.convert_global_to_local(global, juggler, time)?;
    let target_x = local.x;
    let target_y = local.y;
    let target_z = local.z;
    let fallback_index = model.events.len();

    model.events.push(MhnJmlEvent::new(
        target_x, target_y, target_z, time, juggler, hand,
    ));
    model.fix_holds()?;
    model.select_primary_events()?;
    model.sort_events();
    model.rebuild_path_events();
    let event_index = model
        .events
        .iter()
        .position(|event| {
            event.juggler == juggler
                && event.hand == hand
                && (event.t - time).abs() < 1e-9
                && (event.x - target_x).abs() < 1e-9
                && (event.y - target_y).abs() < 1e-9
                && (event.z - target_z).abs() < 1e-9
        })
        .unwrap_or(fallback_index);
    let edited = record_from_edited_jml_model(record, model, "Add event rejected")?;
    Ok((edited, event_index))
}

fn remove_ladder_event_in_record(
    record: &PatternRecord,
    event_index: usize,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    if event_index >= model.events.len() {
        return Err("Selected ladder event is no longer available".to_string());
    }

    if model.events[event_index]
        .transitions
        .iter()
        .any(|transition| {
            matches!(
                transition.transition_type,
                MhnJmlTransitionType::Throw
                    | MhnJmlTransitionType::Catch
                    | MhnJmlTransitionType::SoftCatch
                    | MhnJmlTransitionType::GrabCatch
            )
        })
    {
        return Err(
            "This event cannot be removed because it has throw/catch transitions".to_string(),
        );
    }

    let juggler = model.events[event_index].juggler;
    let hand = model.events[event_index].hand;
    if !model.events.iter().enumerate().any(|(index, event)| {
        index != event_index && event.juggler == juggler && event.hand == hand
    }) {
        return Err(
            "This event cannot be removed because it is the last event for its hand".to_string(),
        );
    }

    model.events.remove(event_index);
    model.sort_events();
    model.rebuild_path_events();
    record_from_edited_jml_model(record, model, "Ladder event remove rejected")
}

fn remove_ladder_position_in_record(
    record: &PatternRecord,
    position_index: usize,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    if position_index >= model.positions.len() {
        return Err("Selected ladder position is no longer available".to_string());
    }

    model.positions.remove(position_index);
    record_from_edited_jml_model(record, model, "Ladder position remove rejected")
}

fn change_ladder_transition_type_in_record(
    record: &PatternRecord,
    event_index: usize,
    transition_index: usize,
    target: MhnJmlTransitionType,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    let event = model
        .events
        .get_mut(event_index)
        .ok_or_else(|| "Selected ladder event is no longer available".to_string())?;
    let transition = event
        .transitions
        .get_mut(transition_index)
        .ok_or_else(|| "Selected ladder transition is no longer available".to_string())?;

    if !matches!(
        transition.transition_type,
        MhnJmlTransitionType::Catch
            | MhnJmlTransitionType::SoftCatch
            | MhnJmlTransitionType::GrabCatch
    ) {
        return Err("Only catch transitions can change catch style".to_string());
    }
    if !matches!(
        target,
        MhnJmlTransitionType::Catch
            | MhnJmlTransitionType::SoftCatch
            | MhnJmlTransitionType::GrabCatch
    ) {
        return Err("Invalid catch style target".to_string());
    }

    transition.transition_type = target;
    transition.throw_type = None;
    transition.throw_mod = None;
    model.rebuild_path_events();
    record_from_edited_jml_model(record, model, "Catch style change rejected")
}

fn make_ladder_transition_last_in_record(
    record: &PatternRecord,
    event_index: usize,
    transition_index: usize,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    let event = model
        .events
        .get_mut(event_index)
        .ok_or_else(|| "Selected ladder event is no longer available".to_string())?;
    if transition_index >= event.transitions.len() {
        return Err("Selected ladder transition is no longer available".to_string());
    }
    if transition_index + 1 == event.transitions.len() {
        return Err("Selected transition is already last in its event".to_string());
    }

    let transition = event.transitions.remove(transition_index);
    event.transitions.push(transition);
    model.rebuild_path_events();
    record_from_edited_jml_model(record, model, "Make-last rejected")
}

fn define_ladder_throw_in_record(
    record: &PatternRecord,
    event_index: usize,
    transition_index: usize,
    throw_type: &str,
    throw_mod: Option<&str>,
) -> Result<PatternRecord, String> {
    let throw_type = throw_type.trim().to_ascii_lowercase();
    if !matches!(throw_type.as_str(), "toss" | "bounce") {
        return Err(format!("Path type '{throw_type}' is not supported"));
    }

    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    let event = model
        .events
        .get_mut(event_index)
        .ok_or_else(|| "Selected ladder event is no longer available".to_string())?;
    let transition = event
        .transitions
        .get_mut(transition_index)
        .ok_or_else(|| "Selected ladder transition is no longer available".to_string())?;

    if transition.transition_type != MhnJmlTransitionType::Throw {
        return Err("Only throw transitions can define a path".to_string());
    }

    transition.throw_type = Some(throw_type);
    transition.throw_mod = throw_mod.and_then(non_empty_trimmed);
    model.rebuild_path_events();
    record_from_edited_jml_model(record, model, "Define throw rejected")
}

fn define_ladder_prop_in_record(
    record: &PatternRecord,
    path: usize,
    runtime_prop_assignment: &[usize],
    prop_type: &str,
    prop_mod: Option<&str>,
) -> Result<PatternRecord, String> {
    let prop_type = prop_type.trim().to_ascii_lowercase();
    if !matches!(prop_type.as_str(), "ball" | "ring" | "image" | "square") {
        return Err(format!("Prop type '{prop_type}' is not supported"));
    }
    let prop_mod = prop_mod.and_then(non_empty_trimmed);
    PropSpec::from_jml(&prop_type, prop_mod.as_deref())?;

    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    if path == 0 || path > model.number_of_paths {
        return Err("Selected ladder path is no longer available".to_string());
    }
    ensure_prop_assignment(&mut model);
    if runtime_prop_assignment.len() == model.number_of_paths
        && runtime_prop_assignment
            .iter()
            .all(|assigned| *assigned > 0 && *assigned <= model.props.len())
    {
        model.prop_assignment = runtime_prop_assignment.to_vec();
    }

    let path_index = path - 1;
    let current_prop_number = model.prop_assignment[path_index];
    if current_prop_number > 0 && current_prop_number <= model.props.len() {
        let still_used = model
            .prop_assignment
            .iter()
            .enumerate()
            .any(|(index, assigned)| index != path_index && *assigned == current_prop_number);
        if !still_used {
            model.props.remove(current_prop_number - 1);
            for assigned in &mut model.prop_assignment {
                if *assigned > current_prop_number {
                    *assigned -= 1;
                }
            }
        }
    }

    let matching_prop = model.props.iter().position(|prop| {
        prop.prop_type.eq_ignore_ascii_case(&prop_type)
            && option_eq_ignore_ascii_case(prop.modifier.as_deref(), prop_mod.as_deref())
    });
    let prop_number = if let Some(index) = matching_prop {
        index + 1
    } else {
        model
            .props
            .push(MhnJmlProp::new(prop_type, prop_mod.clone()));
        model.props.len()
    };

    model.prop_assignment[path_index] = prop_number;
    record_from_edited_jml_model(record, model, "Define prop rejected")
}

fn ensure_prop_assignment(model: &mut MhnJmlPattern) {
    if model.props.is_empty() {
        model.props.push(MhnJmlProp::new("ball", None));
    }
    if model.prop_assignment.len() != model.number_of_paths {
        model.prop_assignment = (0..model.number_of_paths)
            .map(|index| index % model.props.len() + 1)
            .collect();
    }
    for assigned in &mut model.prop_assignment {
        if *assigned == 0 || *assigned > model.props.len() {
            *assigned = 1;
        }
    }
}

fn option_eq_ignore_ascii_case(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
        (None, None) => true,
        _ => false,
    }
}

fn current_editor_snapshot(
    records: ReadSignal<Vec<PatternRecord>>,
    selected: ReadSignal<usize>,
    pattern_source: ReadSignal<String>,
    pattern_text: ReadSignal<String>,
    draft: ReadSignal<String>,
    selected_ladder: ReadSignal<String>,
) -> EditorSnapshot {
    let records = records.get_untracked();
    let selected = if records.is_empty() {
        0
    } else {
        selected.get_untracked().min(records.len() - 1)
    };
    EditorSnapshot {
        records,
        selected,
        pattern_source: pattern_source.get_untracked(),
        pattern_text: pattern_text.get_untracked(),
        draft: draft.get_untracked(),
        selected_ladder: selected_ladder.get_untracked(),
    }
}

fn push_editor_history(
    records: ReadSignal<Vec<PatternRecord>>,
    selected: ReadSignal<usize>,
    pattern_source: ReadSignal<String>,
    pattern_text: ReadSignal<String>,
    draft: ReadSignal<String>,
    selected_ladder: ReadSignal<String>,
    set_undo_stack: WriteSignal<Vec<EditorSnapshot>>,
    set_redo_stack: WriteSignal<Vec<EditorSnapshot>>,
) {
    push_undo_snapshot(
        records,
        selected,
        pattern_source,
        pattern_text,
        draft,
        selected_ladder,
        set_undo_stack,
    );
    set_redo_stack.set(Vec::new());
}

fn push_undo_snapshot(
    records: ReadSignal<Vec<PatternRecord>>,
    selected: ReadSignal<usize>,
    pattern_source: ReadSignal<String>,
    pattern_text: ReadSignal<String>,
    draft: ReadSignal<String>,
    selected_ladder: ReadSignal<String>,
    set_undo_stack: WriteSignal<Vec<EditorSnapshot>>,
) {
    let snapshot = current_editor_snapshot(
        records,
        selected,
        pattern_source,
        pattern_text,
        draft,
        selected_ladder,
    );
    set_undo_stack.update(|stack| push_bounded_snapshot(stack, snapshot));
}

fn push_redo_snapshot(
    records: ReadSignal<Vec<PatternRecord>>,
    selected: ReadSignal<usize>,
    pattern_source: ReadSignal<String>,
    pattern_text: ReadSignal<String>,
    draft: ReadSignal<String>,
    selected_ladder: ReadSignal<String>,
    set_redo_stack: WriteSignal<Vec<EditorSnapshot>>,
) {
    let snapshot = current_editor_snapshot(
        records,
        selected,
        pattern_source,
        pattern_text,
        draft,
        selected_ladder,
    );
    set_redo_stack.update(|stack| push_bounded_snapshot(stack, snapshot));
}

fn push_bounded_snapshot(stack: &mut Vec<EditorSnapshot>, snapshot: EditorSnapshot) {
    if stack.last() == Some(&snapshot) {
        return;
    }
    stack.push(snapshot);
    if stack.len() > HISTORY_LIMIT {
        let overflow = stack.len() - HISTORY_LIMIT;
        stack.drain(0..overflow);
    }
}

fn restore_editor_snapshot(
    snapshot: EditorSnapshot,
    set_records: WriteSignal<Vec<PatternRecord>>,
    set_selected: WriteSignal<usize>,
    set_pattern_source: WriteSignal<String>,
    set_pattern_text: WriteSignal<String>,
    set_draft: WriteSignal<String>,
    set_selected_ladder: WriteSignal<String>,
) {
    let selected = if snapshot.records.is_empty() {
        0
    } else {
        snapshot.selected.min(snapshot.records.len() - 1)
    };
    set_records.set(snapshot.records);
    set_selected.set(selected);
    set_pattern_source.set(snapshot.pattern_source);
    set_pattern_text.set(snapshot.pattern_text);
    set_draft.set(snapshot.draft);
    set_selected_ladder.set(snapshot.selected_ladder);
}

fn replace_current_ladder_record(
    edited: PatternRecord,
    selected: ReadSignal<usize>,
    set_selected: WriteSignal<usize>,
    set_records: WriteSignal<Vec<PatternRecord>>,
    set_pattern_source: WriteSignal<String>,
    set_pattern_text: WriteSignal<String>,
    set_draft: WriteSignal<String>,
) {
    let mut selected_index = selected.get_untracked();
    set_records.update(|records| {
        if selected_index < records.len() {
            records[selected_index] = edited.clone();
        } else {
            records.push(edited.clone());
            selected_index = records.len() - 1;
        }
    });
    set_selected.set(selected_index);
    set_pattern_source.set(PATTERN_SOURCE_JML.to_string());
    set_pattern_text.set(record_text_for_source(&edited, PATTERN_SOURCE_JML));
    if let Some(config) = edited.config.clone() {
        set_draft.set(config);
    }
}

fn record_from_edited_jml_model(
    record: &PatternRecord,
    model: MhnJmlPattern,
    error_prefix: &str,
) -> Result<PatternRecord, String> {
    model
        .assert_valid()
        .map_err(|err| format!("{error_prefix}: {err}"))?;
    let raw_pattern = jml::extract_pattern_xml(&model.write_jml(true, true))?;
    let edited = PatternRecord {
        display: record.display.clone(),
        notation: Some("jml".to_string()),
        config: model
            .base_pattern_config
            .clone()
            .or_else(|| record.config.clone()),
        animprefs: record.animprefs.clone(),
        info: record.info.clone(),
        tags: record.tags.clone(),
        raw_pattern: Some(raw_pattern),
    };

    let spec = AnimationSpec::from_record(&edited)?;
    match spec.kind {
        AnimationKind::Jml(_) => Ok(edited),
        AnimationKind::Unavailable(err) => Err(format!("Edited JML did not produce layout: {err}")),
    }
}

fn constrain_ladder_drag_time(diagram: &LadderDiagram, drag: &LadderDrag, time: f64) -> f64 {
    match &drag.kind {
        LadderDragKind::Event { .. } => diagram
            .constrain_event_time(&drag.selected_id, time)
            .unwrap_or(time),
        LadderDragKind::Position(position_index) => diagram
            .constrain_position_time(*position_index, time)
            .unwrap_or(time),
        LadderDragKind::Tracker { .. } => time.rem_euclid(diagram.period_secs.max(0.1)),
    }
}

#[derive(Clone)]
struct LadderSegment {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    class_name: &'static str,
}

#[derive(Clone)]
struct LadderArc {
    points: String,
    class_name: &'static str,
}

#[derive(Clone)]
enum LadderShape {
    Line(LadderSegment),
    Arc(LadderArc),
}

fn ladder_diagram(spec: &AnimationSpec) -> Option<LadderDiagram> {
    match &spec.kind {
        AnimationKind::Jml(jml) => Some(build_ladder_diagram(jml)),
        AnimationKind::Unavailable(_) => None,
    }
}

fn ladder_track_views(spec: &AnimationSpec) -> Vec<AnyView> {
    let Some(diagram) = ladder_diagram(spec) else {
        return Vec::new();
    };

    diagram
        .tracks
        .iter()
        .map(|track| {
            let x = ladder_track_x(&diagram, track.index);
            let label = track.label.clone();
            view! {
                <line x1=x y1="5" x2=x y2="95" class="hand-line" />
                <text x=x y="4" class="ladder-label">{label}</text>
            }
            .into_any()
        })
        .collect()
}

fn ladder_tracker_view(spec: &AnimationSpec, time: f64) -> AnyView {
    let Some(diagram) = ladder_diagram(spec) else {
        return view! {}.into_any();
    };
    let y = ladder_time_y(&diagram, time);
    view! {
        <line x1="0" y1=y x2="100" y2=y class="ladder-tracker" />
    }
    .into_any()
}

fn ladder_edge_shape_view(shape: LadderShape) -> AnyView {
    match shape {
        LadderShape::Line(segment) => view! {
            <line
                x1=segment.x1
                y1=segment.y1
                x2=segment.x2
                y2=segment.y2
                class=segment.class_name
            />
        }
        .into_any(),
        LadderShape::Arc(arc) => view! {
            <polyline points=arc.points class=arc.class_name />
        }
        .into_any(),
    }
}

fn ladder_edge_hit_shape_view(shape: LadderShape) -> AnyView {
    match shape {
        LadderShape::Line(segment) => view! {
            <line
                x1=segment.x1
                y1=segment.y1
                x2=segment.x2
                y2=segment.y2
                class="ladder-path-hitbox"
            />
        }
        .into_any(),
        LadderShape::Arc(arc) => view! {
            <polyline points=arc.points class="ladder-path-hitbox" />
        }
        .into_any(),
    }
}

fn ladder_edge_shapes(
    diagram: &LadderDiagram,
    edge: &LadderEdge,
    drag: Option<&LadderDrag>,
) -> Vec<LadderShape> {
    let x1 = ladder_endpoint_x(diagram, &edge.start);
    let start_time = ladder_endpoint_preview_time(&edge.start, drag);
    let y1 = ladder_absolute_time_y(diagram, start_time);
    let x2 = ladder_endpoint_x(diagram, &edge.end);
    let end_time = ladder_endpoint_preview_time(&edge.end, drag);
    let y2 = ladder_absolute_time_y(diagram, end_time);
    let class_name = ladder_edge_class(edge);
    vec![ladder_edge_shape_between(
        diagram, edge, x1, y1, x2, y2, class_name,
    )]
}

fn ladder_endpoint_preview_time(endpoint: &LadderEndpoint, drag: Option<&LadderDrag>) -> f64 {
    ladder_primary_preview_time(endpoint.event_index, endpoint.time, drag)
}

fn ladder_event_preview_time(event: &LadderEvent, drag: Option<&LadderDrag>) -> f64 {
    ladder_primary_preview_time(event.event_index, event.time, drag)
}

fn ladder_transition_preview_time(transition: &LadderTransition, drag: Option<&LadderDrag>) -> f64 {
    ladder_primary_preview_time(transition.event_index, transition.time, drag)
}

fn ladder_primary_preview_time(
    primary_index: usize,
    original_time: f64,
    drag: Option<&LadderDrag>,
) -> f64 {
    let Some(drag) = drag else {
        return original_time;
    };
    match &drag.kind {
        LadderDragKind::Event {
            primary_index: dragged_primary,
            ..
        } if *dragged_primary == primary_index => {
            original_time + drag.preview_time - drag.start_time
        }
        _ => original_time,
    }
}

fn ladder_edge_shape_between(
    diagram: &LadderDiagram,
    edge: &LadderEdge,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    class_name: &'static str,
) -> LadderShape {
    if edge.is_self_throw() {
        if let Some(points) = ladder_self_throw_points(diagram, edge, x1, y1, x2, y2) {
            return LadderShape::Arc(LadderArc { points, class_name });
        }
    }

    LadderShape::Line(LadderSegment {
        x1,
        y1,
        x2,
        y2,
        class_name,
    })
}

fn ladder_self_throw_points(
    diagram: &LadderDiagram,
    edge: &LadderEdge,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
) -> Option<String> {
    const SELF_THROW_WIDTH: f64 = 0.8;
    const ARC_STEPS: usize = 24;

    let dx = x1 - x2;
    let dy = y1 - y2;
    let half_chord = 0.5 * (dx * dx + dy * dy).sqrt();
    if half_chord <= 1e-6 {
        return None;
    }

    let x_mid = 0.5 * (x1 + x2);
    let y_mid = 0.5 * (y1 + y2);
    let ladder_center = ladder_position_x(diagram, edge.end.juggler);
    let bulge = SELF_THROW_WIDTH * (ladder_center - x_mid).abs();
    if bulge <= 1e-6 {
        return None;
    }

    let center_offset = 0.5 * (half_chord * half_chord / bulge - bulge).max(half_chord);
    let direction = match edge.end.hand {
        LadderHand::Left => -1.0,
        LadderHand::Right => 1.0,
    };
    let circle_x = x_mid + direction * center_offset * (y_mid - y1) / half_chord;
    let circle_y = y_mid - direction * center_offset * (x_mid - x1) / half_chord;
    let radius = ((x1 - circle_x) * (x1 - circle_x) + (y1 - circle_y) * (y1 - circle_y)).sqrt();
    if !radius.is_finite() || radius <= 1e-6 {
        return None;
    }

    let angle_start = (y1 - circle_y).atan2(x1 - circle_x);
    let angle_end = (y2 - circle_y).atan2(x2 - circle_x);
    let ccw_delta = (angle_end - angle_start).rem_euclid(std::f64::consts::TAU);
    let clockwise_delta = ccw_delta - std::f64::consts::TAU;
    let ccw_mid_x = circle_x + radius * (angle_start + 0.5 * ccw_delta).cos();
    let clockwise_mid_x = circle_x + radius * (angle_start + 0.5 * clockwise_delta).cos();
    let delta = if (clockwise_mid_x - ladder_center).abs() < (ccw_mid_x - ladder_center).abs() {
        clockwise_delta
    } else {
        ccw_delta
    };

    let mut points = String::new();
    for step in 0..=ARC_STEPS {
        let fraction = step as f64 / ARC_STEPS as f64;
        let angle = angle_start + delta * fraction;
        let x = circle_x + radius * angle.cos();
        let y = circle_y + radius * angle.sin();
        if step > 0 {
            points.push(' ');
        }
        points.push_str(&format!("{x:.3},{y:.3}"));
    }
    Some(points)
}

fn ladder_track_x(diagram: &LadderDiagram, track_index: usize) -> f64 {
    const BORDER_SIDES: f64 = 0.15;
    const JUGGLER_SEPARATION: f64 = 0.45;
    let Some(track) = diagram
        .tracks
        .iter()
        .find(|track| track.index == track_index)
    else {
        return 50.0;
    };
    let jugglers = diagram
        .tracks
        .iter()
        .map(|track| track.juggler)
        .max()
        .unwrap_or(1)
        .max(1);
    let width_units = 2.0 * BORDER_SIDES
        + jugglers as f64
        + (jugglers.saturating_sub(1)) as f64 * JUGGLER_SEPARATION;
    let hand_offset = match track.hand {
        LadderHand::Left => 0.0,
        LadderHand::Right => 1.0,
    };
    let x_units = BORDER_SIDES
        + (track.juggler.saturating_sub(1)) as f64 * (1.0 + JUGGLER_SEPARATION)
        + hand_offset;
    100.0 * x_units / width_units
}

fn ladder_endpoint_x(diagram: &LadderDiagram, endpoint: &LadderEndpoint) -> f64 {
    let track_x = ladder_track_x(diagram, endpoint.track_index);
    ladder_transition_x_from_parts(track_x, endpoint.hand, endpoint.transition_index)
}

fn ladder_transition_x(diagram: &LadderDiagram, transition: &LadderTransition) -> f64 {
    let track_x = ladder_track_x(diagram, transition.track_index);
    ladder_transition_x_from_parts(track_x, transition.hand, transition.transition_index)
}

fn ladder_transition_x_from_parts(track_x: f64, hand: LadderHand, transition_index: usize) -> f64 {
    const TRANSITION_SLOT_SPACING: f64 = 5.4;
    let direction = match hand {
        LadderHand::Left => 1.0,
        LadderHand::Right => -1.0,
    };
    track_x + direction * (transition_index as f64 + 1.0) * TRANSITION_SLOT_SPACING
}

fn ladder_position_x(diagram: &LadderDiagram, juggler: usize) -> f64 {
    let mut xs = diagram
        .tracks
        .iter()
        .filter(|track| track.juggler == juggler)
        .map(|track| ladder_track_x(diagram, track.index))
        .collect::<Vec<_>>();
    if xs.is_empty() {
        return 50.0;
    }
    xs.sort_by(f64::total_cmp);
    0.5 * (xs[0] + xs[xs.len() - 1])
}

fn ladder_time_y(diagram: &LadderDiagram, time: f64) -> f64 {
    LADDER_TOP_Y + (time.rem_euclid(diagram.period_secs) / diagram.period_secs) * LADDER_HEIGHT
}

fn ladder_playback_cycle(diagram: &LadderDiagram, time: f64) -> i64 {
    (time / diagram.period_secs.max(0.1)).floor() as i64
}

fn ladder_time_in_cycle(diagram: &LadderDiagram, cycle: i64, local_time: f64) -> f64 {
    let period = diagram.period_secs.max(0.1);
    cycle as f64 * period + local_time.clamp(0.0, period - 1e-6)
}

fn ladder_absolute_time_y(diagram: &LadderDiagram, time: f64) -> f64 {
    LADDER_TOP_Y + (time / diagram.period_secs) * LADDER_HEIGHT
}

fn ladder_time_from_mouse(event: &ev::MouseEvent, diagram: &LadderDiagram) -> Option<f64> {
    ladder_time_from_client_y(event.client_y(), diagram)
}

fn ladder_time_from_client_y(client_y: i32, diagram: &LadderDiagram) -> Option<f64> {
    let element = window()?.document()?.get_element_by_id("ladder-svg")?;
    let rect = element.get_bounding_client_rect();
    let height = rect.height();
    if !height.is_finite() || height <= 0.0 {
        return None;
    }

    let y = ((client_y as f64 - rect.top()) / height * 100.0)
        .clamp(LADDER_TOP_Y, LADDER_TOP_Y + LADDER_HEIGHT);
    let fraction = (y - LADDER_TOP_Y) / LADDER_HEIGHT;
    Some(fraction * diagram.period_secs.max(0.1))
}

fn ladder_juggler_from_mouse(event: &ev::MouseEvent, diagram: &LadderDiagram) -> Option<usize> {
    ladder_juggler_from_client_x(event.client_x(), diagram)
}

fn ladder_juggler_from_client_x(client_x: i32, diagram: &LadderDiagram) -> Option<usize> {
    let element = window()?.document()?.get_element_by_id("ladder-svg")?;
    let rect = element.get_bounding_client_rect();
    let width = rect.width();
    if !width.is_finite() || width <= 0.0 {
        return None;
    }

    let x = ((client_x as f64 - rect.left()) / width * 100.0).clamp(0.0, 100.0);
    (1..=diagram
        .tracks
        .iter()
        .map(|track| track.juggler)
        .max()
        .unwrap_or(1))
        .min_by(|left, right| {
            let left_distance = (ladder_position_x(diagram, *left) - x).abs();
            let right_distance = (ladder_position_x(diagram, *right) - x).abs();
            left_distance.total_cmp(&right_distance)
        })
}

fn capture_ladder_pointer(pointer_id: i32) {
    if let Some(element) = window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("ladder-svg"))
    {
        element.set_pointer_capture(pointer_id).ok();
    }
}

fn release_ladder_pointer(pointer_id: i32) {
    if let Some(element) = window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("ladder-svg"))
    {
        if element.has_pointer_capture(pointer_id) {
            element.release_pointer_capture(pointer_id).ok();
        }
    }
}

fn ladder_context_position(client_x: f64, client_y: f64) -> (f64, f64) {
    const MENU_WIDTH: f64 = 220.0;
    const MENU_HEIGHT: f64 = 410.0;
    const MARGIN: f64 = 8.0;
    let viewport_width = window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|value| value.as_f64())
        .unwrap_or(client_x + MENU_WIDTH + MARGIN);
    let viewport_height = window()
        .and_then(|window| window.inner_height().ok())
        .and_then(|value| value.as_f64())
        .unwrap_or(client_y + MENU_HEIGHT + MARGIN);
    (
        client_x.clamp(MARGIN, (viewport_width - MENU_WIDTH - MARGIN).max(MARGIN)),
        client_y.clamp(MARGIN, (viewport_height - MENU_HEIGHT - MARGIN).max(MARGIN)),
    )
}

fn ladder_edge_class(edge: &LadderEdge) -> &'static str {
    if edge.includes_holding() {
        "hold-throw"
    } else if edge.is_pass() {
        "pass-throw"
    } else if edge.is_crossing() {
        "cross-throw"
    } else {
        "self-throw"
    }
}

fn ladder_edge_label(edge: &LadderEdge) -> String {
    let wrap = if edge.wraps_period { " + wrap" } else { "" };
    format!(
        "Path {}: {} {} -> {} {}, {:.3}s to {:.3}s ({:.3}s{wrap})",
        edge.path,
        edge.start.hand_label(),
        edge.start.transition_label(),
        edge.end.hand_label(),
        edge.end.transition_label(),
        edge.start.time,
        edge.end_time_absolute,
        edge.duration_secs(),
    )
}

fn ladder_event_label(event: &LadderEvent) -> String {
    format!(
        "{} at {:.3}s: {}",
        event.hand_label(),
        event.time,
        event.transition_summary()
    )
}

fn ladder_transition_label(transition: &LadderTransition) -> String {
    transition.label()
}

fn ladder_transition_class(transition: &LadderTransition) -> &'static str {
    match transition.transition {
        TransitionKind::Throw => "transition-throw",
        TransitionKind::Catch => "transition-catch",
        TransitionKind::SoftCatch => "transition-softcatch",
        TransitionKind::GrabCatch => "transition-grabcatch",
        TransitionKind::Holding => "transition-holding",
    }
}

fn ladder_prop_style(spec: &AnimationSpec, path: usize, time: f64) -> String {
    let color = match &spec.kind {
        AnimationKind::Jml(jml) => jml
            .prop_for_path_at_time(path, time)
            .and_then(|prop| prop.color.clone())
            .unwrap_or_else(|| "#d8dde6".to_string()),
        AnimationKind::Unavailable(_) => "#d8dde6".to_string(),
    };
    format!("--ladder-prop-color: {color};")
}

fn ladder_position_label(position: &LadderPosition) -> String {
    position.label()
}

fn ladder_event_can_remove(diagram: &LadderDiagram, event: &LadderEvent) -> bool {
    !event.has_throw_or_catch()
        && diagram.events.iter().any(|other| {
            other.event_index != event.event_index
                && other.juggler == event.juggler
                && other.hand == event.hand
        })
}

fn ladder_can_add_position(spec: &AnimationSpec) -> bool {
    matches!(&spec.kind, AnimationKind::Jml(jml) if jml.layout.is_some())
}

fn ladder_can_add_event(spec: &AnimationSpec) -> bool {
    matches!(&spec.kind, AnimationKind::Jml(jml) if jml.layout.is_some())
}

fn selected_ladder_insert_target(
    spec: &AnimationSpec,
    selected_id: &str,
) -> Option<LadderInsertTarget> {
    let diagram = ladder_diagram(spec)?;
    if let Some(event) = diagram.events.iter().find(|event| event.id == selected_id) {
        return Some(LadderInsertTarget {
            juggler: event.juggler,
            time: event.time,
        });
    }
    if let Some(transition) = diagram
        .transitions
        .iter()
        .find(|transition| transition.id == selected_id)
    {
        return Some(LadderInsertTarget {
            juggler: transition.juggler,
            time: transition.time,
        });
    }
    if let Some(position) = diagram
        .positions
        .iter()
        .find(|position| position.id == selected_id)
    {
        return Some(LadderInsertTarget {
            juggler: position.juggler,
            time: position.time,
        });
    }
    if let Some(edge) = diagram.edges.iter().find(|edge| edge.id == selected_id) {
        return Some(LadderInsertTarget {
            juggler: edge.start.juggler,
            time: edge.start.time,
        });
    }
    None
}

fn selected_ladder_transition(spec: &AnimationSpec, selected_id: &str) -> Option<LadderTransition> {
    ladder_diagram(spec)?
        .transitions
        .into_iter()
        .find(|transition| transition.id == selected_id)
}

fn selected_ladder_position_index(spec: &AnimationSpec, selected_id: &str) -> Option<usize> {
    ladder_diagram(spec)?
        .positions
        .into_iter()
        .find(|position| position.id == selected_id)
        .map(|position| position.position_index)
}

fn selected_ladder_can_define_throw(spec: &AnimationSpec, selected_id: &str) -> bool {
    selected_ladder_transition(spec, selected_id)
        .is_some_and(|transition| transition.transition == TransitionKind::Throw)
}

fn selected_ladder_can_define_prop(spec: &AnimationSpec, selected_id: &str) -> bool {
    selected_ladder_path(spec, selected_id).is_some()
}

fn selected_ladder_can_add_at_context(spec: &AnimationSpec, selected_id: &str) -> bool {
    ladder_can_add_event(spec)
        && ladder_can_add_position(spec)
        && (selected_id.is_empty()
            || ladder_diagram(spec)
                .is_some_and(|diagram| diagram.edges.iter().any(|edge| edge.id == selected_id)))
}

fn selected_ladder_has_context_actions(spec: &AnimationSpec, selected_id: &str) -> bool {
    selected_ladder_can_add_at_context(spec, selected_id)
        || selected_ladder_can_remove(spec, selected_id)
        || selected_ladder_can_define_prop(spec, selected_id)
        || selected_ladder_can_define_throw(spec, selected_id)
        || selected_ladder_can_change_catch(spec, selected_id, MhnJmlTransitionType::Catch)
        || selected_ladder_can_change_catch(spec, selected_id, MhnJmlTransitionType::SoftCatch)
        || selected_ladder_can_change_catch(spec, selected_id, MhnJmlTransitionType::GrabCatch)
        || selected_ladder_can_make_last(spec, selected_id)
}

fn selected_ladder_throw_draft(
    spec: &AnimationSpec,
    selected_id: &str,
) -> Option<DefineThrowDraft> {
    let transition = selected_ladder_transition(spec, selected_id)?;
    if transition.transition != TransitionKind::Throw {
        return None;
    }
    Some(DefineThrowDraft {
        event_index: transition.event_index,
        transition_index: transition.transition_index,
        selected_id: transition.id,
        throw_type: transition
            .throw_type
            .unwrap_or_else(|| "toss".to_string())
            .to_ascii_lowercase(),
        throw_mod: transition.throw_mod,
    })
}

fn selected_ladder_prop_draft(
    record: &PatternRecord,
    spec: &AnimationSpec,
    selected_id: &str,
    time: f64,
) -> Result<Option<DefinePropDraft>, String> {
    let Some(path) = selected_ladder_path(spec, selected_id) else {
        return Ok(None);
    };
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    ensure_prop_assignment(&mut model);
    let prop_assignment = match &spec.kind {
        AnimationKind::Jml(jml) => jml.prop_assignment_at_time(time),
        AnimationKind::Unavailable(_) => model.prop_assignment.clone(),
    };
    let prop_number = prop_assignment[path - 1].saturating_sub(1);
    let prop = model
        .props
        .get(prop_number)
        .cloned()
        .unwrap_or_else(|| MhnJmlProp::new("ball", None));

    Ok(Some(DefinePropDraft {
        path,
        selected_id: selected_id.to_string(),
        prop_assignment,
        playback_time: time.rem_euclid(spec.period_secs.max(0.1)),
        prop_type: prop.prop_type.to_ascii_lowercase(),
        prop_mod: prop.modifier,
    }))
}

fn selected_ladder_path(spec: &AnimationSpec, selected_id: &str) -> Option<usize> {
    let diagram = ladder_diagram(spec)?;
    if let Some(transition) = diagram
        .transitions
        .iter()
        .find(|transition| transition.id == selected_id)
    {
        return Some(transition.path);
    }
    if let Some(edge) = diagram.edges.iter().find(|edge| edge.id == selected_id) {
        return Some(edge.path);
    }
    None
}

fn selected_ladder_can_change_catch(
    spec: &AnimationSpec,
    selected_id: &str,
    target: MhnJmlTransitionType,
) -> bool {
    let Some(transition) = selected_ladder_transition(spec, selected_id) else {
        return false;
    };
    transition.is_catch_style() && transition.transition != transition_kind_for_mhn(target)
}

fn selected_ladder_can_make_last(spec: &AnimationSpec, selected_id: &str) -> bool {
    let Some(diagram) = ladder_diagram(spec) else {
        return false;
    };
    let Some(transition) = diagram
        .transitions
        .iter()
        .find(|transition| transition.id == selected_id)
    else {
        return false;
    };
    diagram
        .events
        .iter()
        .find(|event| event.event_index == transition.event_index)
        .is_some_and(|event| transition.transition_index + 1 < event.transitions.len())
}

fn transition_kind_for_mhn(kind: MhnJmlTransitionType) -> TransitionKind {
    match kind {
        MhnJmlTransitionType::Throw => TransitionKind::Throw,
        MhnJmlTransitionType::Catch => TransitionKind::Catch,
        MhnJmlTransitionType::SoftCatch => TransitionKind::SoftCatch,
        MhnJmlTransitionType::GrabCatch => TransitionKind::GrabCatch,
        MhnJmlTransitionType::Holding => TransitionKind::Holding,
    }
}

fn selected_ladder_can_remove(spec: &AnimationSpec, selected_id: &str) -> bool {
    let Some(diagram) = ladder_diagram(spec) else {
        return false;
    };
    if let Some(event) = diagram.events.iter().find(|event| event.id == selected_id) {
        return ladder_event_can_remove(&diagram, event);
    }
    diagram
        .positions
        .iter()
        .any(|position| position.id == selected_id)
}

fn selected_ladder_remove_label(spec: &AnimationSpec, selected_id: &str) -> String {
    let Some(diagram) = ladder_diagram(spec) else {
        return "Remove".to_string();
    };
    if diagram.events.iter().any(|event| event.id == selected_id) {
        return "Remove Event".to_string();
    }
    if diagram
        .positions
        .iter()
        .any(|position| position.id == selected_id)
    {
        return "Remove Position".to_string();
    }
    "Remove".to_string()
}

fn ladder_selection_text(spec: &AnimationSpec, selected_id: &str) -> String {
    if selected_id.is_empty() {
        return "Click an event or throw/catch edge to inspect timing.".to_string();
    }

    let Some(diagram) = ladder_diagram(spec) else {
        return "No ladder data available for this pattern.".to_string();
    };
    if let Some(event) = diagram.events.iter().find(|event| event.id == selected_id) {
        return ladder_event_label(event);
    }
    if let Some(transition) = diagram
        .transitions
        .iter()
        .find(|transition| transition.id == selected_id)
    {
        return ladder_transition_label(transition);
    }
    if let Some(position) = diagram
        .positions
        .iter()
        .find(|position| position.id == selected_id)
    {
        return ladder_position_label(position);
    }

    diagram
        .edges
        .iter()
        .find(|edge| edge.id == selected_id)
        .map(ladder_edge_label)
        .unwrap_or_else(|| "Selected edge is no longer available.".to_string())
}

fn download_text(filename: &str, text: &str) {
    let Some(document) = window().and_then(|win| win.document()) else {
        return;
    };

    let parts = js_sys::Array::new();
    parts.push(&wasm_bindgen::JsValue::from_str(text));
    let options = BlobPropertyBag::new();
    options.set_type("application/xml;charset=utf-8");
    let Ok(blob) = Blob::new_with_str_sequence_and_options(&parts, &options) else {
        return;
    };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else {
        return;
    };
    let Ok(element) = document.create_element("a") else {
        return;
    };
    let Ok(anchor) = element.dyn_into::<HtmlAnchorElement>() else {
        return;
    };

    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();
    web_sys::Url::revoke_object_url(&url).ok();
}

fn initial_theme() -> String {
    window()
        .and_then(|win| win.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(THEME_STORAGE_KEY).ok().flatten())
        .filter(|theme| is_known_theme(theme))
        .unwrap_or_else(|| DEFAULT_THEME.to_string())
}

fn save_theme(theme: &str) {
    if !is_known_theme(theme) {
        return;
    }
    if let Some(storage) = window().and_then(|win| win.local_storage().ok().flatten()) {
        storage.set_item(THEME_STORAGE_KEY, theme).ok();
    }
}

fn is_known_theme(theme: &str) -> bool {
    matches!(
        theme,
        "midnight" | "aurora" | "contrast" | "atelier" | "light"
    )
}
