use crate::canvas::{self, RenderSettings};
use crate::export::{self, AnimationExportFormat, AnimationExportOptions, ExportedAnimation};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use js_sys::{Reflect, Uint8Array};
use juggling_core::animation::{AnimationKind, AnimationSpec, TransitionKind};
use juggling_core::animation_prefs::{AnimationPrefs, DefaultView, ShowGround};
use juggling_core::generator::{GenerationResult, GenerationStopReason};
use juggling_core::jml::{self, PatternRecord, record_to_pattern_jml};
use juggling_core::ladder::{
    LADDER_BORDER_SIDES, LADDER_JUGGLER_SEPARATION, LadderDiagram, LadderEdge, LadderEndpoint,
    LadderEvent, LadderHand, LadderLimit, LadderPosition, LadderTransition, MAX_JUGGLERS,
    build_ladder_diagram, ladder_item_sizing, ladder_limit,
};
use juggling_core::mhn_body::BodyPosition;
use juggling_core::mhn_hands::Coordinate;
use juggling_core::mhn_jml::{MhnJmlEvent, MhnJmlPattern, MhnJmlProp, MhnJmlTransitionType};
use juggling_core::mutator::{MutatorOptions, mutate_pattern_with_random};
use juggling_core::optimizer::optimize_pattern;
use juggling_core::parameter_list::ParameterList;
use juggling_core::prop::{
    PropKind, PropSpec, decode_image_source, encode_image_source, image_source_requires_embedding,
};
use juggling_core::share;
use juggling_core::util::to_string_rounded;
use juggling_core::{library, siteswap};
use leptos::ev;
use leptos::prelude::*;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    AbortController, Blob, BlobPropertyBag, Clipboard, Event, FileReader, HtmlAnchorElement,
    HtmlCanvasElement, HtmlInputElement, Request, RequestInit, ResizeObserver, Response, window,
};

const THEME_STORAGE_KEY: &str = "jugglinglab.theme";
const DEFAULT_THEME: &str = "midnight";
const LADDER_TOP_Y: f64 = 8.0;
const LADDER_BOTTOM_MARGIN: f64 = 6.0;
const LADDER_MIN_ZOOM: f64 = 1.0;
const LADDER_MAX_ZOOM: f64 = 10.0;
const LADDER_FIT_GAP_PX: f64 = 2.0;
const LADDER_DESKTOP_RADIUS_UNITS: f64 = 2.0;
const PATTERN_SOURCE_BASE: &str = "base";
const PATTERN_SOURCE_JML: &str = "jml";
const HISTORY_LIMIT: usize = 64;
const CAMERA_SNAP_ANGLE: f64 = 8.0_f64.to_radians();
const CAMERA_MIN_PITCH: f64 = 0.0001_f64.to_radians();
const CAMERA_MAX_PITCH: f64 = 179.9999_f64.to_radians();
const PROP_COLOR_CHOICES: [(&str, &str, &str); 12] = [
    ("transparent", "Transparent", "transparent"),
    ("black", "Black", "#000000"),
    ("blue", "Blue", "#0000ff"),
    ("cyan", "Cyan", "#00ffff"),
    ("gray", "Gray", "#808080"),
    ("green", "Green", "#00ff00"),
    ("magenta", "Magenta", "#ff00ff"),
    ("orange", "Orange", "#ffc800"),
    ("pink", "Pink", "#ffafaf"),
    ("red", "Red", "#ff0000"),
    ("white", "White", "#ffffff"),
    ("yellow", "Yellow", "#ffff00"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatternTransform {
    Optimize,
    SwapHands,
    FlipX,
    FlipTime,
}

impl PatternTransform {
    fn status(self) -> &'static str {
        match self {
            Self::Optimize => "Optimized for throwing error",
            Self::SwapHands => "Swapped hands",
            Self::FlipX => "Flipped pattern in X",
            Self::FlipTime => "Flipped pattern in time",
        }
    }
}

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
struct PatternListDocument {
    title: String,
    info: Option<String>,
    records: Vec<PatternRecord>,
    selected: Option<usize>,
    dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct GeneratorForm {
    balls: String,
    max_throw: String,
    period: String,
    jugglers: usize,
    rhythm_async: bool,
    composition: usize,
    multiplexing: usize,
    ground_state: bool,
    excited_state: bool,
    transition_throws: bool,
    pattern_rotations: bool,
    juggler_permutations: bool,
    connected_patterns: bool,
    symmetric_patterns: bool,
    no_simultaneous_catches: bool,
    no_clustered_throws: bool,
    true_multiplexing: bool,
    exclude_expressions: String,
    include_expressions: String,
    passing_delay: String,
}

impl Default for GeneratorForm {
    fn default() -> Self {
        Self {
            balls: "5".to_string(),
            max_throw: "7".to_string(),
            period: "5".to_string(),
            jugglers: 1,
            rhythm_async: true,
            composition: 0,
            multiplexing: 0,
            ground_state: true,
            excited_state: true,
            transition_throws: false,
            pattern_rotations: false,
            juggler_permutations: false,
            connected_patterns: true,
            symmetric_patterns: false,
            no_simultaneous_catches: true,
            no_clustered_throws: false,
            true_multiplexing: true,
            exclude_expressions: String::new(),
            include_expressions: String::new(),
            passing_delay: "0".to_string(),
        }
    }
}

impl GeneratorForm {
    fn arguments(&self) -> String {
        let max_throw = nonempty_or_dash(&self.max_throw);
        let period = nonempty_or_dash(&self.period);
        let mut arguments = format!("{} {max_throw} {period}", self.balls);
        if !self.rhythm_async {
            arguments.push_str(" -s");
        }
        if self.jugglers > 1 {
            arguments.push_str(&format!(" -j {}", self.jugglers));
            let passing_delay_enabled = self.ground_state && !self.excited_state;
            if passing_delay_enabled && !self.passing_delay.is_empty() {
                arguments.push_str(&format!(" -d {} -l 1", self.passing_delay));
            }
            let permutations_enabled = self.ground_state && self.excited_state;
            if (permutations_enabled && self.juggler_permutations) || !permutations_enabled {
                arguments.push_str(" -jp");
            }
            if self.connected_patterns {
                arguments.push_str(" -cp");
            }
            if self.symmetric_patterns {
                arguments.push_str(" -sym");
            }
        }
        match self.composition {
            0 => arguments.push_str(" -f"),
            2 => arguments.push_str(" -prime"),
            _ => {}
        }
        if self.ground_state && !self.excited_state {
            arguments.push_str(" -g");
        } else if !self.ground_state && self.excited_state {
            arguments.push_str(" -ng");
        }
        if !self.excited_state || !self.transition_throws {
            arguments.push_str(" -se");
        }
        if self.pattern_rotations {
            arguments.push_str(" -rot");
        }
        if self.multiplexing > 0 {
            arguments.push_str(&format!(" -m {}", self.multiplexing + 1));
            if !self.no_simultaneous_catches {
                arguments.push_str(" -mf");
            }
            if self.no_clustered_throws {
                arguments.push_str(" -mc");
            }
            if self.true_multiplexing {
                arguments.push_str(" -mt");
            }
        }
        if !self.exclude_expressions.is_empty() {
            arguments.push_str(" -x ");
            arguments.push_str(&self.exclude_expressions);
        }
        if !self.include_expressions.is_empty() {
            arguments.push_str(" -i ");
            arguments.push_str(&self.include_expressions);
        }
        arguments.push_str(" -n");
        arguments
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TransitionerForm {
    from_pattern: String,
    to_pattern: String,
    multiplexing: bool,
    simultaneous_throws: String,
    no_simultaneous_catches: bool,
    no_clustered_throws: bool,
}

impl Default for TransitionerForm {
    fn default() -> Self {
        Self {
            from_pattern: String::new(),
            to_pattern: String::new(),
            multiplexing: false,
            simultaneous_throws: "2".to_string(),
            no_simultaneous_catches: true,
            no_clustered_throws: false,
        }
    }
}

impl TransitionerForm {
    fn arguments(&self) -> String {
        let mut arguments = format!(
            "{} {}",
            nonempty_or_dash(&self.from_pattern),
            nonempty_or_dash(&self.to_pattern)
        );
        if self.multiplexing && !self.simultaneous_throws.is_empty() {
            arguments.push_str(&format!(" -m {}", self.simultaneous_throws));
            if !self.no_simultaneous_catches {
                arguments.push_str(" -mf");
            }
            if self.no_clustered_throws {
                arguments.push_str(" -mc");
            }
        }
        arguments
    }
}

fn nonempty_or_dash(value: &str) -> &str {
    if value.trim().is_empty() { "-" } else { value }
}

#[derive(Clone, Debug, PartialEq)]
struct AnimationPrefsDialogState {
    width: String,
    height: String,
    fps: String,
    slowdown: String,
    border: String,
    show_ground: ShowGround,
    start_paused: bool,
    mouse_pause: bool,
    stereo: bool,
    catch_sound: bool,
    bounce_sound: bool,
    manual_settings: String,
    error: Option<String>,
}

impl AnimationPrefsDialogState {
    fn from_prefs(prefs: &AnimationPrefs) -> Self {
        let mut parameters = ParameterList::parse(Some(&prefs.to_string())).unwrap_or_default();
        for name in [
            "width",
            "height",
            "fps",
            "slowdown",
            "border",
            "showground",
            "stereo",
            "startpaused",
            "mousepause",
            "catchsound",
            "bouncesound",
        ] {
            parameters.remove_parameter(name);
        }
        Self {
            width: prefs.width.to_string(),
            height: prefs.height.to_string(),
            fps: to_string_rounded(prefs.fps, 2),
            slowdown: to_string_rounded(prefs.slowdown, 2),
            border: prefs.border_pixels.to_string(),
            show_ground: prefs.show_ground,
            start_paused: prefs.start_paused,
            mouse_pause: prefs.mouse_pause,
            stereo: prefs.stereo,
            catch_sound: prefs.catch_sound,
            bounce_sound: prefs.bounce_sound,
            manual_settings: parameters.to_string(),
            error: None,
        }
    }

    fn to_prefs(&self) -> Result<AnimationPrefs, String> {
        let width = parse_nonnegative_pref(&self.width, "width")?;
        let height = parse_nonnegative_pref(&self.height, "height")?;
        let fps = parse_positive_pref(&self.fps, "fps")?;
        let slowdown = parse_positive_pref(&self.slowdown, "slowdown")?;
        let border_pixels = parse_nonnegative_pref(&self.border, "border")?;
        let explicit = AnimationPrefs {
            width,
            height,
            fps,
            slowdown,
            border_pixels,
            show_ground: self.show_ground,
            stereo: self.stereo,
            start_paused: self.start_paused,
            mouse_pause: self.mouse_pause,
            catch_sound: self.catch_sound,
            bounce_sound: self.bounce_sound,
            ..AnimationPrefs::default()
        };
        let explicit = explicit.to_string();
        let source = match (explicit.is_empty(), self.manual_settings.trim().is_empty()) {
            (true, true) => String::new(),
            (false, true) => explicit,
            (true, false) => self.manual_settings.trim().to_string(),
            (false, false) => format!("{explicit};{}", self.manual_settings.trim()),
        };
        AnimationPrefs::parse(Some(&source))
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AnimationExportDialogState {
    format: AnimationExportFormat,
    width: String,
    height: String,
    fps: String,
    antialiasing: bool,
    show_title: bool,
    webm_supported: bool,
    mp4_supported: bool,
    error: Option<String>,
}

impl AnimationExportDialogState {
    fn from_prefs(prefs: &AnimationPrefs) -> Self {
        Self {
            format: AnimationExportFormat::Gif,
            width: prefs.width.to_string(),
            height: prefs.height.to_string(),
            fps: to_string_rounded(prefs.fps, 2),
            antialiasing: false,
            show_title: true,
            webm_supported: export::webm_supported(),
            mp4_supported: export::mp4_supported(),
            error: None,
        }
    }

    fn to_options(&self, slowdown: f64) -> Result<AnimationExportOptions, String> {
        let width = parse_nonnegative_pref(&self.width, "width")? as u32;
        let height = parse_nonnegative_pref(&self.height, "height")? as u32;
        let fps = parse_positive_pref(&self.fps, "fps")?;
        if self.format == AnimationExportFormat::Mp4 && !self.mp4_supported {
            return Err(
                "MP4/H.264 export is not supported by this browser; GIF export remains available"
                    .to_string(),
            );
        }
        if self.format == AnimationExportFormat::WebM && !self.webm_supported {
            return Err(
                "WebM export is not supported by this browser; GIF export remains available"
                    .to_string(),
            );
        }
        Ok(AnimationExportOptions {
            format: self.format,
            width,
            height,
            fps,
            slowdown,
            antialiasing: self.antialiasing,
            show_title: self.show_title,
        })
    }
}

fn parse_nonnegative_pref(value: &str, name: &str) -> Result<i32, String> {
    value
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|value| *value >= 0)
        .ok_or_else(|| format!("Number format error in \"{name}\" value"))
}

fn parse_positive_pref(value: &str, name: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| format!("Number format error in \"{name}\" value"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatternListDialogAction {
    InsertText { index: usize },
    EditDisplay { index: usize },
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
struct LadderLongPress {
    pointer_id: i32,
    selected_id: String,
    client_x: f64,
    client_y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LadderTouch {
    pointer_id: i32,
    client_x: f64,
    client_y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LadderScrollDrag {
    pointer_id: i32,
    start_client_y: f64,
    start_scroll_top: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LadderViewMetrics {
    transition_radius: f64,
    position_radius: f64,
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
struct EventCanvasDrag {
    hit: canvas::EventEditorHit,
    start_client_x: f64,
    start_client_y: f64,
    start_image: Coordinate,
    start_primary: MhnJmlEvent,
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
    color: String,
    diameter: f64,
    inside_diameter: f64,
    image_source: String,
    image_width: f64,
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
    let mut initial_records = library::builtin_records();
    let (initial_prefs, initial_status) = match shared_pattern_from_location() {
        Ok(Some(shared)) => {
            initial_records.insert(0, shared.record);
            (
                shared.prefs.unwrap_or_default(),
                "Opened shared pattern".to_string(),
            )
        }
        Ok(None) => (AnimationPrefs::default(), "Ready".to_string()),
        Err(error) => (
            AnimationPrefs::default(),
            format!("Unable to open shared pattern: {error}"),
        ),
    };
    let first_playable = initial_records
        .iter()
        .position(PatternRecord::is_playable)
        .unwrap_or(0);
    let initial_draft = initial_records
        .get(first_playable)
        .and_then(|record| record.config.clone())
        .unwrap_or_else(|| "pattern=3".to_string());
    let initial_spec = initial_records
        .get(first_playable)
        .and_then(|record| AnimationSpec::from_record(record).ok())
        .unwrap_or_else(AnimationSpec::fallback);
    let initial_speed = playback_speed(&initial_prefs);
    let initial_should_play = !initial_prefs.start_paused;
    let initial_mouse_pause_playing = initial_prefs.mouse_pause.then_some(initial_should_play);
    let initial_playing = initial_should_play && !initial_prefs.mouse_pause;
    let initial_show_grid = show_ground_for_pattern(initial_prefs.show_ground, &initial_spec);
    let (initial_camera_yaw, initial_camera_pitch) =
        initial_camera_angles(&initial_spec, &initial_prefs);
    let initial_view_mode_name = initial_view_mode(&initial_spec, initial_prefs.default_view);
    let initial_stereo = initial_prefs.stereo;
    let initial_catch_sound = initial_prefs.catch_sound;
    let initial_bounce_sound = initial_prefs.bounce_sound;
    let (records, set_records) = signal(initial_records);
    let (selected, set_selected) = signal(first_playable);
    let (active_tab, set_active_tab) = signal("entry".to_string());
    let (view_mode, set_view_mode) = signal(initial_view_mode_name.to_string());
    let (theme, set_theme) = signal(initial_theme());
    let (playing, set_playing) = signal(initial_playing);
    let (playhead_time, set_playhead_time) = signal(0.0);
    let (speed, set_speed) = signal(initial_speed);
    let (zoom, set_zoom) = signal(1.15);
    let (camera_yaw, set_camera_yaw) = signal(initial_camera_yaw);
    let (camera_pitch, set_camera_pitch) = signal(initial_camera_pitch);
    let (camera_pan_x, set_camera_pan_x) = signal(0.0);
    let (camera_pan_y, set_camera_pan_y) = signal(0.0);
    let (camera_pan_z, set_camera_pan_z) = signal(0.0);
    let (show_trails, set_show_trails) = signal(true);
    let (show_grid, set_show_grid) = signal(initial_show_grid);
    let (stereo, set_stereo) = signal(initial_stereo);
    let (catch_sound, set_catch_sound) = signal(initial_catch_sound);
    let (bounce_sound, set_bounce_sound) = signal(initial_bounce_sound);
    let (animation_prefs, set_animation_prefs) = signal(initial_prefs);
    let (animation_prefs_dialog, set_animation_prefs_dialog) =
        signal(None::<AnimationPrefsDialogState>);
    let (animation_export_dialog, set_animation_export_dialog) =
        signal(None::<AnimationExportDialogState>);
    let (animation_export_running, set_animation_export_running) = signal(false);
    let (animation_export_progress, set_animation_export_progress) = signal((0usize, 0usize));
    let (animation_export_cancel, set_animation_export_cancel) = signal(None::<Arc<AtomicBool>>);
    let (mouse_pause_was_playing, set_mouse_pause_was_playing) =
        signal(initial_mouse_pause_playing);
    let (mutator_options, set_mutator_options) = signal(MutatorOptions::default());
    let (selection_records, set_selection_records) = signal(Vec::<PatternRecord>::new());
    let (draft, set_draft) = signal(initial_draft);
    let (pattern_text, set_pattern_text) = signal(String::new());
    let (pattern_source, set_pattern_source) = signal(PATTERN_SOURCE_BASE.to_string());
    let (selected_object, set_selected_object) = signal(String::new());
    let (selected_ladder, set_selected_ladder) = signal(String::new());
    let (ladder_drag, set_ladder_drag) = signal(None::<LadderDrag>);
    let (ladder_preview_spec, set_ladder_preview_spec) = signal(None::<AnimationSpec>);
    let (ladder_insert_target, set_ladder_insert_target) = signal(None::<LadderInsertTarget>);
    let (ladder_context_menu, set_ladder_context_menu) = signal(None::<LadderContextMenu>);
    let (ladder_long_press, set_ladder_long_press) = signal(None::<LadderLongPress>);
    let (ladder_zoom, set_ladder_zoom) = signal(1.0_f64);
    let (ladder_auto_fit, set_ladder_auto_fit) = signal(true);
    let (ladder_touches, set_ladder_touches) = signal(Vec::<LadderTouch>::new());
    let (ladder_pinch_distance, set_ladder_pinch_distance) = signal(None::<f64>);
    let (ladder_scroll_drag, set_ladder_scroll_drag) = signal(None::<LadderScrollDrag>);
    let (ladder_popup_was_playing, set_ladder_popup_was_playing) = signal(None::<bool>);
    let (ladder_prop_edit_time, set_ladder_prop_edit_time) = signal(0.0);
    let (define_throw_dialog, set_define_throw_dialog) = signal(None::<DefineThrowDraft>);
    let (define_prop_dialog, set_define_prop_dialog) = signal(None::<DefinePropDraft>);
    let (undo_stack, set_undo_stack) = signal(Vec::<EditorSnapshot>::new());
    let (redo_stack, set_redo_stack) = signal(Vec::<EditorSnapshot>::new());
    let (view_drag_start, set_view_drag_start) = signal(None::<(f64, f64)>);
    let (view_drag_camera, set_view_drag_camera) = signal(None::<(f64, f64)>);
    let (event_canvas_drag, set_event_canvas_drag) = signal(None::<EventCanvasDrag>);
    let (position_canvas_drag, set_position_canvas_drag) = signal(None::<PositionCanvasDrag>);
    let (view_dragged, set_view_dragged) = signal(false);
    let (pressed_camera_keys, set_pressed_camera_keys) = signal(Vec::<String>::new());
    let (status, set_status) = signal(initial_status);
    let (image_cache_revision, set_image_cache_revision) = signal(0_u64);
    let (ladder_layout_revision, set_ladder_layout_revision) = signal(0_u64);
    let (pattern_list_document, set_pattern_list_document) = signal(None::<PatternListDocument>);
    let (pattern_list_visible, set_pattern_list_visible) = signal(false);
    let (pattern_list_dialog, set_pattern_list_dialog) = signal(None::<PatternListDialogAction>);
    let (pattern_list_dialog_text, set_pattern_list_dialog_text) = signal(String::new());
    let (pattern_list_drag_index, set_pattern_list_drag_index) = signal(None::<usize>);
    let (generator_form, set_generator_form) = signal(GeneratorForm::default());
    let (generator_running, set_generator_running) = signal(false);
    let (generator_abort, set_generator_abort) = signal(None::<AbortController>);
    let (transitioner_form, set_transitioner_form) = signal(TransitionerForm::default());
    let (transitioner_running, set_transitioner_running) = signal(false);
    let (transitioner_abort, set_transitioner_abort) = signal(None::<AbortController>);
    let (prop_colors_open, set_prop_colors_open) = signal(false);
    let (pattern_transform_open, set_pattern_transform_open) = signal(false);
    let (about_open, set_about_open) = signal(false);
    let (share_copied, set_share_copied) = signal(false);

    {
        let image_cache_event = Closure::wrap(Box::new(move |_event: Event| {
            set_image_cache_revision.update(|revision| *revision = revision.wrapping_add(1));
            let errors = canvas::take_image_errors();
            if errors.len() == 1 {
                set_status.set(format!(
                    "Unable to load image prop: {}",
                    image_source_label(&errors[0])
                ));
            } else if !errors.is_empty() {
                set_status.set(format!("Unable to load {} image props", errors.len()));
            }
        }) as Box<dyn FnMut(Event)>);
        if let Some(window) = window() {
            window
                .add_event_listener_with_callback(
                    "jugglinglab-image-cache",
                    image_cache_event.as_ref().unchecked_ref(),
                )
                .ok();
        }
        image_cache_event.forget();
    }

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

    let prop_colors_available = Memo::new(move |_| {
        current_record
            .get()
            .as_ref()
            .is_some_and(record_props_are_colorable)
    });

    let current_spec = Memo::new(move |_| {
        current_record
            .get()
            .and_then(|record| AnimationSpec::from_record(&record).ok())
            .unwrap_or_else(AnimationSpec::fallback)
    });

    Effect::new(move |_| {
        let enabled =
            show_ground_for_pattern(animation_prefs.get().show_ground, &current_spec.get());
        if show_grid.get_untracked() != enabled {
            set_show_grid.set(enabled);
        }
    });

    Effect::new(move |_| {
        if view_mode.get() != "selection" {
            return;
        }
        let Some(record) = current_record.get() else {
            set_selection_records.set(Vec::new());
            return;
        };
        match selection_mutation_records(&record, &mutator_options.get()) {
            Ok(variants) => set_selection_records.set(variants),
            Err(err) => {
                set_selection_records.set(Vec::new());
                set_status.set(err);
            }
        }
    });

    Effect::new(move |_| {
        let _image_cache_revision = image_cache_revision.get();
        let spec = ladder_preview_spec
            .get()
            .unwrap_or_else(|| current_spec.get());
        if view_mode.get() == "selection" {
            let entries = selection_records
                .get()
                .into_iter()
                .enumerate()
                .filter_map(|(index, record)| {
                    let spec = AnimationSpec::from_record(&record).ok()?;
                    Some((
                        format!("selection-canvas-{index}"),
                        spec,
                        RenderSettings {
                            theme: theme.get(),
                            speed: speed.get(),
                            zoom: zoom.get(),
                            camera_yaw: camera_yaw.get(),
                            camera_pitch: camera_pitch.get(),
                            camera_pan_x: camera_pan_x.get(),
                            camera_pan_y: camera_pan_y.get(),
                            camera_pan_z: camera_pan_z.get(),
                            paused: !playing.get(),
                            show_trails: show_trails.get(),
                            show_grid: show_grid.get(),
                            show_title: true,
                            show_axes: true,
                            stereo: stereo.get(),
                            catch_sound: catch_sound.get(),
                            bounce_sound: bounce_sound.get(),
                            border_pixels: animation_prefs.get().border_pixels,
                            hide_jugglers: animation_prefs.get().hide_jugglers.clone(),
                            selected_event: None,
                            selected_position: None,
                            position_edit_handle: None,
                        },
                    ))
                })
                .collect();
            canvas::start_group_by_ids(entries);
            return;
        }
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
            show_title: true,
            show_axes: true,
            stereo: stereo.get(),
            catch_sound: catch_sound.get(),
            bounce_sound: bounce_sound.get(),
            border_pixels: animation_prefs.get().border_pixels,
            hide_jugglers: animation_prefs.get().hide_jugglers.clone(),
            selected_event: selected_ladder_event_selection(&spec, &selected_ladder.get()),
            selected_position: selected_ladder_position_index(&spec, &selected_ladder.get()),
            position_edit_handle: position_canvas_drag.get().map(|drag| drag.hit.handle),
        };
        canvas::start_by_id("juggling-stage", spec, settings);
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
                show_title: true,
                show_axes: true,
                stereo: stereo.get_untracked(),
                catch_sound: catch_sound.get_untracked(),
                bounce_sound: bounce_sound.get_untracked(),
                border_pixels: animation_prefs.get_untracked().border_pixels,
                hide_jugglers: animation_prefs.get_untracked().hide_jugglers.clone(),
                selected_event: selected_ladder_event_selection(
                    &current_spec.get_untracked(),
                    &selected_ladder.get_untracked(),
                ),
                selected_position: selected_ladder_position_index(
                    &current_spec.get_untracked(),
                    &selected_ladder.get_untracked(),
                ),
                position_edit_handle: position_canvas_drag
                    .get_untracked()
                    .map(|drag| drag.hit.handle),
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

    let apply_prop_colors = move |color_string: String| {
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        match color_props_in_record(&record, &color_string) {
            Ok(edited) => {
                commit_ladder_record(edited);
                set_prop_colors_open.set(false);
                set_status.set(match color_string.as_str() {
                    "mixed" => "Applied mixed prop colors".to_string(),
                    "orbits" => "Colored props by orbit".to_string(),
                    color => format!("Applied {color} to all props"),
                });
            }
            Err(error) => set_status.set(error),
        }
    };

    let apply_pattern_transform = move |transform: PatternTransform| {
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        match transform_pattern_record(&record, transform) {
            Ok(edited) => {
                commit_ladder_record(edited);
                set_pattern_transform_open.set(false);
                set_status.set(transform.status().to_string());
            }
            Err(error) => set_status.set(error),
        }
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
            if editor_shortcut_target_is_editable(&event) {
                return;
            }
            let key = event.key().to_ascii_lowercase();
            if matches!(key.as_str(), " " | "space" | "spacebar")
                && !(event.ctrl_key() || event.meta_key() || event.alt_key())
            {
                event.prevent_default();
                if event.repeat() {
                    return;
                }
                let resume = !playing.get_untracked();
                set_playing.set(resume);
                set_status.set(if resume {
                    "Animation resumed".to_string()
                } else {
                    "Animation stopped".to_string()
                });
                return;
            }
            if !(event.ctrl_key() || event.meta_key()) {
                return;
            }
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

    let import_jml = move |xml: String, filename: String| match jml::parse_jml(&xml) {
        Ok(imported) if imported.is_pattern_list => {
            if !confirm_pattern_list_replacement(pattern_list_document.get_untracked().as_ref()) {
                set_status.set("Pattern list import cancelled".to_string());
                return;
            }
            let count = imported.records.len();
            let title = imported
                .title
                .unwrap_or_else(|| filename.trim_end_matches(".jml").to_string());
            set_pattern_list_document.set(Some(PatternListDocument {
                title,
                info: imported.info,
                records: imported.records,
                selected: None,
                dirty: false,
            }));
            set_pattern_list_visible.set(true);
            set_status.set(format!("Opened pattern list with {count} lines"));
        }
        Ok(imported) => {
            let added = imported.records.len();
            if added > 0 {
                checkpoint_editor();
            }
            set_records.update(|records| {
                let insert_at = records.len();
                records.extend(imported.records);
                if added > 0 {
                    set_selected.set(insert_at);
                }
            });
            set_status.set(format!("Imported {added} JML pattern"));
            set_pattern_text.set(String::new());
        }
        Err(err) => set_status.set(err),
    };

    let apply_animation_preferences =
        move |prefs: AnimationPrefs, spec: &AnimationSpec, cold_restart: bool| {
            set_speed.set(playback_speed(&prefs));
            set_stereo.set(prefs.stereo);
            set_catch_sound.set(prefs.catch_sound);
            set_bounce_sound.set(prefs.bounce_sound);
            set_show_grid.set(show_ground_for_pattern(prefs.show_ground, spec));
            if prefs.catch_sound {
                crate::audio::prepare_catch();
            }
            if prefs.bounce_sound {
                crate::audio::prepare_bounce();
            }

            if cold_restart {
                let should_play = !prefs.start_paused;
                if prefs.mouse_pause {
                    set_mouse_pause_was_playing.set(Some(should_play));
                    set_playing.set(false);
                } else {
                    set_mouse_pause_was_playing.set(None);
                    set_playing.set(should_play);
                }
                let (yaw, pitch) = initial_camera_angles(spec, &prefs);
                set_camera_yaw.set(yaw);
                set_camera_pitch.set(pitch);
                set_camera_pan_x.set(0.0);
                set_camera_pan_y.set(0.0);
                set_camera_pan_z.set(0.0);
                set_zoom.set(1.0);
                set_view_mode.set(initial_view_mode(spec, prefs.default_view).to_string());
            } else if !prefs.mouse_pause {
                if let Some(was_playing) = mouse_pause_was_playing.get_untracked() {
                    set_playing.set(was_playing);
                }
                set_mouse_pause_was_playing.set(None);
            }

            set_animation_prefs.set(prefs);
        };

    let run_pattern = move |_| {
        let config = draft.get_untracked();
        let (pattern_config, prefs) = match split_animation_prefs(&config) {
            Ok(result) => result,
            Err(err) => {
                set_status.set(err);
                return;
            }
        };
        match record_from_config_or_current_jml(&pattern_config, current_record.get_untracked()) {
            Ok((mut record, message)) => {
                let serialized_prefs = prefs.to_string();
                record.animprefs = (!serialized_prefs.is_empty()).then_some(serialized_prefs);
                let spec = match AnimationSpec::from_record(&record) {
                    Ok(spec) => spec,
                    Err(err) => {
                        set_status.set(err);
                        return;
                    }
                };
                checkpoint_editor();
                set_records.update(|records| {
                    records.push(record);
                    set_selected.set(records.len() - 1);
                });
                apply_animation_preferences(prefs, &spec, true);
                set_status.set(message);
                set_pattern_text.set(pattern_config);
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
        let prefs = match AnimationPrefs::parse(record.animprefs.as_deref()) {
            Ok(prefs) => prefs,
            Err(err) => {
                set_status.set(format!("Invalid animation preferences: {err}"));
                return;
            }
        };
        let spec = match AnimationSpec::from_record(&record) {
            Ok(spec) => spec,
            Err(err) => {
                set_status.set(err);
                return;
            }
        };

        set_selected.set(idx);
        apply_animation_preferences(prefs, &spec, true);
        set_status.set(format!("Loaded {}", record.display));
        if let Some(config) = record.config.clone() {
            set_draft.set(config);
        }
        let source = default_pattern_source(&record);
        set_pattern_source.set(source.to_string());
        set_pattern_text.set(record_text_for_source(&record, source));
    };

    let activate_pattern_list_record = move |record: PatternRecord| {
        if !record.is_playable() {
            return;
        }
        let prefs = match AnimationPrefs::parse(record.animprefs.as_deref()) {
            Ok(prefs) => prefs,
            Err(err) => {
                set_status.set(format!("Invalid animation preferences: {err}"));
                return;
            }
        };
        let spec = match AnimationSpec::from_record(&record) {
            Ok(spec) => spec,
            Err(err) => {
                set_status.set(err);
                return;
            }
        };
        let mut index = None;
        set_records.update(|records| {
            index = records.iter().position(|candidate| candidate == &record);
            if index.is_none() {
                records.push(record.clone());
                index = Some(records.len() - 1);
            }
        });
        if let Some(index) = index {
            set_selected.set(index);
        }
        apply_animation_preferences(prefs, &spec, true);
        if let Some(config) = record.config.clone() {
            set_draft.set(config);
        }
        let source = default_pattern_source(&record);
        set_pattern_source.set(source.to_string());
        set_pattern_text.set(record_text_for_source(&record, source));
        set_status.set(format!("Loaded {}", record.display));
    };

    let open_pattern_list = move |_| {
        if pattern_list_document.get_untracked().is_none() {
            set_pattern_list_document.set(Some(PatternListDocument {
                title: "Untitled Pattern List".to_string(),
                info: None,
                records: Vec::new(),
                selected: None,
                dirty: false,
            }));
        }
        set_pattern_list_visible.set(true);
    };

    let new_pattern_list = move |_| {
        if !confirm_pattern_list_replacement(pattern_list_document.get_untracked().as_ref()) {
            return;
        }
        set_pattern_list_document.set(Some(PatternListDocument {
            title: "Untitled Pattern List".to_string(),
            info: None,
            records: Vec::new(),
            selected: None,
            dirty: false,
        }));
        set_pattern_list_visible.set(true);
        set_status.set("New pattern list".to_string());
    };

    let insert_current_in_pattern_list = move |_| {
        let Some(mut record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        let serialized_prefs = animation_prefs.get_untracked().to_string();
        record.animprefs = (!serialized_prefs.is_empty()).then_some(serialized_prefs);
        set_pattern_list_document.update(|document| {
            let Some(document) = document else {
                return;
            };
            let index = document.selected.unwrap_or(document.records.len());
            let index = index.min(document.records.len());
            document.records.insert(index, record);
            document.selected = Some(index);
            document.dirty = true;
        });
        set_status.set("Current pattern inserted into list".to_string());
    };

    let open_insert_pattern_list_text = move |_| {
        let index = pattern_list_document
            .get_untracked()
            .as_ref()
            .map(|document| document.selected.unwrap_or(document.records.len()))
            .unwrap_or(0);
        set_pattern_list_dialog_text.set(String::new());
        set_pattern_list_dialog.set(Some(PatternListDialogAction::InsertText { index }));
    };

    let remove_pattern_list_line = move |_| {
        set_pattern_list_document.update(|document| {
            let Some(document) = document else {
                return;
            };
            let Some(index) = document
                .selected
                .filter(|index| *index < document.records.len())
            else {
                return;
            };
            document.records.remove(index);
            document.selected = if document.records.is_empty() {
                None
            } else {
                Some(index.min(document.records.len() - 1))
            };
            document.dirty = true;
        });
        set_status.set("Pattern list line removed".to_string());
    };

    let apply_pattern_list_dialog = move |_| {
        let Some(action) = pattern_list_dialog.get_untracked() else {
            return;
        };
        let text = pattern_list_dialog_text.get_untracked();
        set_pattern_list_document.update(|document| {
            let Some(document) = document else {
                return;
            };
            match action {
                PatternListDialogAction::InsertText { index } => {
                    let index = index.min(document.records.len());
                    document.records.insert(index, text_record(text.clone()));
                    document.selected = Some(index);
                }
                PatternListDialogAction::EditDisplay { index } => {
                    if let Some(record) = document.records.get_mut(index) {
                        record.display = text.clone();
                        document.selected = Some(index);
                    }
                }
            }
            document.dirty = true;
        });
        set_pattern_list_dialog.set(None);
        set_status.set(match action {
            PatternListDialogAction::InsertText { .. } => "Text line inserted".to_string(),
            PatternListDialogAction::EditDisplay { .. } => "Display text changed".to_string(),
        });
    };

    let export_current = move |_| {
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        set_status.set("Packaging pattern resources...".to_string());
        spawn_local(async move {
            match portable_pattern_jml(&record).await {
                Ok(xml) => {
                    download_text("jugglinglab-pattern.jml", &xml);
                    set_status.set("Current pattern exported as portable JML".to_string());
                }
                Err(error) => set_status.set(error),
            }
        });
    };

    let open_share = move |_| {
        let Some(record) = current_record.get_untracked() else {
            set_status.set("No current pattern selected".to_string());
            return;
        };
        let base_url = match current_share_base_url() {
            Ok(base_url) => base_url,
            Err(error) => {
                set_status.set(error);
                return;
            }
        };
        let url = match share::build_share_url(&base_url, &record, &animation_prefs.get_untracked())
        {
            Ok(url) => url,
            Err(error) => {
                set_status.set(error);
                return;
            }
        };
        let Some(browser_window) = window() else {
            set_status.set("Browser window is unavailable for sharing".to_string());
            return;
        };
        let history = match browser_window.history() {
            Ok(history) => history,
            Err(error) => {
                set_status.set(js_error_message("Unable to update the share URL", error));
                return;
            }
        };
        if let Err(error) = history.replace_state_with_url(&JsValue::NULL, "", Some(&url)) {
            set_status.set(js_error_message("Unable to update the share URL", error));
            return;
        }
        let navigator = browser_window.navigator();
        let Some(clipboard) = Reflect::get(navigator.as_ref(), &JsValue::from_str("clipboard"))
            .ok()
            .and_then(|value| value.dyn_into::<Clipboard>().ok())
        else {
            set_status
                .set("Clipboard is unavailable; the share URL is in the address bar".to_string());
            return;
        };
        let promise = clipboard.write_text(&url);
        spawn_local(async move {
            match JsFuture::from(promise).await {
                Ok(_) => {
                    set_status.set("Share URL copied".to_string());
                    set_share_copied.set(true);
                    let hide_tooltip = Closure::once_into_js(move || {
                        set_share_copied.set(false);
                    });
                    if let Some(window) = window() {
                        window
                            .set_timeout_with_callback_and_timeout_and_arguments_0(
                                hide_tooltip.unchecked_ref(),
                                1800,
                            )
                            .ok();
                    }
                }
                Err(_) => set_status.set(
                    "Clipboard access denied; the share URL is in the address bar".to_string(),
                ),
            }
        });
    };

    let export_all = move |_| {
        let Some(document) = pattern_list_document.get_untracked() else {
            set_status.set("No pattern list is open".to_string());
            return;
        };
        let filename = pattern_list_filename(&document.title, "jml");
        set_status.set("Packaging pattern list resources...".to_string());
        spawn_local(async move {
            match portable_pattern_list_records(document.records.clone()).await {
                Ok(records) => {
                    download_text(
                        &filename,
                        &jml::write_pattern_list_document(
                            Some(&document.title),
                            document.info.as_deref(),
                            &records,
                        ),
                    );
                    set_pattern_list_document.update(|document| {
                        if let Some(document) = document {
                            document.dirty = false;
                        }
                    });
                    set_status.set("Pattern list exported as portable JML".to_string());
                }
                Err(error) => set_status.set(error),
            }
        });
    };

    let export_pattern_list_text = move |_| {
        let Some(document) = pattern_list_document.get_untracked() else {
            set_status.set("No pattern list is open".to_string());
            return;
        };
        download_text(
            &pattern_list_filename(&document.title, "txt"),
            &jml::write_pattern_list_text(&document.records),
        );
        set_status.set("Pattern list exported as text".to_string());
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

        let filename = file.name();
        let reader_clone = reader.clone();
        let onload = Closure::wrap(Box::new(move |_event: Event| {
            if let Ok(result) = reader_clone.result() {
                if let Some(text) = result.as_string() {
                    import_jml(text, filename.clone());
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

    let start_canvas_drag = move |event: ev::PointerEvent| {
        event.prevent_default();
        if let Some(canvas) = event
            .target()
            .and_then(|target| target.dyn_into::<HtmlCanvasElement>().ok())
        {
            canvas.focus().ok();
            canvas.set_pointer_capture(event.pointer_id()).ok();
        }
        if let Some(hit) = canvas::event_editor_hit_by_id(
            "juggling-stage",
            event.client_x() as f64,
            event.client_y() as f64,
        ) {
            let Some(record) = current_record.get_untracked() else {
                return;
            };
            let spec = current_spec.get_untracked();
            match event_drag_sources(&record, &spec, &hit) {
                Ok((start_image, start_primary)) => {
                    if let Some(event_id) = ladder_event_id_for_editor_hit(&spec, &hit) {
                        set_selected_ladder.set(event_id);
                    }
                    set_selected_object.set(format!(
                        "J{} {} event",
                        start_primary.juggler,
                        if hit.image_hand == 0 { "right" } else { "left" }
                    ));
                    set_event_canvas_drag.set(Some(EventCanvasDrag {
                        hit,
                        start_client_x: event.client_x() as f64,
                        start_client_y: event.client_y() as f64,
                        start_image,
                        start_primary,
                        original_record: record,
                        checkpointed: false,
                    }));
                    set_view_drag_start.set(None);
                    set_view_drag_camera.set(None);
                    set_view_dragged.set(false);
                    set_status.set("Editing event position".to_string());
                }
                Err(err) => set_status.set(err),
            }
            return;
        }
        if let Some(hit) = canvas::position_editor_hit_by_id(
            "juggling-stage",
            event.client_x() as f64,
            event.client_y() as f64,
        ) {
            let Some(record) = current_record.get_untracked() else {
                return;
            };
            let spec = current_spec.get_untracked();
            let Some(start_position) = (match &spec.kind {
                AnimationKind::Jml(jml) => jml.positions.get(hit.position_index).copied(),
                AnimationKind::Unavailable(_) => None,
            }) else {
                return;
            };
            set_selected_ladder.set(format!("position-{}", hit.position_index + 1));
            set_selected_object.set(format!("J{} position", start_position.juggler));
            set_position_canvas_drag.set(Some(PositionCanvasDrag {
                hit,
                start_client_x: event.client_x() as f64,
                start_client_y: event.client_y() as f64,
                start_position,
                original_record: record,
                checkpointed: false,
            }));
            set_view_drag_start.set(None);
            set_view_drag_camera.set(None);
            set_view_dragged.set(false);
            set_status.set("Editing juggler position".to_string());
            return;
        }
        set_view_drag_start.set(Some((event.client_x() as f64, event.client_y() as f64)));
        set_view_drag_camera.set(Some((
            camera_yaw.get_untracked(),
            camera_pitch.get_untracked(),
        )));
        set_view_dragged.set(false);
    };

    let drag_canvas_view = move |event: ev::PointerEvent| {
        if let Some(mut drag) = event_canvas_drag.get_untracked() {
            event.prevent_default();
            let dx = event.client_x() as f64 - drag.start_client_x;
            let dy = event.client_y() as f64 - drag.start_client_y;
            if dx.abs() + dy.abs() <= 0.0 {
                return;
            }
            if !drag.checkpointed {
                checkpoint_editor();
                drag.checkpointed = true;
            }
            let primary = event_from_canvas_drag(&drag, dx, dy);
            match edit_ladder_event_spatial_in_record(
                &drag.original_record,
                drag.hit.primary_index,
                primary,
            ) {
                Ok(edited) => {
                    replace_current_ladder_record(
                        edited,
                        selected,
                        set_selected,
                        set_records,
                        set_pattern_source,
                        set_pattern_text,
                        set_draft,
                    );
                    set_status.set(event_edit_status(drag.hit.handle, primary));
                }
                Err(err) => set_status.set(err),
            }
            set_event_canvas_drag.set(Some(drag));
            set_view_dragged.set(true);
            return;
        }
        if let Some(mut drag) = position_canvas_drag.get_untracked() {
            event.prevent_default();
            let dx = event.client_x() as f64 - drag.start_client_x;
            let dy = event.client_y() as f64 - drag.start_client_y;
            if dx.abs() + dy.abs() <= 0.0 {
                return;
            }
            if !drag.checkpointed {
                checkpoint_editor();
                drag.checkpointed = true;
            }
            let position = position_from_canvas_drag(&drag, dx, dy);
            match edit_ladder_position_spatial_in_record(
                &drag.original_record,
                drag.hit.position_index,
                position,
            ) {
                Ok(edited) => {
                    replace_current_ladder_record(
                        edited,
                        selected,
                        set_selected,
                        set_records,
                        set_pattern_source,
                        set_pattern_text,
                        set_draft,
                    );
                    set_selected_ladder.set(format!("position-{}", drag.hit.position_index + 1));
                    set_status.set(position_edit_status(drag.hit.handle, position));
                }
                Err(err) => set_status.set(err),
            }
            set_position_canvas_drag.set(Some(drag));
            set_view_dragged.set(true);
            return;
        }
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
            let (raw_yaw, raw_pitch) = view_drag_camera
                .get_untracked()
                .unwrap_or((camera_yaw.get_untracked(), camera_pitch.get_untracked()));
            let raw_yaw = (raw_yaw + dx * 0.008).rem_euclid(std::f64::consts::TAU);
            let raw_pitch = (raw_pitch - dy * 0.008).clamp(CAMERA_MIN_PITCH, CAMERA_MAX_PITCH);
            set_view_drag_camera.set(Some((raw_yaw, raw_pitch)));
            let reference = camera_snap_reference(
                &current_spec.get_untracked(),
                &selected_ladder.get_untracked(),
                canvas::playback_time(&current_spec.get_untracked()),
            );
            let (snapped_yaw, snapped_pitch) = snap_camera_angles(raw_yaw, raw_pitch, reference);
            set_camera_yaw.set(snapped_yaw);
            set_camera_pitch.set(snapped_pitch);
            set_view_drag_start.set(Some((x, y)));
        }
    };

    let end_canvas_drag = move |event: ev::PointerEvent| {
        event.prevent_default();
        if let Some(canvas) = window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("juggling-stage"))
        {
            if canvas.has_pointer_capture(event.pointer_id()) {
                canvas.release_pointer_capture(event.pointer_id()).ok();
            }
        }
        if let Some(drag) = event_canvas_drag.get_untracked() {
            set_event_canvas_drag.set(None);
            set_status.set(if drag.checkpointed {
                "Event position changed".to_string()
            } else {
                "Event selected".to_string()
            });
            return;
        }
        if let Some(drag) = position_canvas_drag.get_untracked() {
            set_position_canvas_drag.set(None);
            set_status.set(if drag.checkpointed {
                "Juggler position changed".to_string()
            } else {
                "Juggler position selected".to_string()
            });
            return;
        }
        if view_drag_start.get_untracked().is_some() {
            set_status.set("View adjusted".to_string());
        }
        set_view_drag_start.set(None);
        set_view_drag_camera.set(None);
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
        let spec = current_spec.get_untracked();
        let prefs = animation_prefs.get_untracked();
        let (yaw, pitch) = initial_camera_angles(&spec, &prefs);
        set_zoom.set(1.0);
        set_camera_yaw.set(yaw);
        set_camera_pitch.set(pitch);
        set_camera_pan_x.set(0.0);
        set_camera_pan_y.set(0.0);
        set_camera_pan_z.set(0.0);
        set_view_drag_camera.set(None);
        set_status.set("View reset".to_string());
    };

    let open_animation_prefs = move |_| {
        set_animation_prefs_dialog.set(Some(AnimationPrefsDialogState::from_prefs(
            &animation_prefs.get_untracked(),
        )));
    };

    let reset_animation_prefs = move |_| {
        set_animation_prefs_dialog.set(Some(AnimationPrefsDialogState::from_prefs(
            &AnimationPrefs::default(),
        )));
    };

    let confirm_animation_prefs = move |_| {
        let Some(dialog) = animation_prefs_dialog.get_untracked() else {
            return;
        };
        match dialog.to_prefs() {
            Ok(prefs) => {
                apply_animation_preferences(prefs, &current_spec.get_untracked(), false);
                set_animation_prefs_dialog.set(None);
                set_status.set("Animation preferences changed".to_string());
            }
            Err(err) => set_animation_prefs_dialog.update(|dialog| {
                if let Some(dialog) = dialog {
                    dialog.error = Some(err.clone());
                }
            }),
        }
    };

    let open_animation_export = move |_| {
        set_animation_export_progress.set((0, 0));
        set_animation_export_dialog.set(Some(AnimationExportDialogState::from_prefs(
            &animation_prefs.get_untracked(),
        )));
    };

    let cancel_animation_export = move |_| {
        if let Some(cancel) = animation_export_cancel.get_untracked() {
            cancel.store(true, Ordering::Relaxed);
            set_status.set("Cancelling animation export".to_string());
        } else {
            set_animation_export_dialog.set(None);
        }
    };

    let start_animation_export = move |_| {
        let Some(dialog) = animation_export_dialog.get_untracked() else {
            return;
        };
        let prefs = animation_prefs.get_untracked();
        let options = match dialog.to_options(prefs.slowdown) {
            Ok(options) => options,
            Err(error) => {
                set_animation_export_dialog.update(|dialog| {
                    if let Some(dialog) = dialog {
                        dialog.error = Some(error.clone());
                    }
                });
                return;
            }
        };
        let spec = current_spec.get_untracked();
        let settings = RenderSettings {
            theme: theme.get_untracked(),
            speed: 1.0,
            zoom: zoom.get_untracked(),
            camera_yaw: camera_yaw.get_untracked(),
            camera_pitch: camera_pitch.get_untracked(),
            camera_pan_x: camera_pan_x.get_untracked(),
            camera_pan_y: camera_pan_y.get_untracked(),
            camera_pan_z: camera_pan_z.get_untracked(),
            paused: true,
            show_trails: show_trails.get_untracked(),
            show_grid: show_grid.get_untracked(),
            show_title: options.show_title,
            show_axes: false,
            stereo: stereo.get_untracked(),
            catch_sound: false,
            bounce_sound: false,
            border_pixels: prefs.border_pixels,
            hide_jugglers: prefs.hide_jugglers,
            selected_event: None,
            selected_position: None,
            position_edit_handle: None,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        set_animation_export_cancel.set(Some(Arc::clone(&cancelled)));
        set_animation_export_running.set(true);
        set_animation_export_progress.set((0, 0));
        set_animation_export_dialog.update(|dialog| {
            if let Some(dialog) = dialog {
                dialog.error = None;
            }
        });
        set_status.set("Exporting animation".to_string());

        let progress: Rc<dyn Fn(usize, usize)> = Rc::new(move |current, total| {
            set_animation_export_progress.set((current, total));
        });
        spawn_local(async move {
            let result =
                export::export_animation(spec, settings, options, cancelled, progress).await;
            set_animation_export_running.set(false);
            set_animation_export_cancel.set(None);
            match result {
                Ok(ExportedAnimation { blob, filename }) => match download_blob(&filename, &blob) {
                    Ok(()) => {
                        set_animation_export_dialog.set(None);
                        set_status.set(format!("Animation exported as {filename}"));
                    }
                    Err(error) => set_animation_export_dialog.update(|dialog| {
                        if let Some(dialog) = dialog {
                            dialog.error = Some(error.clone());
                        }
                    }),
                },
                Err(error) if error == "Animation export cancelled" => {
                    set_animation_export_dialog.set(None);
                    set_status.set(error);
                }
                Err(error) => {
                    set_status.set("Animation export failed".to_string());
                    set_animation_export_dialog.update(|dialog| {
                        if let Some(dialog) = dialog {
                            dialog.error = Some(error.clone());
                        }
                    });
                }
            }
        });
    };

    let restore_mouse_pause = move |_| {
        if !animation_prefs.get_untracked().mouse_pause {
            return;
        }
        if let Some(was_playing) = mouse_pause_was_playing.get_untracked() {
            set_playing.set(was_playing);
            set_mouse_pause_was_playing.set(None);
        }
    };

    let apply_mouse_pause = move |_| {
        if !animation_prefs.get_untracked().mouse_pause
            || mouse_pause_was_playing.get_untracked().is_some()
        {
            return;
        }
        set_mouse_pause_was_playing.set(Some(playing.get_untracked()));
        set_playing.set(false);
    };

    let start_selection_canvas_drag = move |event: ev::PointerEvent| {
        event.prevent_default();
        if let Some(canvas) = event
            .target()
            .and_then(|target| target.dyn_into::<HtmlCanvasElement>().ok())
        {
            canvas.set_pointer_capture(event.pointer_id()).ok();
        }
        set_view_drag_start.set(Some((event.client_x() as f64, event.client_y() as f64)));
        set_view_drag_camera.set(Some((
            camera_yaw.get_untracked(),
            camera_pitch.get_untracked(),
        )));
        set_view_dragged.set(false);
    };

    let choose_selection_variant = move |index: usize| {
        if view_dragged.get_untracked() {
            set_view_dragged.set(false);
            return;
        }
        if index == 4 {
            return;
        }
        let Some(record) =
            selection_records.with_untracked(|variants| variants.get(index).cloned())
        else {
            return;
        };
        checkpoint_editor();
        replace_current_ladder_record(
            record,
            selected,
            set_selected,
            set_records,
            set_pattern_source,
            set_pattern_text,
            set_draft,
        );
        set_status.set("Mutation selected; surrounding variants regenerated".to_string());
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

    let zoom_ladder_at = move |requested_zoom: f64, anchor_client_y: f64| {
        set_ladder_auto_fit.set(false);
        let old_zoom = ladder_zoom.get_untracked();
        let new_zoom = requested_zoom.clamp(LADDER_MIN_ZOOM, LADDER_MAX_ZOOM);
        if (new_zoom - old_zoom).abs() < 1e-9 {
            return;
        }
        let Some(document) = window().and_then(|window| window.document()) else {
            set_ladder_zoom.set(new_zoom);
            return;
        };
        let Some(scroll) = document.get_element_by_id("ladder-scroll") else {
            set_ladder_zoom.set(new_zoom);
            return;
        };
        let Some(svg) = document.get_element_by_id("ladder-svg") else {
            set_ladder_zoom.set(new_zoom);
            return;
        };
        let scroll_rect = scroll.get_bounding_client_rect();
        let svg_height = svg.get_bounding_client_rect().height().max(1.0);
        let anchor_y = (anchor_client_y - scroll_rect.top()).clamp(0.0, scroll_rect.height());
        let unit_px = svg_height / ladder_view_height(old_zoom);
        let target = ladder_scroll_target(
            old_zoom,
            new_zoom,
            svg_height,
            scroll.scroll_top() as f64,
            anchor_y,
            LADDER_TOP_Y * unit_px,
            LADDER_BOTTOM_MARGIN * unit_px,
        );
        set_ladder_zoom.set(new_zoom);

        let callback_scroll = scroll.clone();
        let callback = Closure::once_into_js(move || {
            callback_scroll.set_scroll_top(target.round() as i32);
        });
        let scheduled = window().is_some_and(|window| {
            window
                .request_animation_frame(callback.unchecked_ref())
                .is_ok()
        });
        if !scheduled {
            scroll.set_scroll_top(target.round() as i32);
        }
        set_status.set(format!("Ladder zoom {:.0}%", new_zoom * 100.0));
    };

    Effect::new(move |_| {
        let Some(scroll) = window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("ladder-scroll"))
        else {
            return;
        };

        if ladder_auto_fit.get_untracked() {
            fit_ladder_to_height(set_ladder_zoom, set_status, false);
        }
        set_ladder_layout_revision.update(|revision| *revision = revision.wrapping_add(1));
        let callback = Closure::wrap(Box::new(move |_entries: js_sys::Array| {
            set_ladder_layout_revision.update(|revision| *revision = revision.wrapping_add(1));
            if ladder_auto_fit.get_untracked() {
                fit_ladder_to_height(set_ladder_zoom, set_status, false);
            }
        }) as Box<dyn FnMut(js_sys::Array)>);
        let Ok(observer) = ResizeObserver::new(callback.as_ref().unchecked_ref()) else {
            return;
        };
        observer.observe(&scroll);
        callback.forget();
        std::mem::forget(observer);
    });

    let zoom_ladder_wheel = move |event: ev::WheelEvent| {
        event.prevent_default();
        let units = match event.delta_mode() {
            1 => event.delta_y().abs() / 3.0,
            2 => event.delta_y().abs(),
            _ => event.delta_y().abs() / 100.0,
        };
        let mut factor = 1.0 + 0.05 * units.max(0.01);
        if event.delta_y() < 0.0 {
            factor = factor.recip();
        }
        zoom_ladder_at(
            ladder_zoom.get_untracked() * factor,
            event.client_y() as f64,
        );
    };

    let register_ladder_touch = move |event: ev::PointerEvent| {
        if event.pointer_type() != "touch" {
            return;
        }
        capture_ladder_pointer(event.pointer_id());
        let touch = LadderTouch {
            pointer_id: event.pointer_id(),
            client_x: event.client_x() as f64,
            client_y: event.client_y() as f64,
        };
        let mut active = Vec::new();
        set_ladder_touches.update(|touches| {
            if let Some(existing) = touches
                .iter_mut()
                .find(|existing| existing.pointer_id == touch.pointer_id)
            {
                *existing = touch;
            } else {
                touches.push(touch);
            }
            active = touches.clone();
        });
        if active.len() < 2 {
            return;
        }

        event.prevent_default();
        set_ladder_long_press.set(None);
        if let Some(drag) = ladder_drag.get_untracked() {
            release_ladder_pointer(drag.pointer_id);
            if let LadderDragKind::Tracker { was_playing, .. } = drag.kind {
                set_playing.set(was_playing);
            }
        }
        set_ladder_drag.set(None);
        set_ladder_preview_spec.set(None);
        set_ladder_scroll_drag.set(None);
        set_ladder_pinch_distance.set(ladder_touch_distance(&active));
    };

    let preview_ladder_drag = move |event: ev::PointerEvent| {
        if event.pointer_type() == "touch" {
            let mut active = Vec::new();
            set_ladder_touches.update(|touches| {
                if let Some(touch) = touches
                    .iter_mut()
                    .find(|touch| touch.pointer_id == event.pointer_id())
                {
                    touch.client_x = event.client_x() as f64;
                    touch.client_y = event.client_y() as f64;
                }
                active = touches.clone();
            });
            if let Some(previous_distance) = ladder_pinch_distance.get_untracked() {
                event.prevent_default();
                if let (Some(distance), Some(anchor_y)) = (
                    ladder_touch_distance(&active),
                    ladder_touch_centroid_y(&active),
                ) {
                    if previous_distance > 1e-6 && distance > 1e-6 {
                        zoom_ladder_at(
                            ladder_zoom.get_untracked() * distance / previous_distance,
                            anchor_y,
                        );
                        set_ladder_pinch_distance.set(Some(distance));
                    }
                }
                return;
            }
        }
        if ladder_long_press.get_untracked().is_some_and(|pending| {
            pending.pointer_id == event.pointer_id()
                && ((event.client_x() as f64 - pending.client_x).powi(2)
                    + (event.client_y() as f64 - pending.client_y).powi(2))
                .sqrt()
                    > 8.0
        }) {
            set_ladder_long_press.set(None);
        }
        if let Some(scroll_drag) = ladder_scroll_drag.get_untracked() {
            if scroll_drag.pointer_id == event.pointer_id() {
                event.prevent_default();
                if let Some(scroll) = window()
                    .and_then(|window| window.document())
                    .and_then(|document| document.get_element_by_id("ladder-scroll"))
                {
                    let delta = scroll_drag.start_client_y - event.client_y() as f64;
                    scroll.set_scroll_top(scroll_drag.start_scroll_top + delta.round() as i32);
                }
                set_status.set("Ladder scrolled".to_string());
                return;
            }
        }
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
        if let Some(time) =
            ladder_time_from_client_y(event.client_y(), &diagram, ladder_zoom.get_untracked())
        {
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
                let preview = current_record.get_untracked().and_then(|record| {
                    let edited = match &drag.kind {
                        LadderDragKind::Event {
                            primary_index,
                            primary_time,
                        } => move_ladder_event_in_record(
                            &record,
                            *primary_index,
                            *primary_time + time - drag.start_time,
                        ),
                        LadderDragKind::Position(position_index) => {
                            move_ladder_position_in_record(&record, *position_index, time)
                        }
                        LadderDragKind::Tracker { .. } => unreachable!(),
                    };
                    match edited.and_then(|edited| AnimationSpec::from_record(&edited)) {
                        Ok(spec) => Some(spec),
                        Err(err) => {
                            set_status.set(err);
                            None
                        }
                    }
                });
                if let Some(preview) = preview {
                    set_ladder_preview_spec.set(Some(preview));
                    set_status.set(format!("Move ladder item to {time:.3}s"));
                } else {
                    set_ladder_preview_spec.set(None);
                }
            }
        }
    };

    let finish_ladder_drag = move |event: ev::PointerEvent| {
        if event.pointer_type() == "touch" {
            let mut active = Vec::new();
            set_ladder_touches.update(|touches| {
                touches.retain(|touch| touch.pointer_id != event.pointer_id());
                active = touches.clone();
            });
            if ladder_pinch_distance.get_untracked().is_some() {
                event.prevent_default();
                release_ladder_pointer(event.pointer_id());
                if active.is_empty() {
                    set_ladder_pinch_distance.set(None);
                    if ladder_zoom.get_untracked() < 1.1 {
                        zoom_ladder_at(1.0, event.client_y() as f64);
                    }
                }
                return;
            }
        }
        if ladder_long_press
            .get_untracked()
            .is_some_and(|pending| pending.pointer_id == event.pointer_id())
        {
            set_ladder_long_press.set(None);
        }
        if ladder_scroll_drag
            .get_untracked()
            .is_some_and(|drag| drag.pointer_id == event.pointer_id())
        {
            event.prevent_default();
            release_ladder_pointer(event.pointer_id());
            set_ladder_scroll_drag.set(None);
            set_status.set("Ladder scrolled".to_string());
            return;
        }
        let Some(drag) = ladder_drag.get_untracked() else {
            return;
        };
        if drag.pointer_id != event.pointer_id() {
            return;
        }
        event.prevent_default();
        release_ladder_pointer(drag.pointer_id);
        set_ladder_drag.set(None);
        set_ladder_preview_spec.set(None);

        let Some(diagram) = ladder_diagram(&current_spec.get_untracked()) else {
            set_status.set("No ladder data available for this pattern".to_string());
            return;
        };
        let time = if let Some(raw_time) =
            ladder_time_from_client_y(event.client_y(), &diagram, ladder_zoom.get_untracked())
        {
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
        if event.pointer_type() == "touch" {
            let mut active = Vec::new();
            set_ladder_touches.update(|touches| {
                touches.retain(|touch| touch.pointer_id != event.pointer_id());
                active = touches.clone();
            });
            if ladder_pinch_distance.get_untracked().is_some() {
                event.prevent_default();
                release_ladder_pointer(event.pointer_id());
                if active.is_empty() {
                    set_ladder_pinch_distance.set(None);
                    if ladder_zoom.get_untracked() < 1.1 {
                        zoom_ladder_at(1.0, event.client_y() as f64);
                    }
                }
                return;
            }
        }
        if ladder_long_press
            .get_untracked()
            .is_some_and(|pending| pending.pointer_id == event.pointer_id())
        {
            set_ladder_long_press.set(None);
        }
        if ladder_scroll_drag
            .get_untracked()
            .is_some_and(|drag| drag.pointer_id == event.pointer_id())
        {
            event.prevent_default();
            release_ladder_pointer(event.pointer_id());
            set_ladder_scroll_drag.set(None);
            set_status.set("Ladder scroll cancelled".to_string());
            return;
        }
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
            set_ladder_preview_spec.set(None);
            set_status.set("Ladder edit cancelled".to_string());
        }
    };

    let show_ladder_context = move |selected_id: String, client_x: f64, client_y: f64| {
        let Some(diagram) = ladder_diagram(&current_spec.get_untracked()) else {
            set_status.set("No ladder data available for this pattern".to_string());
            return;
        };
        set_selected_ladder.set(selected_id.clone());
        if ladder_popup_was_playing.get_untracked().is_none() {
            set_ladder_popup_was_playing.set(Some(playing.get_untracked()));
        }
        set_ladder_prop_edit_time.set(canvas::playback_time(&current_spec.get_untracked()));
        set_playing.set(false);
        if let (Some(time), Some(juggler)) = (
            ladder_time_from_client_y(client_y as i32, &diagram, ladder_zoom.get_untracked()),
            ladder_juggler_from_client_x(client_x as i32, &diagram),
        ) {
            set_ladder_insert_target.set(Some(LadderInsertTarget { juggler, time }));
            let prop_cycle = ladder_playback_cycle(&diagram, ladder_prop_edit_time.get_untracked());
            let absolute_time = ladder_time_in_cycle(&diagram, prop_cycle, time);
            seek_renderer(absolute_time);
            set_playhead_time.set(absolute_time);
        }
        let (x, y) = ladder_context_position(client_x, client_y);
        set_ladder_context_menu.set(Some(LadderContextMenu { x, y }));
        set_status.set("Ladder actions".to_string());
    };

    let schedule_ladder_long_press = move |pointer_id: i32,
                                           is_touch: bool,
                                           selected_id: String,
                                           client_x: f64,
                                           client_y: f64| {
        if !is_touch {
            return;
        }
        let pending = LadderLongPress {
            pointer_id,
            selected_id,
            client_x,
            client_y,
        };
        set_ladder_long_press.set(Some(pending.clone()));
        let callback_pending = pending.clone();
        let callback = Closure::once_into_js(move || {
            if ladder_long_press.get_untracked().as_ref() != Some(&callback_pending) {
                return;
            }
            set_ladder_long_press.set(None);
            if ladder_scroll_drag
                .get_untracked()
                .is_some_and(|drag| drag.pointer_id == callback_pending.pointer_id)
            {
                release_ladder_pointer(callback_pending.pointer_id);
                set_ladder_scroll_drag.set(None);
            }
            if let Some(drag) = ladder_drag.get_untracked() {
                if drag.pointer_id == callback_pending.pointer_id {
                    release_ladder_pointer(drag.pointer_id);
                    if let LadderDragKind::Tracker { was_playing, .. } = drag.kind {
                        set_ladder_popup_was_playing.set(Some(was_playing));
                    }
                    set_ladder_drag.set(None);
                    set_ladder_preview_spec.set(None);
                }
            }
            show_ladder_context(
                callback_pending.selected_id,
                callback_pending.client_x,
                callback_pending.client_y,
            );
        });
        let scheduled = window().is_some_and(|window| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.unchecked_ref(),
                    550,
                )
                .is_ok()
        });
        if !scheduled {
            set_ladder_long_press.set(None);
        }
    };

    let start_ladder_tracker_drag = move |event: ev::PointerEvent| {
        if event.button() != 0 {
            return;
        }
        let target_class = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            .and_then(|target| target.get_attribute("class"))
            .unwrap_or_default();
        let is_tracker_handle = target_class
            .split_ascii_whitespace()
            .any(|class| class == "ladder-tracker-hitbox");
        let is_background = target_class
            .split_ascii_whitespace()
            .any(|class| class == "ladder-hotzone");
        if !is_tracker_handle && !is_background {
            return;
        }
        if event.ctrl_key() || event.meta_key() {
            event.prevent_default();
            show_ladder_context(
                String::new(),
                event.client_x() as f64,
                event.client_y() as f64,
            );
            return;
        }
        if event.pointer_type() == "touch" && !ladder_touches.with_untracked(Vec::is_empty) {
            return;
        }
        event.prevent_default();
        let scroll = window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("ladder-scroll"));
        let should_pan = !is_tracker_handle
            && scroll.as_ref().is_some_and(|scroll| {
                ladder_background_should_pan(
                    ladder_zoom.get_untracked(),
                    scroll.scroll_height(),
                    scroll.client_height(),
                )
            });
        if should_pan {
            let scroll = scroll.expect("pan requires the ladder scroll element");
            capture_ladder_pointer(event.pointer_id());
            set_ladder_scroll_drag.set(Some(LadderScrollDrag {
                pointer_id: event.pointer_id(),
                start_client_y: event.client_y() as f64,
                start_scroll_top: scroll.scroll_top(),
            }));
            schedule_ladder_long_press(
                event.pointer_id(),
                event.pointer_type() == "touch",
                String::new(),
                event.client_x() as f64,
                event.client_y() as f64,
            );
            set_status.set("Scroll ladder".to_string());
            return;
        }
        let Some(diagram) = ladder_diagram(&current_spec.get_untracked()) else {
            set_status.set("No ladder data available for this pattern".to_string());
            return;
        };
        let Some(time) =
            ladder_time_from_client_y(event.client_y(), &diagram, ladder_zoom.get_untracked())
        else {
            return;
        };
        let juggler = ladder_juggler_from_client_x(event.client_x(), &diagram).unwrap_or(1);
        let was_playing = playing.get_untracked();
        let prop_cycle = ladder_playback_cycle(
            &diagram,
            canvas::playback_time(&current_spec.get_untracked()),
        );
        capture_ladder_pointer(event.pointer_id());
        set_ladder_preview_spec.set(None);
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
        schedule_ladder_long_press(
            event.pointer_id(),
            event.pointer_type() == "touch",
            String::new(),
            event.client_x() as f64,
            event.client_y() as f64,
        );
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
        set_ladder_long_press.set(None);
        show_ladder_context(
            selected_id,
            event.client_x() as f64,
            event.client_y() as f64,
        );
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

        let prop_mod = match define_prop_modifier(&dialog) {
            Ok(modifier) => modifier,
            Err(error) => {
                set_status.set(error);
                return;
            }
        };
        match define_ladder_prop_in_record(
            &record,
            dialog.path,
            &dialog.prop_assignment,
            &dialog.prop_type,
            prop_mod.as_deref(),
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

    let choose_custom_prop_image = move |event: ev::Event| {
        let input = event_target::<HtmlInputElement>(&event);
        let Some(file) = input.files().and_then(|files| files.get(0)) else {
            return;
        };
        let Ok(reader) = FileReader::new() else {
            set_status.set("Could not read the selected image".to_string());
            return;
        };
        let reader_clone = reader.clone();
        let onload = Closure::wrap(Box::new(move |_event: Event| {
            let Some(data_url) = reader_clone
                .result()
                .ok()
                .and_then(|value| value.as_string())
            else {
                set_status.set("Could not decode the selected image".to_string());
                return;
            };
            set_define_prop_dialog.update(|dialog| {
                if let Some(dialog) = dialog {
                    dialog.prop_type = "image".to_string();
                    dialog.image_source = encode_image_source(&data_url);
                }
            });
            set_status.set("Custom prop image loaded".to_string());
        }) as Box<dyn FnMut(_)>);
        reader.set_onload(Some(onload.as_ref().unchecked_ref()));
        reader.read_as_data_url(&file).ok();
        onload.forget();
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

    let reset_generator = move |_| set_generator_form.set(GeneratorForm::default());

    let stop_generator = move |_| {
        if let Some(controller) = generator_abort.get_untracked() {
            controller.abort();
            set_status.set("Generator cancellation requested".to_string());
        }
    };

    let run_generator = move |_| {
        if generator_running.get_untracked() {
            return;
        }
        let arguments = generator_form.get_untracked().arguments();
        let Ok(controller) = AbortController::new() else {
            set_status.set("This browser cannot cancel generator requests".to_string());
            return;
        };
        let request_controller = controller.clone();
        set_generator_abort.set(Some(controller));
        set_generator_running.set(true);
        set_status.set("Generating siteswap patterns...".to_string());
        spawn_local(async move {
            let response = request_generation(arguments, request_controller.clone()).await;
            set_generator_running.set(false);
            set_generator_abort.set(None);
            match response {
                Ok(GenerationResult {
                    patterns,
                    stop_reason,
                }) => {
                    let count = patterns.len();
                    let records = patterns
                        .into_iter()
                        .map(|pattern| PatternRecord::siteswap(pattern.display, pattern.config))
                        .collect::<Vec<_>>();
                    set_pattern_list_document.set(Some(PatternListDocument {
                        title: "Siteswap Patterns".to_string(),
                        info: None,
                        records,
                        selected: None,
                        dirty: false,
                    }));
                    set_pattern_list_visible.set(true);
                    set_status.set(match stop_reason {
                        Some(GenerationStopReason::PatternLimit(limit)) => {
                            format!("Generator stopped at the {limit} pattern limit")
                        }
                        Some(GenerationStopReason::TimeLimit(seconds)) => {
                            format!("Generator stopped after the {seconds} second time limit")
                        }
                        Some(GenerationStopReason::Cancelled) => {
                            format!("Generator cancelled after {count} patterns")
                        }
                        None => format!("Generated {count} siteswap patterns"),
                    });
                }
                Err(_) if request_controller.signal().aborted() => {
                    set_status.set("Generator cancelled".to_string());
                }
                Err(error) => set_status.set(error),
            }
        });
    };

    let reset_transitioner = move |_| set_transitioner_form.set(TransitionerForm::default());

    let swap_transition_patterns = move |_| {
        set_transitioner_form.update(|form| {
            std::mem::swap(&mut form.from_pattern, &mut form.to_pattern);
        });
    };

    let stop_transitioner = move |_| {
        if let Some(controller) = transitioner_abort.get_untracked() {
            controller.abort();
            set_status.set("Transitioner cancellation requested".to_string());
        }
    };

    let run_transitioner = move |_| {
        if transitioner_running.get_untracked() {
            return;
        }
        let arguments = transitioner_form.get_untracked().arguments();
        let Ok(controller) = AbortController::new() else {
            set_status.set("This browser cannot cancel transitioner requests".to_string());
            return;
        };
        let request_controller = controller.clone();
        set_transitioner_abort.set(Some(controller));
        set_transitioner_running.set(true);
        set_status.set("Finding siteswap transitions...".to_string());
        spawn_local(async move {
            let response = request_transition(arguments, request_controller.clone()).await;
            set_transitioner_running.set(false);
            set_transitioner_abort.set(None);
            match response {
                Ok(GenerationResult {
                    patterns,
                    stop_reason,
                }) => {
                    let count = patterns.len();
                    let records = patterns
                        .into_iter()
                        .map(|pattern| PatternRecord::siteswap(pattern.display, pattern.config))
                        .collect::<Vec<_>>();
                    set_pattern_list_document.set(Some(PatternListDocument {
                        title: "Siteswap Transitions".to_string(),
                        info: None,
                        records,
                        selected: None,
                        dirty: false,
                    }));
                    set_pattern_list_visible.set(true);
                    set_status.set(match stop_reason {
                        Some(GenerationStopReason::PatternLimit(limit)) => {
                            format!("Transitioner stopped at the {limit} pattern limit")
                        }
                        Some(GenerationStopReason::TimeLimit(seconds)) => {
                            format!("Transitioner stopped after the {seconds} second time limit")
                        }
                        Some(GenerationStopReason::Cancelled) => {
                            format!("Transitioner cancelled after {count} transitions")
                        }
                        None => match count {
                            1 => "Found 1 siteswap transition".to_string(),
                            _ => format!("Found {count} siteswap transitions"),
                        },
                    });
                }
                Err(_) if request_controller.signal().aborted() => {
                    set_status.set("Transitioner cancelled".to_string());
                }
                Err(error) => set_status.set(error),
            }
        });
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
                    <div class="share-action">
                        <button type="button" on:click=open_share>"Share"</button>
                        <span
                            class=move || if share_copied.get() { "share-copy-tooltip visible" } else { "share-copy-tooltip" }
                            role="status"
                        >"URL copied"</span>
                    </div>
                    <button type="button" on:click=open_pattern_list>"Pattern List"</button>
                    <button
                        type="button"
                        prop:disabled=move || pattern_list_document.get().is_none()
                        on:click=export_all
                    >"Save List"</button>
                    <button type="button" on:click=open_animation_export>"Export"</button>
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
                    <button type="button" on:click=move |_| set_about_open.set(true)>
                        "About"
                    </button>
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
                                <div class="transitioner-control">
                                    <fieldset prop:disabled=move || transitioner_running.get()>
                                        <div class="transitioner-patterns">
                                            <label for="transition-from">"From pattern"</label>
                                            <input id="transition-from" spellcheck="false" autofocus prop:value=move || transitioner_form.get().from_pattern on:input=move |event| {
                                                let value = event_target_value(&event);
                                                set_transitioner_form.update(|form| form.from_pattern = value.clone());
                                            } />
                                            <span></span>
                                            <button type="button" class="swap-patterns-button" title="Swap patterns" aria-label="Swap patterns" on:click=swap_transition_patterns>"Swap"</button>
                                            <label for="transition-to">"To pattern"</label>
                                            <input id="transition-to" spellcheck="false" prop:value=move || transitioner_form.get().to_pattern on:input=move |event| {
                                                let value = event_target_value(&event);
                                                set_transitioner_form.update(|form| form.to_pattern = value.clone());
                                            } />
                                        </div>
                                        <div class="transitioner-options">
                                            <label class="check-row"><input type="checkbox" prop:checked=move || transitioner_form.get().multiplexing on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_transitioner_form.update(|form| form.multiplexing = checked);
                                            } /><span>"Multiplexing in transitions"</span></label>
                                            <div class=move || if transitioner_form.get().multiplexing { "transitioner-multiplex-options" } else { "transitioner-multiplex-options hidden" }>
                                                <label for="transition-occupancy"><span>"Simultaneous throws"</span><input id="transition-occupancy" type="number" min="1" prop:value=move || transitioner_form.get().simultaneous_throws on:input=move |event| {
                                                    let value = event_target_value(&event);
                                                    set_transitioner_form.update(|form| form.simultaneous_throws = value.clone());
                                                } /></label>
                                                <label class="check-row"><input type="checkbox" prop:checked=move || transitioner_form.get().no_simultaneous_catches on:change=move |event: ev::Event| {
                                                    let checked = event_target::<HtmlInputElement>(&event).checked();
                                                    set_transitioner_form.update(|form| form.no_simultaneous_catches = checked);
                                                } /><span>"No simultaneous catches"</span></label>
                                                <label class="check-row"><input type="checkbox" prop:checked=move || transitioner_form.get().no_clustered_throws on:change=move |event: ev::Event| {
                                                    let checked = event_target::<HtmlInputElement>(&event).checked();
                                                    set_transitioner_form.update(|form| form.no_clustered_throws = checked);
                                                } /><span>"No clustered throws"</span></label>
                                            </div>
                                        </div>
                                    </fieldset>
                                    <div class="button-row transitioner-actions">
                                        <button type="button" prop:disabled=move || transitioner_running.get() on:click=reset_transitioner>"Defaults"</button>
                                        <button type="button" class=move || if transitioner_running.get() { "hidden" } else { "primary" } on:click=run_transitioner>"Run"</button>
                                        <button type="button" class=move || if transitioner_running.get() { "danger-button" } else { "danger-button hidden" } on:click=stop_transitioner>"Stop"</button>
                                    </div>
                                </div>
                            }.into_any(),
                            "generator" => view! {
                                <div class="generator-control">
                                    <fieldset prop:disabled=move || generator_running.get()>
                                        <div class="generator-primary-grid">
                                            <label><span>"Objects"</span><input type="number" min="1" prop:value=move || generator_form.get().balls on:input=move |event| {
                                                let value = event_target_value(&event);
                                                set_generator_form.update(|form| form.balls = value.clone());
                                            } /></label>
                                            <label><span>"Maximum throw"</span><input inputmode="text" prop:value=move || generator_form.get().max_throw on:input=move |event| {
                                                let value = event_target_value(&event);
                                                set_generator_form.update(|form| form.max_throw = value.clone());
                                            } /></label>
                                            <label><span>"Period"</span><input inputmode="text" prop:value=move || generator_form.get().period on:input=move |event| {
                                                let value = event_target_value(&event);
                                                set_generator_form.update(|form| form.period = value.clone());
                                            } /></label>
                                        </div>
                                        <div class="generator-select-grid">
                                            <label><span>"Rhythm"</span><select prop:value=move || if generator_form.get().rhythm_async { "async" } else { "sync" } on:change=move |event| {
                                                let asynchronous = event_target_value(&event) == "async";
                                                set_generator_form.update(|form| form.rhythm_async = asynchronous);
                                            }><option value="async">"Asynchronous"</option><option value="sync">"Synchronous"</option></select></label>
                                            <label><span>"Jugglers"</span><select prop:value=move || generator_form.get().jugglers.to_string() on:change=move |event| {
                                                let value = event_target_value(&event).parse::<usize>().unwrap_or(1).clamp(1, 6);
                                                set_generator_form.update(|form| form.jugglers = value);
                                            }>{(1..=6).map(|value| view! { <option value=value.to_string()>{value}</option> }).collect::<Vec<_>>()}</select></label>
                                            <label><span>"Compositions"</span><select prop:value=move || generator_form.get().composition.to_string() on:change=move |event| {
                                                let value = event_target_value(&event).parse::<usize>().unwrap_or(0).min(2);
                                                set_generator_form.update(|form| form.composition = value);
                                            }><option value="0">"All"</option><option value="1">"Non-obvious"</option><option value="2">"Prime only"</option></select></label>
                                            <label><span>"Multiplexing"</span><select prop:value=move || generator_form.get().multiplexing.to_string() on:change=move |event| {
                                                let value = event_target_value(&event).parse::<usize>().unwrap_or(0).min(3);
                                                set_generator_form.update(|form| form.multiplexing = value);
                                            }><option value="0">"None"</option><option value="1">"2"</option><option value="2">"3"</option><option value="3">"4"</option></select></label>
                                        </div>
                                        <div class="generator-filter-grid">
                                            <label class="check-row"><input type="checkbox" prop:checked=move || generator_form.get().ground_state on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.ground_state = checked);
                                            } /><span>"Ground state patterns"</span></label>
                                            <label class="check-row"><input type="checkbox" prop:checked=move || generator_form.get().excited_state on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.excited_state = checked);
                                            } /><span>"Excited state patterns"</span></label>
                                            <label class="check-row"><input type="checkbox" prop:checked=move || generator_form.get().transition_throws prop:disabled=move || !generator_form.get().excited_state on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.transition_throws = checked);
                                            } /><span>"Transition throws"</span></label>
                                            <label class="check-row"><input type="checkbox" prop:checked=move || generator_form.get().pattern_rotations on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.pattern_rotations = checked);
                                            } /><span>"Pattern rotations"</span></label>
                                            <label class=move || if generator_form.get().jugglers != 1 { "check-row" } else { "check-row hidden" }><input type="checkbox" prop:checked=move || generator_form.get().juggler_permutations prop:disabled=move || !(generator_form.get().ground_state && generator_form.get().excited_state) on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.juggler_permutations = checked);
                                            } /><span>"Juggler permutations"</span></label>
                                            <label class=move || if generator_form.get().jugglers != 1 { "check-row" } else { "check-row hidden" }><input type="checkbox" prop:checked=move || generator_form.get().connected_patterns on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.connected_patterns = checked);
                                            } /><span>"Connected patterns"</span></label>
                                            <label class=move || if generator_form.get().jugglers != 1 { "check-row" } else { "check-row hidden" }><input type="checkbox" prop:checked=move || generator_form.get().symmetric_patterns on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.symmetric_patterns = checked);
                                            } /><span>"Symmetric patterns"</span></label>
                                            <label class=move || if generator_form.get().multiplexing != 0 { "check-row" } else { "check-row hidden" }><input type="checkbox" prop:checked=move || generator_form.get().no_simultaneous_catches on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.no_simultaneous_catches = checked);
                                            } /><span>"No simultaneous catches"</span></label>
                                            <label class=move || if generator_form.get().multiplexing != 0 { "check-row" } else { "check-row hidden" }><input type="checkbox" prop:checked=move || generator_form.get().no_clustered_throws on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.no_clustered_throws = checked);
                                            } /><span>"No clustered throws"</span></label>
                                            <label class=move || if generator_form.get().multiplexing != 0 { "check-row" } else { "check-row hidden" }><input type="checkbox" prop:checked=move || generator_form.get().true_multiplexing on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_generator_form.update(|form| form.true_multiplexing = checked);
                                            } /><span>"True multiplexing"</span></label>
                                        </div>
                                        <div class="generator-expression-grid">
                                            <label for="generator-exclude">"Exclude throws"</label><input id="generator-exclude" spellcheck="false" prop:value=move || generator_form.get().exclude_expressions on:input=move |event| {
                                                let value = event_target_value(&event);
                                                set_generator_form.update(|form| form.exclude_expressions = value.clone());
                                            } />
                                            <label for="generator-include">"Include throws"</label><input id="generator-include" spellcheck="false" prop:value=move || generator_form.get().include_expressions on:input=move |event| {
                                                let value = event_target_value(&event);
                                                set_generator_form.update(|form| form.include_expressions = value.clone());
                                            } />
                                            <label class=move || if generator_form.get().jugglers != 1 && generator_form.get().ground_state && !generator_form.get().excited_state { "" } else { "hidden" } for="generator-delay">"Passing delay"</label><input class=move || if generator_form.get().jugglers != 1 && generator_form.get().ground_state && !generator_form.get().excited_state { "" } else { "hidden" } id="generator-delay" type="number" min="0" prop:value=move || generator_form.get().passing_delay on:input=move |event| {
                                                let value = event_target_value(&event);
                                                set_generator_form.update(|form| form.passing_delay = value.clone());
                                            } />
                                        </div>
                                    </fieldset>
                                    <div class="button-row generator-actions">
                                        <button type="button" prop:disabled=move || generator_running.get() on:click=reset_generator>"Defaults"</button>
                                        <button type="button" class=move || if generator_running.get() { "hidden" } else { "primary" } on:click=run_generator>"Run"</button>
                                        <button type="button" class=move || if generator_running.get() { "danger-button" } else { "danger-button hidden" } on:click=stop_generator>"Stop"</button>
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
                        <button
                            type="button"
                            class=move || tab_class(&view_mode.get(), "selection")
                            on:click=move |_| set_view_mode.set("selection".to_string())
                        >
                            "Selection"
                        </button>
                    </div>

                    <div class=move || match view_mode.get().as_str() {
                        "pattern" => "animation-split with-editor",
                        "edit" => "animation-split with-graph",
                        "selection" => "animation-split with-selection",
                        _ => "animation-split",
                    }>
                        <section class="selection-workspace">
                            <div class="selection-grid">
                                {move || {
                                    selection_records
                                        .get()
                                        .into_iter()
                                        .enumerate()
                                        .map(|(index, _)| {
                                            let class_name = if index == 4 {
                                                "selection-cell current"
                                            } else {
                                                "selection-cell"
                                            };
                                            view! {
                                                <button
                                                    type="button"
                                                    class=class_name
                                                    aria-label=if index == 4 {
                                                        "Current pattern".to_string()
                                                    } else {
                                                        format!("Select mutation {}", index + 1)
                                                    }
                                                    on:click=move |_| choose_selection_variant(index)
                                                >
                                                    <canvas
                                                        id=format!("selection-canvas-{index}")
                                                        on:pointerenter=restore_mouse_pause
                                                        on:pointerleave=apply_mouse_pause
                                                        on:pointerdown=start_selection_canvas_drag
                                                        on:pointermove=drag_canvas_view
                                                        on:pointerup=end_canvas_drag
                                                        on:pointercancel=end_canvas_drag
                                                    ></canvas>
                                                </button>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                            </div>
                            <aside class="mutator-controls">
                                <div class="graph-title">"Mutations"</div>
                                {[
                                    "Event position",
                                    "Event time",
                                    "Pattern timing",
                                    "Add event",
                                    "Remove event",
                                ]
                                .into_iter()
                                .enumerate()
                                .map(|(index, label)| view! {
                                    <label class="check-row">
                                        <input
                                            type="checkbox"
                                            prop:checked=move || mutator_options.get().enabled[index]
                                            on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_mutator_options.update(|options| options.enabled[index] = checked);
                                            }
                                        />
                                        <span>{label}</span>
                                    </label>
                                })
                                .collect::<Vec<_>>()}
                                <label class="mutator-rate">
                                    <span>"Mutation rate"</span>
                                    <input
                                        type="range"
                                        min="0"
                                        max="6"
                                        step="1"
                                        prop:value=move || mutator_options.get().rate_index.to_string()
                                        on:input=move |event| {
                                            let value = event_target_value(&event).parse::<usize>().unwrap_or(3).min(6);
                                            set_mutator_options.update(|options| options.rate_index = value);
                                        }
                                    />
                                    <div class="mutator-rate-labels">
                                        <span>"Low"</span>
                                        <span>"Medium"</span>
                                        <span>"High"</span>
                                    </div>
                                </label>
                            </aside>
                        </section>
                        <div class=move || if view_mode.get() == "selection" { "stage-pane hidden" } else { "stage-pane" }>
                            <canvas
                                id="juggling-stage"
                                class="stage-canvas"
                                tabindex="0"
                                on:pointerenter=restore_mouse_pause
                                on:pointerleave=apply_mouse_pause
                                on:pointerdown=start_canvas_drag
                                on:pointermove=drag_canvas_view
                                on:pointerup=end_canvas_drag
                                on:pointercancel=end_canvas_drag
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
                            <div class="graph-title-row">
                                <div class="graph-title">"Ladder Diagram"</div>
                                <button
                                    type="button"
                                    class=move || {
                                        if ladder_auto_fit.get() {
                                            "ladder-fit-button active"
                                        } else {
                                            "ladder-fit-button"
                                        }
                                    }
                                    aria-pressed=move || ladder_auto_fit.get().to_string()
                                    on:click=move |_| {
                                        set_ladder_auto_fit.set(true);
                                        fit_ladder_to_height(set_ladder_zoom, set_status, true);
                                    }
                                >
                                    "Fit Height"
                                </button>
                            </div>
                            <div
                                id="ladder-scroll"
                                class=move || {
                                    if ladder_zoom.get() > 1.0 {
                                        "ladder-scroll zoomed"
                                    } else {
                                        "ladder-scroll"
                                    }
                                }
                                on:wheel=zoom_ladder_wheel
                            >
                                <div
                                    class="ladder-unavailable"
                                    hidden=move || ladder_unavailable_reason(&current_spec.get()).is_none()
                                >
                                    {move || ladder_unavailable_reason(&current_spec.get()).unwrap_or_default()}
                                </div>
                                <svg
                                    id="ladder-svg"
                                    hidden=move || ladder_unavailable_reason(&current_spec.get()).is_some()
                                    viewBox=move || format!("0 0 100 {:.4}", ladder_view_height(ladder_zoom.get()))
                                    preserveAspectRatio="none"
                                    style=move || format!("--ladder-zoom: {:.4}", ladder_zoom.get())
                                    class=move || {
                                        if ladder_drag.get().is_some() || ladder_pinch_distance.get().is_some() {
                                            "ladder-svg dragging"
                                        } else {
                                            "ladder-svg"
                                        }
                                    }
                                    on:pointerdown=move |event: ev::PointerEvent| {
                                        start_ladder_tracker_drag(event.clone());
                                        register_ladder_touch(event);
                                    }
                                    on:pointermove=preview_ladder_drag
                                    on:pointerup=finish_ladder_drag
                                    on:pointercancel=cancel_ladder_drag
                                >
                                <defs>
                                    <clipPath id="ladder-period-clip">
                                        <rect
                                            x="0"
                                            y=LADDER_TOP_Y
                                            width="100"
                                            height=move || ladder_period_height(ladder_zoom.get())
                                        />
                                    </clipPath>
                                </defs>
                                <rect
                                    x="0"
                                    y="5"
                                    width="100"
                                    height=move || ladder_view_height(ladder_zoom.get()) - 10.0
                                    class="ladder-hotzone"
                                    on:contextmenu=move |event| open_ladder_context(event, String::new())
                                />
                                {move || ladder_tracker_hitbox_view(
                                    &current_spec.get(),
                                    playhead_time.get(),
                                    ladder_zoom.get(),
                                )}
                                {move || ladder_symmetry_views(&current_spec.get(), ladder_zoom.get())}
                                {move || ladder_track_views(&current_spec.get(), ladder_zoom.get())}
                                {move || {
                                    let _ladder_layout_revision = ladder_layout_revision.get();
                                    let spec = current_spec.get();
                                    let Some(diagram) = ladder_diagram(&spec) else {
                                        return Vec::new();
                                    };
                                    let drag = ladder_drag.get();
                                    let zoom = ladder_zoom.get();
                                    let metrics = ladder_view_metrics(&diagram, zoom);
                                    diagram
                                        .edges
                                        .iter()
                                        .map(|edge| {
                                            let edge_id = edge.id.clone();
                                            let context_edge_id = edge.id.clone();
                                            let long_press_edge_id = edge.id.clone();
                                            let status_label = ladder_edge_label(edge);
                                            let selected = selected_ladder.get() == edge_id;
                                            let shapes = ladder_edge_shapes(
                                                &diagram,
                                                edge,
                                                drag.as_ref(),
                                                zoom,
                                                metrics,
                                            );
                                            let start_x = ladder_endpoint_x(
                                                &diagram,
                                                &edge.start,
                                                metrics,
                                            );
                                            let start_y = ladder_absolute_time_y(
                                                &diagram,
                                                ladder_endpoint_preview_time(&edge.start, drag.as_ref()),
                                                zoom,
                                            );
                                            let end_x = ladder_endpoint_x(
                                                &diagram,
                                                &edge.end,
                                                metrics,
                                            );
                                            let end_y = ladder_absolute_time_y(
                                                &diagram,
                                                ladder_endpoint_preview_time(&edge.end, drag.as_ref()),
                                                zoom,
                                            );
                                            let prop_style = ladder_prop_style(
                                                &spec,
                                                edge.path,
                                                playhead_time.get(),
                                            );
                                            let prop = ladder_prop_spec(
                                                &spec,
                                                edge.path,
                                                playhead_time.get(),
                                            );
                                            let start_marker = ladder_prop_marker_view(
                                                &prop,
                                                start_x,
                                                start_y,
                                                metrics.transition_radius * 0.65,
                                                "edge-endpoint",
                                            );
                                            let end_marker = ladder_prop_marker_view(
                                                &prop,
                                                end_x,
                                                end_y,
                                                metrics.transition_radius * 0.65,
                                                "edge-endpoint",
                                            );
                                            view! {
                                                <g
                                                    class=if selected { "ladder-item selected" } else { "ladder-item" }
                                                    style=prop_style
                                                    clip-path="url(#ladder-period-clip)"
                                                    on:pointerdown=move |event: ev::PointerEvent| {
                                                        let drag = ladder_drag.get_untracked();
                                                        let nearest_id = nearest_ladder_edge_at_client(
                                                            &current_spec.get_untracked(),
                                                            drag.as_ref(),
                                                            ladder_zoom.get_untracked(),
                                                            event.client_x() as f64,
                                                            event.client_y() as f64,
                                                        )
                                                        .map(|(id, _)| id)
                                                        .unwrap_or_else(|| long_press_edge_id.clone());
                                                        if event.button() == 0 && (event.ctrl_key() || event.meta_key()) {
                                                            event.prevent_default();
                                                            event.stop_propagation();
                                                            show_ladder_context(
                                                                nearest_id,
                                                                event.client_x() as f64,
                                                                event.client_y() as f64,
                                                            );
                                                            return;
                                                        }
                                                        if event.button() == 0 {
                                                            schedule_ladder_long_press(
                                                                event.pointer_id(),
                                                                event.pointer_type() == "touch",
                                                                nearest_id,
                                                                event.client_x() as f64,
                                                                event.client_y() as f64,
                                                            );
                                                        }
                                                    }
                                                    on:click=move |event: ev::MouseEvent| {
                                                        let drag = ladder_drag.get_untracked();
                                                        let (nearest_id, nearest_label) = nearest_ladder_edge_at_client(
                                                            &current_spec.get_untracked(),
                                                            drag.as_ref(),
                                                            ladder_zoom.get_untracked(),
                                                            event.client_x() as f64,
                                                            event.client_y() as f64,
                                                        )
                                                        .unwrap_or_else(|| (edge_id.clone(), status_label.clone()));
                                                        set_selected_ladder.set(nearest_id);
                                                        set_status.set(format!("Selected timing: {nearest_label}"));
                                                    }
                                                    on:contextmenu=move |event| {
                                                        let drag = ladder_drag.get_untracked();
                                                        let nearest_id = nearest_ladder_edge_at_client(
                                                            &current_spec.get_untracked(),
                                                            drag.as_ref(),
                                                            ladder_zoom.get_untracked(),
                                                            event.client_x() as f64,
                                                            event.client_y() as f64,
                                                        )
                                                        .map(|(id, _)| id)
                                                        .unwrap_or_else(|| context_edge_id.clone());
                                                        open_ladder_context(event, nearest_id);
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
                                                    {start_marker}
                                                    {end_marker}
                                                </g>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                                {move || {
                                    let _ladder_layout_revision = ladder_layout_revision.get();
                                    let spec = current_spec.get();
                                    let Some(diagram) = ladder_diagram(&spec) else {
                                        return Vec::new();
                                    };
                                    let zoom = ladder_zoom.get();
                                    let metrics = ladder_view_metrics(&diagram, zoom);
                                    diagram
                                        .transitions
                                        .clone()
                                        .into_iter()
                                        .map(|transition| {
                                            let transition_id = transition.id.clone();
                                            let context_transition_id = transition.id.clone();
                                            let long_press_transition_id = transition.id.clone();
                                            let status_label = ladder_transition_label(&transition);
                                            let selected = selected_ladder.get() == transition_id;
                                            let x = ladder_transition_x(
                                                &diagram,
                                                &transition,
                                                metrics,
                                            );
                                            let y = ladder_time_y(
                                                &diagram,
                                                ladder_transition_preview_time(
                                                    &transition,
                                                    ladder_drag.get().as_ref(),
                                                ),
                                                zoom,
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
                                            let marker = ladder_prop_marker_view(
                                                &ladder_prop_spec(
                                                    &spec,
                                                    transition.path,
                                                    playhead_time.get(),
                                                ),
                                                x,
                                                y,
                                                metrics.transition_radius,
                                                "ladder-prop-marker",
                                            );
                                            view! {
                                                <g
                                                    class=class_name
                                                    style=prop_style
                                                    on:pointerdown=move |event: ev::PointerEvent| {
                                                        if event.button() == 0 && (event.ctrl_key() || event.meta_key()) {
                                                            event.prevent_default();
                                                            event.stop_propagation();
                                                            show_ladder_context(
                                                                long_press_transition_id.clone(),
                                                                event.client_x() as f64,
                                                                event.client_y() as f64,
                                                            );
                                                            return;
                                                        }
                                                        if event.button() == 0 {
                                                            schedule_ladder_long_press(
                                                                event.pointer_id(),
                                                                event.pointer_type() == "touch",
                                                                long_press_transition_id.clone(),
                                                                event.client_x() as f64,
                                                                event.client_y() as f64,
                                                            );
                                                        }
                                                    }
                                                    on:click=move |_| {
                                                        set_selected_ladder.set(transition_id.clone());
                                                        set_status.set(format!("Selected transition: {status_label}"));
                                                    }
                                                    on:contextmenu=move |event| {
                                                        open_ladder_context(event, context_transition_id.clone());
                                                    }
                                                >
                                                    <circle
                                                        class="ladder-node-hitbox"
                                                        cx=x
                                                        cy=y
                                                        r=metrics.transition_radius
                                                    />
                                                    {marker}
                                                </g>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                                {move || {
                                    let _ladder_layout_revision = ladder_layout_revision.get();
                                    let Some(diagram) = ladder_diagram(&current_spec.get()) else {
                                        return Vec::new();
                                    };
                                    let zoom = ladder_zoom.get();
                                    let metrics = ladder_view_metrics(&diagram, zoom);
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
                                            let y = ladder_time_y(
                                                &diagram,
                                                preview_time,
                                                zoom,
                                            );
                                            let radius = metrics.position_radius;
                                            let side = 2.0 * radius;
                                            let top_left_x = x - radius;
                                            let top_left_y = y - radius;
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
                                                        if pointer_event.ctrl_key() || pointer_event.meta_key() {
                                                            pointer_event.prevent_default();
                                                            pointer_event.stop_propagation();
                                                            show_ladder_context(
                                                                drag_position_id.clone(),
                                                                pointer_event.client_x() as f64,
                                                                pointer_event.client_y() as f64,
                                                            );
                                                            return;
                                                        }
                                                        if pointer_event.pointer_type() == "touch"
                                                            && !ladder_touches.with_untracked(Vec::is_empty)
                                                        {
                                                            return;
                                                        }
                                                        pointer_event.prevent_default();
                                                        capture_ladder_pointer(pointer_event.pointer_id());
                                                        set_ladder_preview_spec.set(None);
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
                                                        schedule_ladder_long_press(
                                                            pointer_event.pointer_id(),
                                                            pointer_event.pointer_type() == "touch",
                                                            drag_position_id.clone(),
                                                            pointer_event.client_x() as f64,
                                                            pointer_event.client_y() as f64,
                                                        );
                                                        set_status.set(format!("Dragging position: {drag_status_label}"));
                                                    }
                                                    on:contextmenu=move |event| {
                                                        open_ladder_context(event, context_position_id.clone());
                                                    }
                                                >
                                                    <rect
                                                        class="ladder-node-hitbox"
                                                        x=x - radius
                                                        y=y - radius
                                                        width=side
                                                        height=side
                                                    />
                                                    <rect x=top_left_x y=top_left_y width=side height=side />
                                                </g>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                                {move || {
                                    let _ladder_layout_revision = ladder_layout_revision.get();
                                    let Some(diagram) = ladder_diagram(&current_spec.get()) else {
                                        return Vec::new();
                                    };
                                    let zoom = ladder_zoom.get();
                                    let metrics = ladder_view_metrics(&diagram, zoom);
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
                                            let y = ladder_time_y(
                                                &diagram,
                                                preview_time,
                                                zoom,
                                            );
                                            let radius = metrics.transition_radius;
                                            let cross_radius = radius * 0.78;
                                            let x_left = x - cross_radius;
                                            let x_right = x + cross_radius;
                                            let y_top = y - cross_radius;
                                            let y_bottom = y + cross_radius;
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
                                                        if pointer_event.ctrl_key() || pointer_event.meta_key() {
                                                            pointer_event.prevent_default();
                                                            pointer_event.stop_propagation();
                                                            show_ladder_context(
                                                                drag_event_id.clone(),
                                                                pointer_event.client_x() as f64,
                                                                pointer_event.client_y() as f64,
                                                            );
                                                            return;
                                                        }
                                                        if pointer_event.pointer_type() == "touch"
                                                            && !ladder_touches.with_untracked(Vec::is_empty)
                                                        {
                                                            return;
                                                        }
                                                        pointer_event.prevent_default();
                                                        capture_ladder_pointer(pointer_event.pointer_id());
                                                        set_ladder_preview_spec.set(None);
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
                                                        schedule_ladder_long_press(
                                                            pointer_event.pointer_id(),
                                                            pointer_event.pointer_type() == "touch",
                                                            drag_event_id.clone(),
                                                            pointer_event.client_x() as f64,
                                                            pointer_event.client_y() as f64,
                                                        );
                                                        set_status.set(format!("Dragging event: {drag_status_label}"));
                                                    }
                                                    on:contextmenu=move |context_event| {
                                                        open_ladder_context(context_event, context_event_id.clone());
                                                    }
                                                >
                                                    <circle
                                                        class="ladder-node-hitbox"
                                                        cx=x
                                                        cy=y
                                                        r=radius
                                                    />
                                                    <circle cx=x cy=y r=radius />
                                                    <line x1=x_left y1=y x2=x_right y2=y />
                                                    <line x1=x y1=y_top x2=x y2=y_bottom />
                                                </g>
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                }}
                                {move || ladder_tracker_view(
                                    &current_spec.get(),
                                    playhead_time.get(),
                                    ladder_zoom.get(),
                                    !playing.get(),
                                )}
                                </svg>
                            </div>
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
                        <label class="speed-control"><span>"Speed"</span><input type="range" min="0.25" max="2.25" step="0.05" prop:value=move || speed.get().to_string() on:input=move |ev| {
                            let value = event_target_value(&ev).parse::<f64>().unwrap_or(0.5).max(0.01);
                            set_speed.set(value);
                            set_animation_prefs.update(|prefs| prefs.slowdown = 1.0 / value);
                        } /></label>
                        <div class="animation-command-group">
                            <button type="button" on:click=reset_view>"Reset View"</button>
                            <button type="button" on:click=open_animation_prefs>"Preferences"</button>
                            <button
                                type="button"
                                on:click=move |_| set_pattern_transform_open.set(true)
                            >"Transform"</button>
                            <button
                                type="button"
                                prop:disabled=move || !prop_colors_available.get()
                                on:click=move |_| set_prop_colors_open.set(true)
                            >"Color Props"</button>
                        </div>
                        <div class="animation-options">
                            <label class="check-row"><input type="checkbox" prop:checked=move || show_trails.get() on:change=move |ev: ev::Event| set_show_trails.set(event_target::<HtmlInputElement>(&ev).checked()) /> <span>"Trails"</span></label>
                            <label class="check-row"><input type="checkbox" prop:checked=move || show_grid.get() on:change=move |ev: ev::Event| {
                                let checked = event_target::<HtmlInputElement>(&ev).checked();
                                set_show_grid.set(checked);
                                set_animation_prefs.update(|prefs| prefs.show_ground = if checked { ShowGround::On } else { ShowGround::Off });
                            } /> <span>"Ground"</span></label>
                            <label class="check-row"><input type="checkbox" prop:checked=move || stereo.get() on:change=move |ev: ev::Event| {
                                let checked = event_target::<HtmlInputElement>(&ev).checked();
                                set_stereo.set(checked);
                                set_animation_prefs.update(|prefs| prefs.stereo = checked);
                            } /> <span>"Stereo"</span></label>
                            <label class="check-row"><input type="checkbox" prop:checked=move || catch_sound.get() on:change=move |ev: ev::Event| {
                                let checked = event_target::<HtmlInputElement>(&ev).checked();
                                if checked {
                                    crate::audio::prepare_catch();
                                }
                                set_catch_sound.set(checked);
                                set_animation_prefs.update(|prefs| prefs.catch_sound = checked);
                            } /> <span>"Catch sound"</span></label>
                            <label class="check-row"><input type="checkbox" prop:checked=move || bounce_sound.get() on:change=move |ev: ev::Event| {
                                let checked = event_target::<HtmlInputElement>(&ev).checked();
                                if checked {
                                    crate::audio::prepare_bounce();
                                }
                                set_bounce_sound.set(checked);
                                set_animation_prefs.update(|prefs| prefs.bounce_sound = checked);
                            } /> <span>"Bounce sound"</span></label>
                        </div>
                    </div>
                </section>
            </section>
            {move || {
                if !about_open.get() {
                    return view! {}.into_any();
                }
                view! {
                    <div
                        class="dialog-backdrop about-backdrop"
                        on:click=move |_| set_about_open.set(false)
                    >
                        <section
                            class="dialog-panel about-dialog"
                            role="dialog"
                            aria-modal="true"
                            aria-labelledby="about-dialog-title"
                            on:click=move |event| event.stop_propagation()
                        >
                            <div class="dialog-title" id="about-dialog-title">"About Juggling Lab"</div>
                            <div class="about-content">
                                <div class="about-copy">
                                    <h2>"Juggling Lab"</h2>
                                    <p>
                                        {format!(
                                            "Copyright {} 2002-2026 Jack Boyce and the Juggling Lab contributors",
                                            '\u{00a9}'
                                        )}
                                    </p>
                                    <p class="about-derivative">
                                        "This web application is an unofficial derivative work based on Juggling Lab and adapted for the web under the GNU General Public License v2."
                                    </p>
                                    <p class="about-source">
                                        <span>"Source: "</span>
                                        <a
                                            href="https://github.com/paolobettelini/jugglinglab/"
                                            target="_blank"
                                            rel="noopener noreferrer"
                                        >
                                            "paolobettelini/jugglinglab"
                                        </a>
                                    </p>
                                    <p class="about-platform">"Web browser edition"</p>
                                </div>
                            </div>
                            <div class="dialog-actions">
                                <button type="button" on:click=move |_| set_about_open.set(false)>
                                    "OK"
                                </button>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
            {move || {
                let Some(dialog) = animation_export_dialog.get() else {
                    return view! {}.into_any();
                };
                let running = animation_export_running.get();
                let (completed, total) = animation_export_progress.get();
                let error_class = if dialog.error.is_some() {
                    "dialog-error"
                } else {
                    "dialog-error hidden"
                };
                let progress_class = if running {
                    "export-progress"
                } else {
                    "export-progress hidden"
                };
                view! {
                    <div
                        class="dialog-backdrop animation-export-backdrop"
                        on:click=move |_| {
                            if !animation_export_running.get_untracked() {
                                set_animation_export_dialog.set(None);
                            }
                        }
                    >
                        <section
                            class="dialog-panel animation-export-dialog"
                            on:click=move |event| event.stop_propagation()
                        >
                            <div class="dialog-title">"Export Animation"</div>
                            <div class="animation-export-form">
                                <div class="export-number-grid">
                                    <label for="export-format">"Format"</label>
                                    <select
                                        id="export-format"
                                        prop:disabled=running
                                        prop:value=match dialog.format {
                                            AnimationExportFormat::Gif => "gif",
                                            AnimationExportFormat::WebM => "webm",
                                            AnimationExportFormat::Mp4 => "mp4",
                                        }
                                        on:change=move |event| {
                                            let value = event_target_value(&event);
                                            set_animation_export_dialog.update(|dialog| if let Some(dialog) = dialog {
                                                dialog.format = match value.as_str() {
                                                    "webm" => AnimationExportFormat::WebM,
                                                    "mp4" => AnimationExportFormat::Mp4,
                                                    _ => AnimationExportFormat::Gif,
                                                };
                                                dialog.error = None;
                                            });
                                        }
                                    >
                                        <option value="gif">"GIF"</option>
                                        <option value="webm" disabled=!dialog.webm_supported>
                                            {if dialog.webm_supported { "WebM" } else { "WebM (unavailable in this browser)" }}
                                        </option>
                                        <option value="mp4" disabled=!dialog.mp4_supported>
                                            {if dialog.mp4_supported { "MP4" } else { "MP4 (unavailable in this browser)" }}
                                        </option>
                                    </select>
                                    <label for="export-width">"Width"</label>
                                    <input
                                        id="export-width"
                                        type="number"
                                        min="64"
                                        max="4096"
                                        prop:disabled=running
                                        prop:value=dialog.width
                                        on:input=move |event| {
                                            let value = event_target_value(&event);
                                            set_animation_export_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.width = value.clone(); dialog.error = None; });
                                        }
                                    />
                                    <label for="export-height">"Height"</label>
                                    <input
                                        id="export-height"
                                        type="number"
                                        min="64"
                                        max="4096"
                                        prop:disabled=running
                                        prop:value=dialog.height
                                        on:input=move |event| {
                                            let value = event_target_value(&event);
                                            set_animation_export_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.height = value.clone(); dialog.error = None; });
                                        }
                                    />
                                    <label for="export-fps">"Frames per second"</label>
                                    <input
                                        id="export-fps"
                                        type="number"
                                        min="1"
                                        max="60"
                                        step="0.1"
                                        prop:disabled=running
                                        prop:value=dialog.fps
                                        on:input=move |event| {
                                            let value = event_target_value(&event);
                                            set_animation_export_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.fps = value.clone(); dialog.error = None; });
                                        }
                                    />
                                </div>
                                <div class="export-check-grid">
                                    <label class="check-row">
                                        <input
                                            type="checkbox"
                                            prop:disabled=running
                                            prop:checked=dialog.antialiasing
                                            on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_animation_export_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.antialiasing = checked; dialog.error = None; });
                                            }
                                        />
                                        <span>"Antialiasing"</span>
                                    </label>
                                    <label class="check-row">
                                        <input
                                            type="checkbox"
                                            prop:disabled=running
                                            prop:checked=dialog.show_title
                                            on:change=move |event: ev::Event| {
                                                let checked = event_target::<HtmlInputElement>(&event).checked();
                                                set_animation_export_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.show_title = checked; dialog.error = None; });
                                            }
                                        />
                                        <span>"Title overlay"</span>
                                    </label>
                                </div>
                                <div class=progress_class>
                                    <progress max=total.max(1) value=completed></progress>
                                    <span>{format!("{completed} / {total}")}</span>
                                </div>
                                <div class=error_class>{dialog.error.unwrap_or_default()}</div>
                            </div>
                            <div class="dialog-actions">
                                <button type="button" on:click=cancel_animation_export>
                                    {if running { "Cancel Export" } else { "Cancel" }}
                                </button>
                                <button
                                    type="button"
                                    class="primary"
                                    prop:disabled=running
                                    on:click=start_animation_export
                                >"Export"</button>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
            {move || {
                if !pattern_transform_open.get() {
                    return view! {}.into_any();
                }
                view! {
                    <div
                        class="dialog-backdrop pattern-transform-backdrop"
                        on:click=move |_| set_pattern_transform_open.set(false)
                    >
                        <section
                            class="dialog-panel pattern-transform-dialog"
                            on:click=move |event| event.stop_propagation()
                        >
                            <div class="dialog-title">"Transform Pattern"</div>
                            <div class="pattern-transform-actions">
                                <button
                                    type="button"
                                    on:click=move |_| apply_pattern_transform(PatternTransform::Optimize)
                                >"Optimize for Throwing Error"</button>
                                <button
                                    type="button"
                                    on:click=move |_| apply_pattern_transform(PatternTransform::SwapHands)
                                >"Swap Hands"</button>
                                <button
                                    type="button"
                                    on:click=move |_| apply_pattern_transform(PatternTransform::FlipX)
                                >"Flip Pattern in X"</button>
                                <button
                                    type="button"
                                    on:click=move |_| apply_pattern_transform(PatternTransform::FlipTime)
                                >"Flip Pattern in Time"</button>
                            </div>
                            <div class="dialog-actions">
                                <button
                                    type="button"
                                    on:click=move |_| set_pattern_transform_open.set(false)
                                >"Cancel"</button>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
            {move || {
                if !prop_colors_open.get() {
                    return view! {}.into_any();
                }
                view! {
                    <div
                        class="dialog-backdrop prop-colors-backdrop"
                        on:click=move |_| set_prop_colors_open.set(false)
                    >
                        <section
                            class="dialog-panel prop-colors-dialog"
                            on:click=move |event| event.stop_propagation()
                        >
                            <div class="dialog-title">"Color Props"</div>
                            <div class="prop-color-presets">
                                <button type="button" on:click=move |_| apply_prop_colors("mixed".to_string())>
                                    <span class="mixed-color-swatch" aria-hidden="true">
                                        <i></i><i></i><i></i><i></i>
                                    </span>
                                    <span>"Mixed"</span>
                                </button>
                                <button type="button" on:click=move |_| apply_prop_colors("orbits".to_string())>
                                    <span class="orbit-color-swatch" aria-hidden="true">
                                        <i></i><i></i><i></i>
                                    </span>
                                    <span>"By orbit"</span>
                                </button>
                            </div>
                            <div class="prop-color-grid">
                                {PROP_COLOR_CHOICES.into_iter().map(|(name, label, css)| {
                                    let command = name.to_string();
                                    let swatch_class = if name == "transparent" {
                                        "prop-color-swatch transparent"
                                    } else {
                                        "prop-color-swatch"
                                    };
                                    view! {
                                        <button type="button" on:click=move |_| apply_prop_colors(command.clone())>
                                            <span
                                                class=swatch_class
                                                style=format!("--prop-choice-color: {css}")
                                                aria-hidden="true"
                                            ></span>
                                            <span>{label}</span>
                                        </button>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                            <div class="dialog-actions">
                                <button type="button" on:click=move |_| set_prop_colors_open.set(false)>"Cancel"</button>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
            {move || {
                let Some(dialog) = animation_prefs_dialog.get() else {
                    return view! {}.into_any();
                };
                let error_class = if dialog.error.is_some() {
                    "dialog-error"
                } else {
                    "dialog-error hidden"
                };
                view! {
                    <div
                        class="dialog-backdrop animation-prefs-backdrop"
                        on:click=move |_| set_animation_prefs_dialog.set(None)
                    >
                        <section
                            class="dialog-panel animation-prefs-dialog"
                            on:click=move |event| event.stop_propagation()
                        >
                            <div class="dialog-title">"Animation Preferences"</div>
                            <div class="prefs-form">
                                <div class="prefs-number-grid">
                                    <label for="prefs-width">"Width"</label>
                                    <input id="prefs-width" type="number" min="0" prop:value=dialog.width on:input=move |event| {
                                        let value = event_target_value(&event);
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.width = value.clone(); dialog.error = None; });
                                    } />
                                    <label for="prefs-height">"Height"</label>
                                    <input id="prefs-height" type="number" min="0" prop:value=dialog.height on:input=move |event| {
                                        let value = event_target_value(&event);
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.height = value.clone(); dialog.error = None; });
                                    } />
                                    <label for="prefs-fps">"Frames per second"</label>
                                    <input id="prefs-fps" type="number" min="0.01" step="0.1" prop:value=dialog.fps on:input=move |event| {
                                        let value = event_target_value(&event);
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.fps = value.clone(); dialog.error = None; });
                                    } />
                                    <label for="prefs-slowdown">"Slowdown factor"</label>
                                    <input id="prefs-slowdown" type="number" min="0.01" step="0.1" prop:value=dialog.slowdown on:input=move |event| {
                                        let value = event_target_value(&event);
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.slowdown = value.clone(); dialog.error = None; });
                                    } />
                                    <label for="prefs-border">"Border (pixels)"</label>
                                    <input id="prefs-border" type="number" min="0" prop:value=dialog.border on:input=move |event| {
                                        let value = event_target_value(&event);
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.border = value.clone(); dialog.error = None; });
                                    } />
                                    <label for="prefs-ground">"Show ground"</label>
                                    <select id="prefs-ground" prop:value=dialog.show_ground.as_index().to_string() on:change=move |event| {
                                        let value = event_target_value(&event).parse::<usize>().unwrap_or(0);
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog {
                                            dialog.show_ground = ShowGround::from_index(value).unwrap_or(ShowGround::Auto);
                                            dialog.error = None;
                                        });
                                    }>
                                        <option value="0">"Auto"</option>
                                        <option value="1">"Yes"</option>
                                        <option value="2">"No"</option>
                                    </select>
                                </div>

                                <div class="prefs-check-grid">
                                    <label class="check-row"><input type="checkbox" prop:checked=dialog.start_paused on:change=move |event: ev::Event| {
                                        let checked = event_target::<HtmlInputElement>(&event).checked();
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.start_paused = checked; dialog.error = None; });
                                    } /><span>"Start paused"</span></label>
                                    <label class="check-row"><input type="checkbox" prop:checked=dialog.mouse_pause on:change=move |event: ev::Event| {
                                        let checked = event_target::<HtmlInputElement>(&event).checked();
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.mouse_pause = checked; dialog.error = None; });
                                    } /><span>"Pause when mouse is away"</span></label>
                                    <label class="check-row"><input type="checkbox" prop:checked=dialog.stereo on:change=move |event: ev::Event| {
                                        let checked = event_target::<HtmlInputElement>(&event).checked();
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.stereo = checked; dialog.error = None; });
                                    } /><span>"Stereo display"</span></label>
                                    <label class="check-row"><input type="checkbox" prop:checked=dialog.catch_sound on:change=move |event: ev::Event| {
                                        let checked = event_target::<HtmlInputElement>(&event).checked();
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.catch_sound = checked; dialog.error = None; });
                                    } /><span>"Catch sounds"</span></label>
                                    <label class="check-row"><input type="checkbox" prop:checked=dialog.bounce_sound on:change=move |event: ev::Event| {
                                        let checked = event_target::<HtmlInputElement>(&event).checked();
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.bounce_sound = checked; dialog.error = None; });
                                    } /><span>"Bounce sounds"</span></label>
                                </div>

                                <label class="prefs-manual" for="prefs-manual">
                                    <span>"Manual settings"</span>
                                    <input id="prefs-manual" spellcheck="false" prop:value=dialog.manual_settings on:input=move |event| {
                                        let value = event_target_value(&event);
                                        set_animation_prefs_dialog.update(|dialog| if let Some(dialog) = dialog { dialog.manual_settings = value.clone(); dialog.error = None; });
                                    } />
                                </label>
                                <div class=error_class role="alert">{dialog.error.unwrap_or_default()}</div>
                            </div>
                            <div class="dialog-actions prefs-actions">
                                <button type="button" on:click=reset_animation_prefs>"Defaults"</button>
                                <span></span>
                                <button type="button" on:click=move |_| set_animation_prefs_dialog.set(None)>"Cancel"</button>
                                <button type="button" class="primary" on:click=confirm_animation_prefs>"OK"</button>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
            {move || {
                if !pattern_list_visible.get() {
                    return view! {}.into_any();
                }
                let Some(document) = pattern_list_document.get() else {
                    return view! {}.into_any();
                };
                let selected_line = document.selected;
                let line_count = document.records.len();
                let title_class = if document.dirty {
                    "pattern-list-document-title dirty"
                } else {
                    "pattern-list-document-title"
                };
                view! {
                    <div
                        class="dialog-backdrop pattern-list-backdrop"
                        on:click=move |_| set_pattern_list_visible.set(false)
                    >
                        <section
                            class="pattern-list-window"
                            on:click=move |event| event.stop_propagation()
                        >
                            <header class="pattern-list-header">
                                <div class=title_class>
                                    <input
                                        aria-label="Pattern list title"
                                        prop:value=document.title
                                        on:input=move |event| {
                                            let value = event_target_value(&event);
                                            set_pattern_list_document.update(|document| {
                                                if let Some(document) = document {
                                                    document.title = value.clone();
                                                    document.dirty = true;
                                                }
                                            });
                                        }
                                    />
                                </div>
                                <span class="pattern-list-count">{format!("{line_count} lines")}</span>
                                <button type="button" on:click=move |_| set_pattern_list_visible.set(false)>
                                    "Close"
                                </button>
                            </header>
                            <div class="pattern-list-toolbar">
                                <button type="button" on:click=new_pattern_list>"New List"</button>
                                <button type="button" on:click=insert_current_in_pattern_list>
                                    "Insert Current"
                                </button>
                                <button type="button" on:click=open_insert_pattern_list_text>
                                    "Insert Text"
                                </button>
                                <button
                                    type="button"
                                    prop:disabled=selected_line.is_none()
                                    on:click=remove_pattern_list_line
                                >"Remove"</button>
                                <span class="pattern-list-toolbar-spacer"></span>
                                <button type="button" on:click=export_pattern_list_text>"Save Text"</button>
                                <button type="button" class="primary" on:click=export_all>"Save JML"</button>
                            </div>
                            <div class="pattern-list-table-header" aria-hidden="true">
                                <span></span>
                                <span>"Display"</span>
                                <span>"Notation"</span>
                                <span>"Actions"</span>
                            </div>
                            <div class="pattern-list-rows">
                                {document.records
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, record)| {
                                        let selected = selected_line == Some(index);
                                        let playable = record.is_playable();
                                        let notation = record
                                            .notation
                                            .clone()
                                            .unwrap_or_else(|| "Text".to_string());
                                        let open_record = record.clone();
                                        let display = record.display.clone();
                                        view! {
                                            <div
                                                class=if selected { "pattern-list-row selected" } else { "pattern-list-row" }
                                                draggable="true"
                                                on:click=move |_| {
                                                    set_pattern_list_document.update(|document| {
                                                        if let Some(document) = document {
                                                            document.selected = Some(index);
                                                        }
                                                    });
                                                }
                                                on:dragstart=move |_| set_pattern_list_drag_index.set(Some(index))
                                                on:dragend=move |_| set_pattern_list_drag_index.set(None)
                                                on:dragover=move |event| event.prevent_default()
                                                on:drop=move |event| {
                                                    event.prevent_default();
                                                    let Some(from) = pattern_list_drag_index.get_untracked() else {
                                                        return;
                                                    };
                                                    set_pattern_list_document.update(|document| {
                                                        if let Some(document) = document {
                                                            move_pattern_list_record(document, from, index);
                                                        }
                                                    });
                                                    set_pattern_list_drag_index.set(None);
                                                }
                                            >
                                                <span class="pattern-list-drag-handle" title="Drag to reorder"></span>
                                                <button
                                                    type="button"
                                                    class="pattern-list-open"
                                                    prop:disabled=!playable
                                                    on:click=move |event| {
                                                        event.stop_propagation();
                                                        activate_pattern_list_record(open_record.clone());
                                                    }
                                                >{display.clone()}</button>
                                                <span class="pattern-list-notation">{notation}</span>
                                                <div class="pattern-list-row-actions">
                                                    <button
                                                        type="button"
                                                        on:click=move |event| {
                                                            event.stop_propagation();
                                                            set_pattern_list_dialog_text.set(display.clone());
                                                            set_pattern_list_dialog.set(Some(
                                                                PatternListDialogAction::EditDisplay { index },
                                                            ));
                                                        }
                                                    >"Edit"</button>
                                                    <button
                                                        type="button"
                                                        class="danger-button"
                                                        on:click=move |event| {
                                                            event.stop_propagation();
                                                            set_pattern_list_document.update(|document| {
                                                                if let Some(document) = document {
                                                                    remove_pattern_list_record(document, index);
                                                                }
                                                            });
                                                            set_status.set("Pattern list line removed".to_string());
                                                        }
                                                    >"Remove"</button>
                                                </div>
                                            </div>
                                        }
                                    })
                                    .collect::<Vec<_>>()}
                                <div
                                    class="pattern-list-drop-end"
                                    on:dragover=move |event| event.prevent_default()
                                    on:drop=move |event| {
                                        event.prevent_default();
                                        let Some(from) = pattern_list_drag_index.get_untracked() else {
                                            return;
                                        };
                                        set_pattern_list_document.update(|document| {
                                            if let Some(document) = document {
                                                let end = document.records.len();
                                                move_pattern_list_record(document, from, end);
                                            }
                                        });
                                        set_pattern_list_drag_index.set(None);
                                    }
                                ></div>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
            {move || {
                let Some(action) = pattern_list_dialog.get() else {
                    return view! {}.into_any();
                };
                let title = match action {
                    PatternListDialogAction::InsertText { .. } => "Insert Text",
                    PatternListDialogAction::EditDisplay { .. } => "Change Display Text",
                };
                view! {
                    <div class="dialog-backdrop pattern-list-text-backdrop">
                        <section class="dialog-panel">
                            <div class="dialog-title">{title}</div>
                            <div class="dialog-grid single-field-dialog">
                                <label for="pattern-list-text">"Text"</label>
                                <input
                                    id="pattern-list-text"
                                    autofocus
                                    prop:value=move || pattern_list_dialog_text.get()
                                    on:input=move |event| set_pattern_list_dialog_text.set(event_target_value(&event))
                                />
                            </div>
                            <div class="dialog-actions">
                                <button type="button" on:click=move |_| set_pattern_list_dialog.set(None)>
                                    "Cancel"
                                </button>
                                <button type="button" class="primary" on:click=apply_pattern_list_dialog>
                                    "OK"
                                </button>
                            </div>
                        </section>
                    </div>
                }
                .into_any()
            }}
            {move || {
                let Some(menu) = ladder_context_menu.get() else {
                    return view! {}.into_any();
                };
                let spec = current_spec.get();
                let selected_id = selected_ladder.get();
                let can_add = selected_ladder_can_add_at_context(&spec, &selected_id);
                let can_remove_event = selected_ladder_can_remove_event(&spec, &selected_id);
                let can_remove_position = selected_ladder_can_remove_position(&spec, &selected_id);
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
                                class="context-action"
                                prop:disabled=!can_add
                                on:click=move |_| {
                                    add_ladder_event_from_target(1);
                                    finish_ladder_popup();
                                }
                            >"Add Left Event"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_add
                                on:click=move |_| {
                                    add_ladder_event_from_target(0);
                                    finish_ladder_popup();
                                }
                            >"Add Right Event"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_remove_event
                                on:click=move |event| {
                                    remove_selected_ladder_item(event);
                                    finish_ladder_popup();
                                }
                            >"Remove Event"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_add
                                on:click=move |event| {
                                    add_ladder_position_from_target(event);
                                    finish_ladder_popup();
                                }
                            >"Add Position"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_remove_position
                                on:click=move |event| {
                                    remove_selected_ladder_item(event);
                                    finish_ladder_popup();
                                }
                            >"Remove Position"</button>
                            <div class="ladder-context-divider"></div>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_define_prop
                                on:click=move |event| {
                                    set_ladder_context_menu.set(None);
                                    open_define_prop_dialog(event);
                                }
                            >"Define Prop"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_define_throw
                                on:click=move |event| {
                                    set_ladder_context_menu.set(None);
                                    open_define_throw_dialog(event);
                                }
                            >"Define Throw"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_catch
                                on:click=move |_| {
                                    change_selected_ladder_catch(MhnJmlTransitionType::Catch);
                                    finish_ladder_popup();
                                }
                            >"Change to Normal Catch"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_soft
                                on:click=move |_| {
                                    change_selected_ladder_catch(MhnJmlTransitionType::SoftCatch);
                                    finish_ladder_popup();
                                }
                            >"Change to Soft Catch"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_grab
                                on:click=move |_| {
                                    change_selected_ladder_catch(MhnJmlTransitionType::GrabCatch);
                                    finish_ladder_popup();
                                }
                            >"Change to Grab Catch"</button>
                            <button
                                type="button"
                                class="context-action"
                                prop:disabled=!can_make_last
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
                                                let defaults = PropSpec::default_for_type(&value);
                                                dialog.prop_type = value;
                                                dialog.color = prop_color_input_value(defaults.color.as_deref());
                                                dialog.diameter = defaults.diameter;
                                                dialog.inside_diameter = defaults.inside_diameter.unwrap_or(20.0);
                                                dialog.image_source = defaults.image_source.unwrap_or_else(|| "ball.png".to_string());
                                                dialog.image_width = defaults.diameter;
                                            }
                                        });
                                    }
                                >
                                    <option value="ball">"ball"</option>
                                    <option value="ring">"ring"</option>
                                    <option value="image">"image"</option>
                                    <option value="square">"square"</option>
                                </select>
                                <label
                                    for="prop-color"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "image") { "dialog-field-hidden" } else { "" }
                                >"Color"</label>
                                <input
                                    id="prop-color"
                                    type="color"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "image") { "dialog-field-hidden" } else { "prop-color-input" }
                                    prop:value=move || {
                                        define_prop_dialog
                                            .get()
                                            .map(|dialog| dialog.color)
                                            .unwrap_or_else(|| "#ff0000".to_string())
                                    }
                                    on:input=move |ev| {
                                        let value = event_target_value(&ev);
                                        set_define_prop_dialog.update(|dialog| {
                                            if let Some(dialog) = dialog {
                                                dialog.color = value;
                                            }
                                        });
                                    }
                                />
                                <label
                                    for="prop-diameter"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| matches!(dialog.prop_type.as_str(), "ball" | "square")) { "" } else { "dialog-field-hidden" }
                                >"Diameter (cm)"</label>
                                <input
                                    id="prop-diameter"
                                    type="number"
                                    min="0.1"
                                    step="0.1"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| matches!(dialog.prop_type.as_str(), "ball" | "square")) { "" } else { "dialog-field-hidden" }
                                    prop:value=move || define_prop_dialog.get().map(|dialog| dialog.diameter.to_string()).unwrap_or_else(|| "10".to_string())
                                    on:input=move |ev| {
                                        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
                                            set_define_prop_dialog.update(|dialog| {
                                                if let Some(dialog) = dialog {
                                                    dialog.diameter = value;
                                                }
                                            });
                                        }
                                    }
                                />
                                <label
                                    for="prop-outside-diameter"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "ring") { "" } else { "dialog-field-hidden" }
                                >"Outside diameter (cm)"</label>
                                <input
                                    id="prop-outside-diameter"
                                    type="number"
                                    min="0.1"
                                    step="0.1"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "ring") { "" } else { "dialog-field-hidden" }
                                    prop:value=move || define_prop_dialog.get().map(|dialog| dialog.diameter.to_string()).unwrap_or_else(|| "25".to_string())
                                    on:input=move |ev| {
                                        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
                                            set_define_prop_dialog.update(|dialog| {
                                                if let Some(dialog) = dialog {
                                                    dialog.diameter = value;
                                                }
                                            });
                                        }
                                    }
                                />
                                <label
                                    for="prop-inside-diameter"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "ring") { "" } else { "dialog-field-hidden" }
                                >"Inside diameter (cm)"</label>
                                <input
                                    id="prop-inside-diameter"
                                    type="number"
                                    min="0.1"
                                    step="0.1"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "ring") { "" } else { "dialog-field-hidden" }
                                    prop:value=move || define_prop_dialog.get().map(|dialog| dialog.inside_diameter.to_string()).unwrap_or_else(|| "20".to_string())
                                    on:input=move |ev| {
                                        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
                                            set_define_prop_dialog.update(|dialog| {
                                                if let Some(dialog) = dialog {
                                                    dialog.inside_diameter = value;
                                                }
                                            });
                                        }
                                    }
                                />
                                <label
                                    for="prop-image-source"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "image") { "" } else { "dialog-field-hidden" }
                                >"Image source"</label>
                                <input
                                    id="prop-image-source"
                                    type="text"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "image") { "" } else { "dialog-field-hidden" }
                                    prop:value=move || define_prop_dialog.get().map(|dialog| decode_image_source(&dialog.image_source)).unwrap_or_default()
                                    on:input=move |ev| {
                                        let value = encode_image_source(&event_target_value(&ev));
                                        set_define_prop_dialog.update(|dialog| {
                                            if let Some(dialog) = dialog {
                                                dialog.image_source = value;
                                            }
                                        });
                                    }
                                />
                                <label
                                    for="prop-image-file"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "image") { "" } else { "dialog-field-hidden" }
                                >"Custom image"</label>
                                <input
                                    id="prop-image-file"
                                    type="file"
                                    accept="image/*"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "image") { "" } else { "dialog-field-hidden" }
                                    on:change=choose_custom_prop_image
                                />
                                <label
                                    for="prop-image-width"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "image") { "" } else { "dialog-field-hidden" }
                                >"Width (cm)"</label>
                                <input
                                    id="prop-image-width"
                                    type="number"
                                    min="0.1"
                                    step="0.1"
                                    class=move || if define_prop_dialog.get().is_some_and(|dialog| dialog.prop_type == "image") { "" } else { "dialog-field-hidden" }
                                    prop:value=move || define_prop_dialog.get().map(|dialog| dialog.image_width.to_string()).unwrap_or_else(|| "10".to_string())
                                    on:input=move |ev| {
                                        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
                                            if value.is_finite() && value > 0.0 {
                                                set_define_prop_dialog.update(|dialog| {
                                                    if let Some(dialog) = dialog {
                                                        dialog.image_width = value;
                                                    }
                                                });
                                            }
                                        }
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

fn text_record(display: String) -> PatternRecord {
    PatternRecord {
        display,
        notation: None,
        config: None,
        animprefs: None,
        info: None,
        tags: Vec::new(),
        raw_pattern: None,
    }
}

fn remove_pattern_list_record(document: &mut PatternListDocument, index: usize) {
    if index >= document.records.len() {
        return;
    }
    document.records.remove(index);
    document.selected = if document.records.is_empty() {
        None
    } else {
        Some(index.min(document.records.len() - 1))
    };
    document.dirty = true;
}

fn move_pattern_list_record(document: &mut PatternListDocument, from: usize, target: usize) {
    if from >= document.records.len() || from == target {
        return;
    }
    let record = document.records.remove(from);
    let target = if target > from { target - 1 } else { target };
    let target = target.min(document.records.len());
    document.records.insert(target, record);
    document.selected = Some(target);
    document.dirty = true;
}

fn confirm_pattern_list_replacement(document: Option<&PatternListDocument>) -> bool {
    if !document.is_some_and(|document| document.dirty) {
        return true;
    }
    window()
        .and_then(|window| {
            window
                .confirm_with_message("Discard unsaved changes to the current pattern list?")
                .ok()
        })
        .unwrap_or(false)
}

fn pattern_list_filename(title: &str, extension: &str) -> String {
    let stem = title
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    let stem = stem.trim_matches('_');
    let stem = if stem.is_empty() {
        "pattern-list"
    } else {
        stem
    };
    format!("{stem}.{extension}")
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

fn split_animation_prefs(config: &str) -> Result<(String, AnimationPrefs), String> {
    let trimmed = config.trim();
    if trimmed.starts_with('<') || !trimmed.contains('=') {
        return Ok((config.to_string(), AnimationPrefs::default()));
    }
    let mut parameters = ParameterList::parse(Some(config))?;
    let prefs = AnimationPrefs::from_parameters(&mut parameters)?;
    Ok((parameters.to_string(), prefs))
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

fn image_prop_modifier(source: &str, width: f64) -> String {
    format!(
        "image={};width={}",
        encode_image_source(&decode_image_source(source)),
        width.max(0.1)
    )
}

fn define_prop_modifier(dialog: &DefinePropDraft) -> Result<Option<String>, String> {
    if dialog.prop_type.eq_ignore_ascii_case("image") {
        if dialog.image_source.trim().is_empty() {
            return Err("Image source cannot be empty".to_string());
        }
        if !dialog.image_width.is_finite() || dialog.image_width <= 0.0 {
            return Err("Image width must be greater than zero".to_string());
        }
        return Ok(Some(image_prop_modifier(
            &dialog.image_source,
            dialog.image_width,
        )));
    }

    let color = jml_color_from_input(&dialog.color)?;
    let default_color = "#ff0000";
    let mut parameters = Vec::new();
    if !dialog.color.eq_ignore_ascii_case(default_color) {
        parameters.push(format!("color={color}"));
    }

    match dialog.prop_type.as_str() {
        "ball" | "square" => {
            validate_prop_diameter(dialog.diameter, "Diameter")?;
            if (dialog.diameter - 10.0).abs() > 1e-9 {
                parameters.push(format!("diam={}", to_string_rounded(dialog.diameter, 4)));
            }
        }
        "ring" => {
            validate_prop_diameter(dialog.diameter, "Outside diameter")?;
            validate_prop_diameter(dialog.inside_diameter, "Inside diameter")?;
            if (dialog.diameter - 25.0).abs() > 1e-9 {
                parameters.push(format!("outside={}", to_string_rounded(dialog.diameter, 4)));
            }
            if (dialog.inside_diameter - 20.0).abs() > 1e-9 {
                parameters.push(format!(
                    "inside={}",
                    to_string_rounded(dialog.inside_diameter, 4)
                ));
            }
        }
        _ => return Err(format!("Unknown prop type: {}", dialog.prop_type)),
    }

    Ok((!parameters.is_empty()).then(|| parameters.join(";")))
}

fn validate_prop_diameter(value: f64, label: &str) -> Result<(), String> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(format!("{label} must be greater than zero"))
    }
}

fn prop_color_input_value(color: Option<&str>) -> String {
    let Some(color) = color.map(str::trim) else {
        return "#ff0000".to_string();
    };
    if color.len() == 7
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return color.to_ascii_lowercase();
    }
    let components = color
        .split_once('(')
        .and_then(|(_, tail)| tail.strip_suffix(')'))
        .map(|values| {
            values
                .split(',')
                .take(3)
                .map(|value| value.trim().parse::<u8>())
                .collect::<Result<Vec<_>, _>>()
        });
    match components {
        Some(Ok(components)) if components.len() == 3 => format!(
            "#{:02x}{:02x}{:02x}",
            components[0], components[1], components[2]
        ),
        _ => "#ff0000".to_string(),
    }
}

fn jml_color_from_input(color: &str) -> Result<String, String> {
    let color = color.trim();
    if color.len() != 7
        || !color.starts_with('#')
        || !color.as_bytes()[1..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Prop color must be a six-digit color".to_string());
    }
    let named = match color.to_ascii_lowercase().as_str() {
        "#000000" => Some("black"),
        "#0000ff" => Some("blue"),
        "#00ffff" => Some("cyan"),
        "#808080" => Some("gray"),
        "#00ff00" => Some("green"),
        "#ff00ff" => Some("magenta"),
        "#ffc800" => Some("orange"),
        "#ffafaf" => Some("pink"),
        "#ff0000" => Some("red"),
        "#ffffff" => Some("white"),
        "#ffff00" => Some("yellow"),
        _ => None,
    };
    if let Some(named) = named {
        return Ok(named.to_string());
    }
    let red = u8::from_str_radix(&color[1..3], 16)
        .map_err(|_| "Prop color must be a six-digit color".to_string())?;
    let green = u8::from_str_radix(&color[3..5], 16)
        .map_err(|_| "Prop color must be a six-digit color".to_string())?;
    let blue = u8::from_str_radix(&color[5..7], 16)
        .map_err(|_| "Prop color must be a six-digit color".to_string())?;
    Ok(format!("{{{red},{green},{blue}}}"))
}

fn event_drag_sources(
    record: &PatternRecord,
    spec: &AnimationSpec,
    hit: &canvas::EventEditorHit,
) -> Result<(Coordinate, MhnJmlEvent), String> {
    let AnimationKind::Jml(jml) = &spec.kind else {
        return Err("No event layout available for this pattern".to_string());
    };
    let layout = jml
        .layout
        .as_ref()
        .ok_or_else(|| "No physical layout available for this pattern".to_string())?;
    let image = layout
        .events
        .iter()
        .filter(|event| {
            event.primary_index == hit.primary_index && event.event.hand == hit.image_hand
        })
        .min_by(|left, right| {
            cyclic_time_distance(left.event.t, hit.event_time, jml.period_secs).total_cmp(
                &cyclic_time_distance(right.event.t, hit.event_time, jml.period_secs),
            )
        })
        .ok_or_else(|| "Selected event image is no longer available".to_string())?;
    let xml = record_to_pattern_jml(record)?;
    let model = MhnJmlPattern::from_jml_xml(&xml)?;
    let primary = model
        .events
        .get(hit.primary_index)
        .cloned()
        .ok_or_else(|| "Selected primary event is no longer available".to_string())?;
    Ok((
        Coordinate {
            x: image.event.x,
            y: image.event.y,
            z: image.event.z,
        },
        primary,
    ))
}

fn cyclic_time_distance(left: f64, right: f64, period: f64) -> f64 {
    if period <= 0.0 {
        return (left - right).abs();
    }
    let delta = (left - right).rem_euclid(period);
    delta.min(period - delta)
}

fn event_from_canvas_drag(drag: &EventCanvasDrag, dx: f64, dy: f64) -> Coordinate {
    const SNAP_CM: f64 = 3.0;
    let mut image = drag.start_image;
    match drag.hit.handle {
        canvas::EventEditHandle::Xz => {
            if let Some((local_x, local_z)) = solve_screen_basis(
                dx,
                dy,
                drag.hit.local_x_dx,
                drag.hit.local_x_dy,
                drag.hit.z_dx,
                drag.hit.z_dy,
            ) {
                image.x += local_x;
                image.z += local_z;
                if image.z.abs() < SNAP_CM {
                    image.z = 0.0;
                }
            }
        }
        canvas::EventEditHandle::Y => {
            let length_squared = drag.hit.local_y_dx * drag.hit.local_y_dx
                + drag.hit.local_y_dy * drag.hit.local_y_dy;
            if length_squared > 1e-9 {
                image.y += (dx * drag.hit.local_y_dx + dy * drag.hit.local_y_dy) / length_squared;
                if image.y.abs() < SNAP_CM {
                    image.y = 0.0;
                }
            }
        }
    }

    let mut delta = Coordinate {
        x: image.x - drag.start_image.x,
        y: image.y - drag.start_image.y,
        z: image.z - drag.start_image.z,
    };
    if drag.hit.image_hand != drag.start_primary.hand {
        delta.x = -delta.x;
    }
    Coordinate {
        x: drag.start_primary.x + delta.x,
        y: drag.start_primary.y + delta.y,
        z: drag.start_primary.z + delta.z,
    }
}

fn event_edit_status(handle: canvas::EventEditHandle, event: Coordinate) -> String {
    match handle {
        canvas::EventEditHandle::Xz => {
            format!("Event x {:.1}, z {:.1} cm", event.x, event.z)
        }
        canvas::EventEditHandle::Y => format!("Event y {:.1} cm", event.y),
    }
}

fn position_from_canvas_drag(drag: &PositionCanvasDrag, dx: f64, dy: f64) -> BodyPosition {
    const GRID_SPACING_CM: f64 = 20.0;
    const SNAP_CM: f64 = 3.0;
    const ANGLE_HANDLE_CM: f64 = 20.0;
    const ANGLE_SNAP_RADIANS: f64 = 8.0_f64.to_radians();
    let mut position = drag.start_position;
    match drag.hit.handle {
        canvas::PositionEditHandle::Xy => {
            if let Some((local_x, local_y)) = solve_screen_basis(
                dx,
                dy,
                drag.hit.local_x_dx,
                drag.hit.local_x_dy,
                drag.hit.local_y_dx,
                drag.hit.local_y_dy,
            ) {
                let angle = drag.start_position.angle.to_radians();
                position.x += local_x * angle.cos() - local_y * angle.sin();
                position.y += local_x * angle.sin() + local_y * angle.cos();
                position.x = snap_grid_value(position.x, GRID_SPACING_CM, SNAP_CM);
                position.y = snap_grid_value(position.y, GRID_SPACING_CM, SNAP_CM);
            }
        }
        canvas::PositionEditHandle::Z => {
            let length_squared = drag.hit.z_dx * drag.hit.z_dx + drag.hit.z_dy * drag.hit.z_dy;
            if length_squared > 1e-9 {
                position.z += (dx * drag.hit.z_dx + dy * drag.hit.z_dy) / length_squared;
                for target in [0.0, 100.0] {
                    if (position.z - target).abs() < SNAP_CM {
                        position.z = target;
                    }
                }
            }
        }
        canvas::PositionEditHandle::Angle => {
            let control_x = -drag.hit.local_y_dx * ANGLE_HANDLE_CM + dx;
            let control_y = -drag.hit.local_y_dy * ANGLE_HANDLE_CM + dy;
            if let Some((a, b)) = solve_screen_basis(
                control_x,
                control_y,
                drag.hit.local_x_dx,
                drag.hit.local_x_dy,
                drag.hit.local_y_dx,
                drag.hit.local_y_dy,
            ) {
                let mut angle = drag.start_position.angle.to_radians() - (-a).atan2(-b);
                for cardinal in [
                    0.0,
                    std::f64::consts::FRAC_PI_2,
                    std::f64::consts::PI,
                    1.5 * std::f64::consts::PI,
                ] {
                    if normalized_angle_difference(angle, cardinal) < ANGLE_SNAP_RADIANS / 2.0 {
                        angle = cardinal;
                        break;
                    }
                }
                position.angle = angle.to_degrees().rem_euclid(360.0);
            }
        }
    }
    position
}

fn solve_screen_basis(dx: f64, dy: f64, ax: f64, ay: f64, bx: f64, by: f64) -> Option<(f64, f64)> {
    let determinant = ax * by - ay * bx;
    (determinant.abs() > 1e-9).then(|| {
        (
            (by * dx - bx * dy) / determinant,
            (-ay * dx + ax * dy) / determinant,
        )
    })
}

fn snap_grid_value(value: f64, spacing: f64, threshold: f64) -> f64 {
    let target = spacing * (value / spacing).round();
    if (value - target).abs() < threshold {
        target
    } else {
        value
    }
}

fn normalized_angle_difference(left: f64, right: f64) -> f64 {
    ((left - right + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI)
        .abs()
}

fn snap_camera_angles(yaw: f64, pitch: f64, horizontal_reference: Option<f64>) -> (f64, f64) {
    let snapped_pitch = if pitch < CAMERA_SNAP_ANGLE {
        CAMERA_MIN_PITCH
    } else if normalized_angle_difference(pitch, std::f64::consts::FRAC_PI_2) < CAMERA_SNAP_ANGLE {
        std::f64::consts::FRAC_PI_2
    } else if pitch > std::f64::consts::PI - CAMERA_SNAP_ANGLE {
        CAMERA_MAX_PITCH
    } else {
        pitch
    };

    let mut snapped_yaw = yaw;
    if let Some(reference) = horizontal_reference {
        for quarter_turn in 0..4 {
            let target = reference + quarter_turn as f64 * std::f64::consts::FRAC_PI_2;
            if normalized_angle_difference(yaw, target) < CAMERA_SNAP_ANGLE {
                snapped_yaw = target.rem_euclid(std::f64::consts::TAU);
                break;
            }
        }
    }
    (snapped_yaw, snapped_pitch)
}

fn camera_snap_reference(spec: &AnimationSpec, selected_id: &str, time: f64) -> Option<f64> {
    let AnimationKind::Jml(jml) = &spec.kind else {
        return None;
    };
    let diagram = build_ladder_diagram(jml);
    if diagram
        .positions
        .iter()
        .any(|position| position.id == selected_id)
    {
        return Some(0.0);
    }

    let event_target = diagram
        .events
        .iter()
        .find(|event| event.id == selected_id)
        .map(|event| (event.juggler, event.time))
        .or_else(|| {
            diagram
                .transitions
                .iter()
                .find(|transition| transition.id == selected_id)
                .map(|transition| (transition.juggler, transition.time))
        });
    let target = event_target.or_else(|| (jml.jugglers == 1).then_some((1, time)))?;
    let angle = jml
        .layout
        .as_ref()?
        .juggler_angle(target.0, target.1)
        .ok()?;
    Some((-angle.to_radians()).rem_euclid(std::f64::consts::TAU))
}

fn position_edit_status(handle: canvas::PositionEditHandle, position: BodyPosition) -> String {
    match handle {
        canvas::PositionEditHandle::Xy => {
            format!("Position x {:.1}, y {:.1} cm", position.x, position.y)
        }
        canvas::PositionEditHandle::Z => format!("Position z {:.1} cm", position.z),
        canvas::PositionEditHandle::Angle => {
            format!("Position angle {:.1} degrees", position.angle)
        }
    }
}

fn editor_shortcut_target_is_editable(event: &ev::KeyboardEvent) -> bool {
    let Some(target) = event.target() else {
        return false;
    };
    let Ok(element) = target.dyn_into::<web_sys::Element>() else {
        return false;
    };
    matches!(
        element.tag_name().as_str(),
        "INPUT" | "TEXTAREA" | "SELECT" | "BUTTON" | "A"
    ) || element
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

fn edit_ladder_event_spatial_in_record(
    record: &PatternRecord,
    event_index: usize,
    coordinate: Coordinate,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    let target = model
        .events
        .get_mut(event_index)
        .ok_or_else(|| "Selected ladder event is no longer available".to_string())?;
    target.x = coordinate.x;
    target.y = coordinate.y;
    target.z = coordinate.z;
    target.calcpos = false;
    model.rebuild_path_events();
    record_from_edited_jml_model(record, model, "Event spatial edit rejected")
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

fn color_props_in_record(
    record: &PatternRecord,
    color_string: &str,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    model.apply_prop_colors(color_string)?;
    record_from_edited_jml_model(record, model, "Color props rejected")
}

fn transform_pattern_record(
    record: &PatternRecord,
    transform: PatternTransform,
) -> Result<PatternRecord, String> {
    let xml = record_to_pattern_jml(record)?;
    let model = MhnJmlPattern::from_jml_xml(&xml)?;
    let transformed = match transform {
        PatternTransform::Optimize => optimize_pattern(&model)?.pattern,
        PatternTransform::SwapHands => model.with_inverted_x_axis(false),
        PatternTransform::FlipX => model.with_inverted_x_axis(true),
        PatternTransform::FlipTime => model.with_inverted_time()?,
    };
    record_from_edited_jml_model(record, transformed, "Pattern transform rejected")
}

fn record_props_are_colorable(record: &PatternRecord) -> bool {
    record_to_pattern_jml(record)
        .and_then(|xml| MhnJmlPattern::from_jml_xml(&xml))
        .is_ok_and(|model| model.props.iter().all(MhnJmlProp::is_colorable))
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
    mut model: MhnJmlPattern,
    error_prefix: &str,
) -> Result<PatternRecord, String> {
    if model.info.is_none() {
        model.info = record.info.clone();
    }
    if model.tags.is_empty() {
        model.tags = record.tags.clone();
    }
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
        info: model.info.clone(),
        tags: model.tags.clone(),
        raw_pattern: Some(raw_pattern),
    };

    let spec = AnimationSpec::from_record(&edited)?;
    match spec.kind {
        AnimationKind::Jml(_) => Ok(edited),
        AnimationKind::Unavailable(err) => Err(format!("Edited JML did not produce layout: {err}")),
    }
}

fn selection_mutation_records(
    record: &PatternRecord,
    options: &MutatorOptions,
) -> Result<Vec<PatternRecord>, String> {
    let xml = record_to_pattern_jml(record)?;
    let model = MhnJmlPattern::from_jml_xml(&xml)?;
    let mut variants = Vec::with_capacity(9);
    for index in 0..9 {
        if index == 4 {
            variants.push(record.clone());
            continue;
        }
        let mutation = mutate_pattern_with_random(&model, options, &mut js_sys::Math::random)?;
        variants.push(record_from_edited_jml_model(
            record,
            mutation.pattern,
            "Selection mutation rejected",
        )?);
    }
    Ok(variants)
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
    points: Vec<(f64, f64)>,
    class_name: &'static str,
}

#[derive(Clone)]
enum LadderShape {
    Line(LadderSegment),
    Arc(LadderArc),
}

fn ladder_diagram(spec: &AnimationSpec) -> Option<LadderDiagram> {
    match &spec.kind {
        AnimationKind::Jml(jml) if ladder_limit(jml.jugglers, jml.paths).is_none() => {
            Some(build_ladder_diagram(jml))
        }
        AnimationKind::Jml(_) => None,
        AnimationKind::Unavailable(_) => None,
    }
}

fn ladder_unavailable_reason(spec: &AnimationSpec) -> Option<String> {
    match &spec.kind {
        AnimationKind::Jml(jml) => match ladder_limit(jml.jugglers, jml.paths) {
            Some(LadderLimit::Jugglers { maximum, .. }) => {
                Some(format!("Ladder supports at most {maximum} jugglers"))
            }
            Some(LadderLimit::Paths { maximum, .. }) => {
                Some(format!("Ladder supports at most {maximum} paths"))
            }
            None => None,
        },
        AnimationKind::Unavailable(_) => Some("Ladder unavailable".to_string()),
    }
}

fn ladder_track_views(spec: &AnimationSpec, zoom: f64) -> Vec<AnyView> {
    let Some(diagram) = ladder_diagram(spec) else {
        return Vec::new();
    };

    diagram
        .tracks
        .iter()
        .map(|track| {
            let x = ladder_track_x(&diagram, track.index);
            let label = track.label.clone();
            let bottom = ladder_period_bottom(zoom);
            view! {
                <line x1=x y1=LADDER_TOP_Y x2=x y2=bottom class="hand-line" />
                <text x=x y="4" class="ladder-label">{label}</text>
            }
            .into_any()
        })
        .collect()
}

fn ladder_symmetry_views(spec: &AnimationSpec, zoom: f64) -> Vec<AnyView> {
    let Some(diagram) = ladder_diagram(spec) else {
        return Vec::new();
    };
    let bottom = ladder_period_bottom(zoom);
    let view_height = ladder_view_height(zoom);
    let mut views = vec![
        view! {
            <line x1="0" y1=LADDER_TOP_Y x2="100" y2=LADDER_TOP_Y class="ladder-symmetry" />
        }
        .into_any(),
        view! {
            <line x1="0" y1=bottom x2="100" y2=bottom class="ladder-symmetry" />
        }
        .into_any(),
    ];

    if diagram.has_switch_symmetry {
        let left = diagram
            .tracks
            .iter()
            .map(|track| ladder_track_x(&diagram, track.index))
            .fold(f64::INFINITY, f64::min);
        let right = diagram
            .tracks
            .iter()
            .map(|track| ladder_track_x(&diagram, track.index))
            .fold(f64::NEG_INFINITY, f64::max);
        let margin = view_height - bottom;
        let middle_y = bottom + margin / 2.0;
        let upper_y = bottom + margin / 4.0;
        let lower_y = bottom + margin * 3.0 / 4.0;
        let arrow_dx = 2.0 * (lower_y - middle_y);
        let segments = [
            (left, middle_y, right, middle_y),
            (left, middle_y, left + arrow_dx, upper_y),
            (left, middle_y, left + arrow_dx, lower_y),
            (right, middle_y, right - arrow_dx, upper_y),
            (right, middle_y, right - arrow_dx, lower_y),
        ];
        views.extend(segments.into_iter().map(|(x1, y1, x2, y2)| {
            view! {
                <line x1=x1 y1=y1 x2=x2 y2=y2 class="ladder-symmetry" />
            }
            .into_any()
        }));
    }

    if diagram.has_switch_delay_symmetry {
        let half_period_y = ladder_time_y(&diagram, diagram.period_secs / 2.0, zoom);
        views.push(
            view! {
                <line x1="0" y1=half_period_y x2="100" y2=half_period_y class="ladder-symmetry" />
            }
            .into_any(),
        );
    }

    views
}

fn ladder_tracker_view(spec: &AnimationSpec, time: f64, zoom: f64, paused: bool) -> AnyView {
    let Some(diagram) = ladder_diagram(spec) else {
        return view! {}.into_any();
    };
    let y = ladder_time_y(&diagram, time, zoom);
    let label = ladder_tracker_label(&diagram, time, paused);
    view! {
        <g class="ladder-tracker-group">
            <line x1="0" y1=y x2="100" y2=y class="ladder-tracker" />
            {label.map(|label| view! {
                <text x="50" y=y - 1.8 class="ladder-tracker-label">{label}</text>
            })}
        </g>
    }
    .into_any()
}

fn ladder_tracker_label(diagram: &LadderDiagram, time: f64, paused: bool) -> Option<String> {
    paused.then(|| {
        format!(
            "{} s",
            to_string_rounded(time.rem_euclid(diagram.period_secs.max(0.1)), 2)
        )
    })
}

fn ladder_tracker_hitbox_view(spec: &AnimationSpec, time: f64, zoom: f64) -> AnyView {
    let Some(diagram) = ladder_diagram(spec) else {
        return view! {}.into_any();
    };
    let y = ladder_time_y(&diagram, time, zoom);
    view! {
        <line x1="0" y1=y x2="100" y2=y class="ladder-tracker-hitbox" />
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
            <polyline points=ladder_arc_points_attribute(&arc.points) class=arc.class_name />
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
            <polyline points=ladder_arc_points_attribute(&arc.points) class="ladder-path-hitbox" />
        }
        .into_any(),
    }
}

fn ladder_edge_shapes(
    diagram: &LadderDiagram,
    edge: &LadderEdge,
    drag: Option<&LadderDrag>,
    zoom: f64,
    metrics: LadderViewMetrics,
) -> Vec<LadderShape> {
    let x1 = ladder_endpoint_x(diagram, &edge.start, metrics);
    let start_time = ladder_endpoint_preview_time(&edge.start, drag);
    let y1 = ladder_absolute_time_y(diagram, start_time, zoom);
    let x2 = ladder_endpoint_x(diagram, &edge.end, metrics);
    let end_time = ladder_endpoint_preview_time(&edge.end, drag);
    let y2 = ladder_absolute_time_y(diagram, end_time, zoom);
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
) -> Option<Vec<(f64, f64)>> {
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

    let mut points = Vec::with_capacity(ARC_STEPS + 1);
    for step in 0..=ARC_STEPS {
        let fraction = step as f64 / ARC_STEPS as f64;
        let angle = angle_start + delta * fraction;
        let x = circle_x + radius * angle.cos();
        let y = circle_y + radius * angle.sin();
        points.push((x, y));
    }
    Some(points)
}

fn ladder_arc_points_attribute(points: &[(f64, f64)]) -> String {
    points
        .iter()
        .map(|(x, y)| format!("{x:.3},{y:.3}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ladder_track_x(diagram: &LadderDiagram, track_index: usize) -> f64 {
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
    let width_units = 2.0 * LADDER_BORDER_SIDES
        + jugglers as f64
        + (jugglers.saturating_sub(1)) as f64 * LADDER_JUGGLER_SEPARATION;
    let hand_offset = match track.hand {
        LadderHand::Left => 0.0,
        LadderHand::Right => 1.0,
    };
    let x_units = LADDER_BORDER_SIDES
        + (track.juggler.saturating_sub(1)) as f64 * (1.0 + LADDER_JUGGLER_SEPARATION)
        + hand_offset;
    100.0 * x_units / width_units
}

fn ladder_view_metrics(diagram: &LadderDiagram, zoom: f64) -> LadderViewMetrics {
    let width_px = window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("ladder-svg"))
        .map(|element| element.get_bounding_client_rect().width())
        .filter(|width| width.is_finite() && *width > 0.0)
        .unwrap_or(300.0);
    let px_per_unit = width_px / 100.0;
    let mobile = window()
        .and_then(|window| window.match_media("(pointer: coarse)").ok().flatten())
        .is_some_and(|query| query.matches());
    let sizing = ladder_item_sizing(
        diagram,
        width_px,
        ladder_period_height(zoom) * px_per_unit,
        mobile,
    );
    let transition_radius = ladder_radius_units(sizing.transition_radius_px, px_per_unit, mobile);
    let position_radius = ladder_radius_units(sizing.position_radius_px, px_per_unit, mobile);
    LadderViewMetrics {
        transition_radius,
        position_radius,
    }
}

fn ladder_radius_units(radius_px: f64, px_per_unit: f64, mobile: bool) -> f64 {
    let pixel_radius = radius_px / px_per_unit.max(0.001);
    if mobile {
        pixel_radius
    } else {
        pixel_radius.max(LADDER_DESKTOP_RADIUS_UNITS)
    }
}

fn ladder_endpoint_x(
    diagram: &LadderDiagram,
    endpoint: &LadderEndpoint,
    metrics: LadderViewMetrics,
) -> f64 {
    let track_x = ladder_track_x(diagram, endpoint.track_index);
    ladder_transition_x_from_parts(track_x, endpoint.hand, endpoint.transition_index, metrics)
}

fn ladder_transition_x(
    diagram: &LadderDiagram,
    transition: &LadderTransition,
    metrics: LadderViewMetrics,
) -> f64 {
    let track_x = ladder_track_x(diagram, transition.track_index);
    ladder_transition_x_from_parts(
        track_x,
        transition.hand,
        transition.transition_index,
        metrics,
    )
}

fn ladder_transition_x_from_parts(
    track_x: f64,
    hand: LadderHand,
    transition_index: usize,
    metrics: LadderViewMetrics,
) -> f64 {
    let direction = match hand {
        LadderHand::Left => 1.0,
        LadderHand::Right => -1.0,
    };
    track_x + direction * (transition_index as f64 + 1.0) * 2.0 * metrics.transition_radius
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

fn ladder_view_height(zoom: f64) -> f64 {
    100.0 * zoom.clamp(LADDER_MIN_ZOOM, LADDER_MAX_ZOOM)
}

fn ladder_fit_zoom(width: f64, height: f64) -> Option<f64> {
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let usable_height = (height - LADDER_FIT_GAP_PX).max(1.0);
    Some((usable_height / width).clamp(LADDER_MIN_ZOOM, LADDER_MAX_ZOOM))
}

fn ladder_background_should_pan(zoom: f64, scroll_height: i32, client_height: i32) -> bool {
    zoom > LADDER_MIN_ZOOM && scroll_height > client_height
}

fn fit_ladder_to_height(
    set_ladder_zoom: WriteSignal<f64>,
    set_status: WriteSignal<String>,
    announce: bool,
) -> bool {
    let Some(scroll) = window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("ladder-scroll"))
    else {
        return false;
    };
    let Some(zoom) = ladder_fit_zoom(scroll.client_width() as f64, scroll.client_height() as f64)
    else {
        return false;
    };
    set_ladder_zoom.set(zoom);
    scroll.set_scroll_top(0);
    if announce {
        set_status.set(format!("Ladder fitted to height ({:.0}%)", zoom * 100.0));
    }
    true
}

fn ladder_period_height(zoom: f64) -> f64 {
    (ladder_view_height(zoom) - LADDER_TOP_Y - LADDER_BOTTOM_MARGIN).max(1.0)
}

fn ladder_period_bottom(zoom: f64) -> f64 {
    LADDER_TOP_Y + ladder_period_height(zoom)
}

fn ladder_scroll_target(
    old_zoom: f64,
    new_zoom: f64,
    old_canvas_height: f64,
    old_scroll_top: f64,
    anchor_y: f64,
    top_margin: f64,
    bottom_margin: f64,
) -> f64 {
    let new_canvas_height = old_canvas_height * new_zoom / old_zoom.max(1e-9);
    let old_scrollable = (old_canvas_height - top_margin - bottom_margin).max(1.0);
    let new_scrollable = (new_canvas_height - top_margin - bottom_margin).max(1.0);
    let canvas_anchor = old_scroll_top + anchor_y;
    let mapped_anchor = (canvas_anchor - top_margin) * new_scrollable / old_scrollable + top_margin;
    (mapped_anchor - anchor_y).max(0.0)
}

fn ladder_touch_distance(touches: &[LadderTouch]) -> Option<f64> {
    let [first, second, ..] = touches else {
        return None;
    };
    Some(
        ((first.client_x - second.client_x).powi(2) + (first.client_y - second.client_y).powi(2))
            .sqrt(),
    )
}

fn ladder_touch_centroid_y(touches: &[LadderTouch]) -> Option<f64> {
    (!touches.is_empty())
        .then(|| touches.iter().map(|touch| touch.client_y).sum::<f64>() / touches.len() as f64)
}

fn ladder_time_y(diagram: &LadderDiagram, time: f64, zoom: f64) -> f64 {
    LADDER_TOP_Y
        + (time.rem_euclid(diagram.period_secs) / diagram.period_secs) * ladder_period_height(zoom)
}

fn ladder_playback_cycle(diagram: &LadderDiagram, time: f64) -> i64 {
    (time / diagram.period_secs.max(0.1)).floor() as i64
}

fn ladder_time_in_cycle(diagram: &LadderDiagram, cycle: i64, local_time: f64) -> f64 {
    let period = diagram.period_secs.max(0.1);
    cycle as f64 * period + local_time.clamp(0.0, period - 1e-6)
}

fn ladder_absolute_time_y(diagram: &LadderDiagram, time: f64, zoom: f64) -> f64 {
    LADDER_TOP_Y + (time / diagram.period_secs) * ladder_period_height(zoom)
}

fn nearest_ladder_edge_at_client(
    spec: &AnimationSpec,
    drag: Option<&LadderDrag>,
    zoom: f64,
    client_x: f64,
    client_y: f64,
) -> Option<(String, String)> {
    const PATH_SLOP_PX: f64 = 5.0;

    let diagram = ladder_diagram(spec)?;
    let element = window()?.document()?.get_element_by_id("ladder-svg")?;
    let rect = element.get_bounding_client_rect();
    let width = rect.width();
    let height = rect.height();
    if !width.is_finite() || width <= 0.0 || !height.is_finite() || height <= 0.0 {
        return None;
    }

    let point_x = (client_x - rect.left()) / width * 100.0;
    let point_y = (client_y - rect.top()) / height * ladder_view_height(zoom);
    let slop = PATH_SLOP_PX * 100.0 / width;
    if point_y < LADDER_TOP_Y - slop || point_y > ladder_period_bottom(zoom) + slop {
        return None;
    }

    let metrics = ladder_view_metrics(&diagram, zoom);
    diagram
        .edges
        .iter()
        .filter_map(|edge| {
            let distance = ladder_edge_shapes(&diagram, edge, drag, zoom, metrics)
                .iter()
                .map(|shape| ladder_shape_distance(shape, point_x, point_y))
                .fold(f64::INFINITY, f64::min);
            (distance < slop).then_some((edge, distance))
        })
        .min_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(edge, _)| (edge.id.clone(), ladder_edge_label(edge)))
}

fn ladder_shape_distance(shape: &LadderShape, x: f64, y: f64) -> f64 {
    match shape {
        LadderShape::Line(segment) => {
            point_segment_distance(x, y, segment.x1, segment.y1, segment.x2, segment.y2)
        }
        LadderShape::Arc(arc) => arc
            .points
            .windows(2)
            .map(|points| {
                point_segment_distance(x, y, points[0].0, points[0].1, points[1].0, points[1].1)
            })
            .fold(f64::INFINITY, f64::min),
    }
}

fn point_segment_distance(x: f64, y: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f64::EPSILON {
        return ((x - x1).powi(2) + (y - y1).powi(2)).sqrt();
    }
    let fraction = (((x - x1) * dx + (y - y1) * dy) / length_squared).clamp(0.0, 1.0);
    let nearest_x = x1 + fraction * dx;
    let nearest_y = y1 + fraction * dy;
    ((x - nearest_x).powi(2) + (y - nearest_y).powi(2)).sqrt()
}

fn ladder_time_from_client_y(client_y: i32, diagram: &LadderDiagram, zoom: f64) -> Option<f64> {
    let element = window()?.document()?.get_element_by_id("ladder-svg")?;
    let rect = element.get_bounding_client_rect();
    let height = rect.height();
    if !height.is_finite() || height <= 0.0 {
        return None;
    }

    let y = ((client_y as f64 - rect.top()) / height * ladder_view_height(zoom))
        .clamp(LADDER_TOP_Y, ladder_period_bottom(zoom));
    let fraction = (y - LADDER_TOP_Y) / ladder_period_height(zoom);
    Some(fraction * diagram.period_secs.max(0.1))
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
    let color = ladder_prop_spec(spec, path, time)
        .color
        .unwrap_or_else(|| "#d8dde6".to_string());
    format!("--ladder-prop-color: {color};")
}

fn ladder_prop_spec(spec: &AnimationSpec, path: usize, time: f64) -> PropSpec {
    match &spec.kind {
        AnimationKind::Jml(jml) => jml
            .prop_for_path_at_time(path, time)
            .cloned()
            .unwrap_or_else(|| PropSpec::default_for_type("ball")),
        AnimationKind::Unavailable(_) => PropSpec::default_for_type("ball"),
    }
}

fn ladder_prop_marker_view(
    prop: &PropSpec,
    x: f64,
    y: f64,
    radius: f64,
    class_name: &'static str,
) -> AnyView {
    match &prop.kind {
        PropKind::Square => view! {
            <rect
                class=format!("{class_name} ladder-prop-square")
                x=x - radius
                y=y - radius
                width=2.0 * radius
                height=2.0 * radius
            />
        }
        .into_any(),
        PropKind::Ring => view! {
            <g class=format!("{class_name} ladder-prop-ring")>
                <circle cx=x cy=y r=radius />
                <circle class="ladder-prop-ring-hole" cx=x cy=y r=radius * 0.48 />
            </g>
        }
        .into_any(),
        PropKind::Image => {
            let source = prop
                .image_source
                .as_deref()
                .map(ladder_image_source_url)
                .unwrap_or_else(|| "./assets/ball.png".to_string());
            view! {
                <image
                    class=format!("{class_name} ladder-prop-image")
                    href=source
                    x=x - radius
                    y=y - radius
                    width=2.0 * radius
                    height=2.0 * radius
                    preserveAspectRatio="xMidYMid meet"
                />
            }
            .into_any()
        }
        PropKind::Ball | PropKind::Unknown(_) => view! {
            <circle class=class_name cx=x cy=y r=radius />
        }
        .into_any(),
    }
}

fn ladder_image_source_url(source: &str) -> String {
    let decoded = source.trim().replace("%3B", ";").replace("%3b", ";");
    if decoded.contains('/') || decoded.starts_with("data:") || decoded.starts_with("blob:") {
        decoded
    } else {
        format!("./assets/{decoded}")
    }
}

fn image_source_label(source: &str) -> String {
    let decoded = decode_image_source(source);
    if decoded.starts_with("data:") {
        return "embedded image".to_string();
    }
    const MAX_LABEL_CHARS: usize = 96;
    if decoded.chars().count() <= MAX_LABEL_CHARS {
        return decoded;
    }
    let mut label = decoded
        .chars()
        .take(MAX_LABEL_CHARS - 3)
        .collect::<String>();
    label.push_str("...");
    label
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

fn selected_ladder_event_selection(
    spec: &AnimationSpec,
    selected_id: &str,
) -> Option<canvas::EventSelection> {
    let diagram = ladder_diagram(spec)?;
    if let Some(event) = diagram.events.iter().find(|event| event.id == selected_id) {
        return Some(canvas::EventSelection {
            primary_index: event.event_index,
            time: event.time,
        });
    }
    diagram
        .transitions
        .iter()
        .find(|transition| transition.id == selected_id)
        .map(|transition| canvas::EventSelection {
            primary_index: transition.event_index,
            time: transition.time,
        })
}

fn ladder_event_id_for_editor_hit(
    spec: &AnimationSpec,
    hit: &canvas::EventEditorHit,
) -> Option<String> {
    let diagram = ladder_diagram(spec)?;
    let hand = if hit.image_hand == 1 {
        LadderHand::Left
    } else {
        LadderHand::Right
    };
    diagram
        .events
        .iter()
        .filter(|event| event.event_index == hit.primary_index && event.hand == hand)
        .min_by(|left, right| {
            cyclic_time_distance(left.time, hit.event_time, diagram.period_secs).total_cmp(
                &cyclic_time_distance(right.time, hit.event_time, diagram.period_secs),
            )
        })
        .map(|event| event.id.clone())
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
    let prop_spec = PropSpec::from_jml(&prop.prop_type, prop.modifier.as_deref())
        .unwrap_or_else(|_| PropSpec::default_for_type(&prop.prop_type));
    let image_source = prop_spec
        .image_source
        .clone()
        .unwrap_or_else(|| "ball.png".to_string());
    let color = prop_color_input_value(prop_spec.color.as_deref());

    Ok(Some(DefinePropDraft {
        path,
        selected_id: selected_id.to_string(),
        prop_assignment,
        playback_time: time.rem_euclid(spec.period_secs.max(0.1)),
        prop_type: prop.prop_type.to_ascii_lowercase(),
        color,
        diameter: prop_spec.diameter.max(0.1),
        inside_diameter: prop_spec.inside_diameter.unwrap_or(20.0).max(0.1),
        image_source,
        image_width: prop_spec.diameter.max(0.1),
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

fn selected_ladder_can_remove_event(spec: &AnimationSpec, selected_id: &str) -> bool {
    let Some(diagram) = ladder_diagram(spec) else {
        return false;
    };
    diagram
        .events
        .iter()
        .find(|event| event.id == selected_id)
        .is_some_and(|event| ladder_event_can_remove(&diagram, event))
}

fn selected_ladder_can_remove_position(spec: &AnimationSpec, selected_id: &str) -> bool {
    let Some(diagram) = ladder_diagram(spec) else {
        return false;
    };
    diagram
        .positions
        .iter()
        .any(|position| position.id == selected_id)
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

fn playback_speed(prefs: &AnimationPrefs) -> f64 {
    if prefs.slowdown.is_finite() && prefs.slowdown > 0.0 {
        1.0 / prefs.slowdown
    } else {
        1.0 / AnimationPrefs::SLOWDOWN_DEFAULT
    }
}

fn show_ground_for_pattern(show_ground: ShowGround, spec: &AnimationSpec) -> bool {
    match show_ground {
        ShowGround::On => true,
        ShowGround::Off => false,
        ShowGround::Auto => match &spec.kind {
            AnimationKind::Jml(jml) => jml
                .layout
                .as_ref()
                .is_some_and(|layout| layout.is_bounce_pattern()),
            AnimationKind::Unavailable(_) => false,
        },
    }
}

fn initial_camera_angles(spec: &AnimationSpec, prefs: &AnimationPrefs) -> (f64, f64) {
    if let Some([yaw, pitch]) = prefs.default_camera_angle {
        return (yaw.to_radians(), pitch.clamp(0.0001, 179.9999).to_radians());
    }
    let jugglers = match &spec.kind {
        AnimationKind::Jml(jml) => jml.jugglers,
        AnimationKind::Unavailable(_) => 1,
    };
    if jugglers == 1 {
        (0.0, 90.0_f64.to_radians())
    } else {
        (340.0_f64.to_radians(), 70.0_f64.to_radians())
    }
}

fn initial_view_mode(spec: &AnimationSpec, default_view: DefaultView) -> &'static str {
    match default_view {
        DefaultView::Simple => "simple",
        DefaultView::Edit => "edit",
        DefaultView::Pattern => "pattern",
        DefaultView::Selection => "selection",
        DefaultView::None => match &spec.kind {
            AnimationKind::Jml(jml) if jml.jugglers > MAX_JUGGLERS => "simple",
            _ => "edit",
        },
    }
}

async fn portable_pattern_jml(record: &PatternRecord) -> Result<String, String> {
    let xml = record_to_pattern_jml(record)?;
    let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
    let mut cache = HashMap::new();
    if embed_external_image_props(&mut model, &mut cache).await? {
        model.assert_valid()?;
        Ok(model.write_jml(true, true))
    } else {
        Ok(xml)
    }
}

async fn portable_pattern_list_records(
    mut records: Vec<PatternRecord>,
) -> Result<Vec<PatternRecord>, String> {
    let mut cache = HashMap::new();
    for record in &mut records {
        if !record.is_playable() {
            continue;
        }
        let xml = record_to_pattern_jml(record)?;
        let mut model = MhnJmlPattern::from_jml_xml(&xml)?;
        if !embed_external_image_props(&mut model, &mut cache).await? {
            continue;
        }
        model.assert_valid()?;
        record.notation = Some("jml".to_string());
        record.raw_pattern = Some(jml::extract_pattern_xml(&model.write_jml(true, true))?);
    }
    Ok(records)
}

async fn embed_external_image_props(
    model: &mut MhnJmlPattern,
    cache: &mut HashMap<String, String>,
) -> Result<bool, String> {
    let mut changed = false;
    for index in 0..model.props.len() {
        let Some(source) = model.props[index].image_source()? else {
            continue;
        };
        if !image_source_requires_embedding(&source) {
            continue;
        }
        let data_url = if let Some(cached) = cache.get(&source) {
            cached.clone()
        } else {
            let data_url = fetch_image_data_url(&source).await?;
            cache.insert(source.clone(), data_url.clone());
            data_url
        };
        model.props[index].set_image_source(&data_url)?;
        changed = true;
    }
    Ok(changed)
}

async fn fetch_image_data_url(source: &str) -> Result<String, String> {
    let label = image_source_label(source);
    let response = JsFuture::from(
        window()
            .ok_or_else(|| "Browser window is unavailable".to_string())?
            .fetch_with_str(source),
    )
    .await
    .map_err(|error| {
        js_error_message(
            &format!("Unable to package image '{label}'. The image server must allow CORS"),
            error,
        )
    })?
    .dyn_into::<Response>()
    .map_err(|_| format!("Image '{label}' returned an invalid response"))?;
    if !response.ok() {
        return Err(format!(
            "Unable to package image '{label}': HTTP status {}",
            response.status()
        ));
    }
    let declared_type = response.headers().get("Content-Type").ok().flatten();
    let buffer = JsFuture::from(
        response
            .array_buffer()
            .map_err(|error| js_error_message("Unable to read image response", error))?,
    )
    .await
    .map_err(|error| js_error_message(&format!("Unable to read image '{label}'"), error))?;
    let array = Uint8Array::new(&buffer);
    let mut bytes = vec![0; array.length() as usize];
    array.copy_to(&mut bytes);
    if bytes.is_empty() {
        return Err(format!("Image '{label}' is empty"));
    }
    let mime_type = portable_image_mime_type(source, declared_type.as_deref(), &bytes)?;
    Ok(format!(
        "data:{mime_type};base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

fn portable_image_mime_type(
    source: &str,
    declared_type: Option<&str>,
    bytes: &[u8],
) -> Result<String, String> {
    if let Some(mime_type) = declared_type
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| value.to_ascii_lowercase().starts_with("image/"))
    {
        return Ok(mime_type.to_ascii_lowercase());
    }

    let detected = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some("image/x-icon")
    } else if bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && (&bytes[8..12] == b"avif" || &bytes[8..12] == b"avis")
    {
        Some("image/avif")
    } else if std::str::from_utf8(&bytes[..bytes.len().min(512)])
        .ok()
        .is_some_and(|prefix| prefix.to_ascii_lowercase().contains("<svg"))
    {
        Some("image/svg+xml")
    } else {
        None
    };
    if let Some(mime_type) = detected {
        return Ok(mime_type.to_string());
    }

    let path = source
        .split(['?', '#'])
        .next()
        .unwrap_or(source)
        .to_ascii_lowercase();
    let inferred = if path.ends_with(".png") {
        Some("image/png")
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if path.ends_with(".gif") {
        Some("image/gif")
    } else if path.ends_with(".webp") {
        Some("image/webp")
    } else if path.ends_with(".svg") {
        Some("image/svg+xml")
    } else if path.ends_with(".avif") {
        Some("image/avif")
    } else if path.ends_with(".bmp") {
        Some("image/bmp")
    } else if path.ends_with(".ico") {
        Some("image/x-icon")
    } else {
        None
    };
    inferred.map(str::to_string).ok_or_else(|| {
        format!(
            "Image '{}' did not return a recognized image format",
            image_source_label(source)
        )
    })
}

async fn request_generation(
    arguments: String,
    controller: AbortController,
) -> Result<GenerationResult, String> {
    request_pattern_search("/api/generate", "Generator", arguments, controller).await
}

async fn request_transition(
    arguments: String,
    controller: AbortController,
) -> Result<GenerationResult, String> {
    request_pattern_search("/api/transition", "Transitioner", arguments, controller).await
}

async fn request_pattern_search(
    endpoint: &str,
    operation: &str,
    arguments: String,
    controller: AbortController,
) -> Result<GenerationResult, String> {
    let body = serde_json::to_string(&serde_json::json!({ "arguments": arguments }))
        .map_err(|error| format!("Unable to encode generator request: {error}"))?;
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(&body));
    init.set_signal(Some(&controller.signal()));
    let request = Request::new_with_str_and_init(endpoint, &init).map_err(|error| {
        js_error_message(&format!("Unable to create {operation} request"), error)
    })?;
    request
        .headers()
        .set("Content-Type", "application/json")
        .map_err(|error| {
            js_error_message(&format!("Unable to configure {operation} request"), error)
        })?;
    let response = JsFuture::from(
        window()
            .ok_or_else(|| "Browser window is unavailable".to_string())?
            .fetch_with_request(&request),
    )
    .await
    .map_err(|error| js_error_message(&format!("{operation} request failed"), error))?
    .dyn_into::<Response>()
    .map_err(|_| format!("{operation} endpoint returned an invalid response"))?;
    let status = response.status();
    let text = JsFuture::from(
        response
            .text()
            .map_err(|error| js_error_message("Unable to read generator response", error))?,
    )
    .await
    .map_err(|error| js_error_message("Unable to read generator response", error))?
    .as_string()
    .unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(if text.is_empty() {
            format!("{operation} failed with HTTP status {status}")
        } else {
            text
        });
    }
    serde_json::from_str(&text)
        .map_err(|error| format!("Unable to decode generator response: {error}"))
}

fn js_error_message(context: &str, error: JsValue) -> String {
    error
        .as_string()
        .map(|message| format!("{context}: {message}"))
        .unwrap_or_else(|| context.to_string())
}

fn download_text(filename: &str, text: &str) {
    let Some(document) = window().and_then(|win| win.document()) else {
        return;
    };

    let parts = js_sys::Array::new();
    parts.push(&wasm_bindgen::JsValue::from_str(text));
    let options = BlobPropertyBag::new();
    options.set_type(if filename.to_ascii_lowercase().ends_with(".txt") {
        "text/plain;charset=utf-8"
    } else {
        "application/xml;charset=utf-8"
    });
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

fn download_blob(filename: &str, blob: &Blob) -> Result<(), String> {
    let document = window()
        .and_then(|window| window.document())
        .ok_or_else(|| "Document is unavailable for animation download".to_string())?;
    let url = web_sys::Url::create_object_url_with_blob(blob)
        .map_err(|_| "Unable to create animation download URL".to_string())?;
    let anchor = document
        .create_element("a")
        .map_err(|_| "Unable to create animation download".to_string())?
        .dyn_into::<HtmlAnchorElement>()
        .map_err(|_| "Unable to create animation download".to_string())?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();
    web_sys::Url::revoke_object_url(&url).ok();
    Ok(())
}

fn shared_pattern_from_location() -> Result<Option<share::DecodedShare>, String> {
    let Some(window) = window() else {
        return Ok(None);
    };
    let query = window
        .location()
        .search()
        .map_err(|error| js_error_message("Unable to read shared pattern URL", error))?;
    let query = query.strip_prefix('?').unwrap_or(&query);
    if query.trim().is_empty() {
        return Ok(None);
    }
    share::decode_share_url(query).map(Some)
}

fn current_share_base_url() -> Result<String, String> {
    let window = window().ok_or_else(|| "Browser window is unavailable".to_string())?;
    let location = window.location();
    let origin = location
        .origin()
        .map_err(|error| js_error_message("Unable to read page origin", error))?;
    let pathname = location
        .pathname()
        .map_err(|error| js_error_message("Unable to read page path", error))?;
    Ok(format!("{origin}{pathname}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_snap_uses_original_pitch_thresholds() {
        let (_, side) = snap_camera_angles(0.3, 86.0_f64.to_radians(), None);
        let (_, unsnapped) = snap_camera_angles(0.3, 80.0_f64.to_radians(), None);

        assert!((side - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((unsnapped - 80.0_f64.to_radians()).abs() < 1e-12);
    }

    #[test]
    fn camera_snap_uses_selected_juggler_reference_quarters() {
        let reference = 32.0_f64.to_radians();
        let near_quarter = reference + 94.0_f64.to_radians();
        let (snapped, _) =
            snap_camera_angles(near_quarter, std::f64::consts::FRAC_PI_2, Some(reference));
        let (free, _) = snap_camera_angles(
            reference + 100.0_f64.to_radians(),
            std::f64::consts::FRAC_PI_2,
            Some(reference),
        );

        assert!(
            normalized_angle_difference(snapped, reference + std::f64::consts::FRAC_PI_2) < 1e-12
        );
        assert!(normalized_angle_difference(free, reference + 100.0_f64.to_radians()) < 1e-12);
    }

    #[test]
    fn ladder_zoom_stretches_only_the_timeline() {
        assert!((ladder_period_height(1.0) - 86.0).abs() < 1e-12);
        assert!((ladder_period_height(2.0) - 186.0).abs() < 1e-12);
        assert!((ladder_period_bottom(2.0) + LADDER_BOTTOM_MARGIN - 200.0).abs() < 1e-12);
    }

    #[test]
    fn ladder_fit_height_preserves_square_minimum_and_zoom_limit() {
        assert_eq!(ladder_fit_zoom(400.0, 602.0), Some(1.5));
        assert_eq!(ladder_fit_zoom(600.0, 300.0), Some(1.0));
        assert_eq!(ladder_fit_zoom(100.0, 2_000.0), Some(10.0));
        assert_eq!(ladder_fit_zoom(0.0, 300.0), None);
    }

    #[test]
    fn desktop_ladder_nodes_keep_the_previous_view_relative_minimum() {
        assert_eq!(ladder_radius_units(5.0, 5.0, false), 2.0);
        assert_eq!(ladder_radius_units(5.0, 2.0, false), 2.5);
        assert_eq!(ladder_radius_units(22.0, 5.0, true), 4.4);
    }

    #[test]
    fn ladder_background_pans_only_when_a_scrollbar_has_content() {
        assert!(!ladder_background_should_pan(2.0, 600, 600));
        assert!(!ladder_background_should_pan(2.0, 599, 600));
        assert!(ladder_background_should_pan(2.0, 601, 600));
        assert!(!ladder_background_should_pan(1.0, 900, 600));
    }

    #[test]
    fn ladder_zoom_keeps_the_pointer_anchor_stable() {
        let target = ladder_scroll_target(1.0, 2.0, 500.0, 100.0, 200.0, 40.0, 30.0);
        let old_fraction = (100.0 + 200.0 - 40.0) / (500.0 - 40.0 - 30.0);
        let new_fraction = (target + 200.0 - 40.0) / (1000.0 - 40.0 - 30.0);

        assert!((old_fraction - new_fraction).abs() < 1e-12);
    }

    #[test]
    fn ladder_pinch_uses_pointer_distance_and_centroid() {
        let touches = [
            LadderTouch {
                pointer_id: 1,
                client_x: 10.0,
                client_y: 20.0,
            },
            LadderTouch {
                pointer_id: 2,
                client_x: 13.0,
                client_y: 24.0,
            },
        ];

        assert!((ladder_touch_distance(&touches).unwrap() - 5.0).abs() < 1e-12);
        assert!((ladder_touch_centroid_y(&touches).unwrap() - 22.0).abs() < 1e-12);
    }

    #[test]
    fn ladder_path_hit_distance_uses_the_nearest_segment_or_arc() {
        let line = LadderShape::Line(LadderSegment {
            x1: 10.0,
            y1: 10.0,
            x2: 30.0,
            y2: 10.0,
            class_name: "cross-throw",
        });
        let arc = LadderShape::Arc(LadderArc {
            points: vec![(10.0, 20.0), (20.0, 25.0), (30.0, 20.0)],
            class_name: "self-throw",
        });

        assert!((ladder_shape_distance(&line, 20.0, 13.0) - 3.0).abs() < 1e-12);
        assert!(ladder_shape_distance(&arc, 20.0, 24.0) < 1.0);
        assert!(ladder_shape_distance(&line, 20.0, 24.0) > 10.0);
    }

    #[test]
    fn canvas_event_hit_resolves_the_matching_ladder_event_image() {
        let spec =
            AnimationSpec::from_record(&PatternRecord::siteswap("Cascade", "pattern=3")).unwrap();
        let diagram = ladder_diagram(&spec).unwrap();
        let event = diagram.events.last().unwrap();
        let hit = canvas::EventEditorHit {
            primary_index: event.event_index,
            event_time: event.time,
            image_hand: match event.hand {
                LadderHand::Left => 1,
                LadderHand::Right => 0,
            },
            handle: canvas::EventEditHandle::Xz,
            local_x_dx: 0.0,
            local_x_dy: 0.0,
            local_y_dx: 0.0,
            local_y_dy: 0.0,
            z_dx: 0.0,
            z_dy: 0.0,
        };

        assert_eq!(
            ladder_event_id_for_editor_hit(&spec, &hit).as_deref(),
            Some(event.id.as_str())
        );
    }

    #[test]
    fn paused_ladder_tracker_shows_wrapped_original_time_label() {
        let spec =
            AnimationSpec::from_record(&PatternRecord::siteswap("Cascade", "pattern=3")).unwrap();
        let diagram = ladder_diagram(&spec).unwrap();

        assert_eq!(ladder_tracker_label(&diagram, 0.456, false), None);
        assert_eq!(
            ladder_tracker_label(&diagram, diagram.period_secs + 0.456, true).as_deref(),
            Some("0.46 s")
        );
    }

    #[test]
    fn image_error_labels_do_not_expose_embedded_payloads() {
        assert_eq!(
            image_source_label("data:image/png%3Bbase64,abcdef"),
            "embedded image"
        );
        assert_eq!(image_source_label("ball.png"), "ball.png");
        assert!(image_source_label(&"x".repeat(120)).ends_with("..."));
    }

    #[test]
    fn portable_image_packaging_detects_declared_magic_and_path_formats() {
        assert_eq!(
            portable_image_mime_type("asset.bin", Some("image/PNG; charset=binary"), b"data")
                .unwrap(),
            "image/png"
        );
        assert_eq!(
            portable_image_mime_type(
                "asset.bin",
                Some("application/octet-stream"),
                b"\x89PNG\r\n\x1a\nrest"
            )
            .unwrap(),
            "image/png"
        );
        assert_eq!(
            portable_image_mime_type("asset.webp?version=2", None, b"unknown").unwrap(),
            "image/webp"
        );
        assert!(portable_image_mime_type("asset.bin", Some("text/html"), b"not an image").is_err());
    }

    #[test]
    fn pattern_list_rows_move_and_remove_without_losing_selection() {
        let mut document = PatternListDocument {
            title: "Test".to_string(),
            info: None,
            records: vec![
                text_record("A".to_string()),
                text_record("B".to_string()),
                text_record("C".to_string()),
            ],
            selected: Some(0),
            dirty: false,
        };

        move_pattern_list_record(&mut document, 0, 3);
        assert_eq!(
            document
                .records
                .iter()
                .map(|record| record.display.as_str())
                .collect::<Vec<_>>(),
            vec!["B", "C", "A"]
        );
        assert_eq!(document.selected, Some(2));

        remove_pattern_list_record(&mut document, 2);
        assert_eq!(document.selected, Some(1));
        assert!(document.dirty);
    }

    #[test]
    fn pattern_list_filenames_are_browser_safe() {
        assert_eq!(
            pattern_list_filename("My favorite patterns", "jml"),
            "My_favorite_patterns.jml"
        );
        assert_eq!(pattern_list_filename("***", "txt"), "pattern-list.txt");
    }

    #[test]
    fn pattern_entry_separates_animation_preferences_from_notation_parameters() {
        let (config, prefs) = split_animation_prefs(concat!(
            "pattern=3;title=Cascade;slowdown=4;showground=on;",
            "camangle=(20,75);view=pattern_editor"
        ))
        .unwrap();

        assert_eq!(config, "pattern=3;title=Cascade");
        assert_eq!(prefs.slowdown, 4.0);
        assert_eq!(prefs.show_ground, ShowGround::On);
        assert_eq!(prefs.default_camera_angle, Some([20.0, 75.0]));
        assert_eq!(prefs.default_view, DefaultView::Pattern);
    }

    #[test]
    fn preferences_dialog_preserves_manual_original_fields() {
        let prefs = AnimationPrefs {
            default_camera_angle: Some([340.0, 70.0]),
            default_view: DefaultView::Selection,
            hide_jugglers: vec![2, 4],
            ..AnimationPrefs::default()
        };
        let dialog = AnimationPrefsDialogState::from_prefs(&prefs);

        assert!(dialog.manual_settings.contains("camangle=(340.0,70.0)"));
        assert_eq!(dialog.to_prefs().unwrap(), prefs);
    }

    #[test]
    fn automatic_ground_matches_bounce_paths_only() {
        let cascade = AnimationSpec::from_record(&PatternRecord::siteswap(
            "Cascade".to_string(),
            "pattern=3".to_string(),
        ))
        .unwrap();
        let bounce = AnimationSpec::from_record(&PatternRecord::siteswap(
            "Bounce".to_string(),
            "pattern=3BHL".to_string(),
        ))
        .unwrap();

        assert!(!show_ground_for_pattern(ShowGround::Auto, &cascade));
        assert!(show_ground_for_pattern(ShowGround::Auto, &bounce));
    }

    #[test]
    fn generator_form_matches_original_control_arguments() {
        assert_eq!(GeneratorForm::default().arguments(), "5 7 5 -f -se -n");

        let passing = GeneratorForm {
            jugglers: 2,
            excited_state: false,
            ..GeneratorForm::default()
        };
        assert_eq!(
            passing.arguments(),
            "5 7 5 -j 2 -d 0 -l 1 -jp -cp -f -g -se -n"
        );
    }

    #[test]
    fn transitioner_form_matches_original_control_arguments() {
        assert_eq!(TransitionerForm::default().arguments(), "- -");

        let form = TransitionerForm {
            from_pattern: "3".to_string(),
            to_pattern: "51".to_string(),
            multiplexing: true,
            simultaneous_throws: "3".to_string(),
            no_simultaneous_catches: false,
            no_clustered_throws: true,
        };
        assert_eq!(form.arguments(), "3 51 -m 3 -mf -mc");
    }

    #[test]
    fn color_props_command_updates_structured_jml() {
        let record = PatternRecord::siteswap("Cascade".to_string(), "pattern=3".to_string());
        assert!(record_props_are_colorable(&record));

        let edited = color_props_in_record(&record, "mixed").unwrap();
        let xml = record_to_pattern_jml(&edited).unwrap();
        let model = MhnJmlPattern::from_jml_xml(&xml).unwrap();

        assert_eq!(model.prop_assignment, vec![1, 2, 3]);
        assert_eq!(model.props[0].modifier.as_deref(), Some("color=red"));
        assert_eq!(model.props[1].modifier.as_deref(), Some("color=green"));
        assert_eq!(model.props[2].modifier.as_deref(), Some("color=blue"));
    }

    #[test]
    fn structured_jml_edits_preserve_record_info_and_tags() {
        let mut record = PatternRecord::siteswap("Cascade", "pattern=3");
        record.info = Some("Practice notes".to_string());
        record.tags = vec!["solo".to_string(), "technical".to_string()];

        let xml = record_to_pattern_jml(&record).unwrap();
        let mut model = MhnJmlPattern::from_jml_xml(&xml).unwrap();
        assert_eq!(model.info, record.info);
        assert_eq!(model.tags, record.tags);

        model.events[0].x += 1.0;
        model.rebuild_path_events();
        let edited = record_from_edited_jml_model(&record, model, "Edit rejected").unwrap();
        let reparsed =
            MhnJmlPattern::from_jml_xml(&record_to_pattern_jml(&edited).unwrap()).unwrap();
        assert_eq!(reparsed.info.as_deref(), Some("Practice notes"));
        assert_eq!(reparsed.tags, vec!["solo", "technical"]);
    }

    #[test]
    fn color_props_command_is_unavailable_for_images() {
        let record = PatternRecord::siteswap("Cascade".to_string(), "pattern=3".to_string());
        let xml = record_to_pattern_jml(&record).unwrap();
        let mut model = MhnJmlPattern::from_jml_xml(&xml).unwrap();
        model.props = vec![MhnJmlProp::new(
            "image",
            Some("image=ball.png;width=10".to_string()),
        )];
        model.prop_assignment = vec![1; model.number_of_paths];
        let image_record =
            record_from_edited_jml_model(&record, model, "Image prop rejected").unwrap();

        assert!(!record_props_are_colorable(&image_record));
        assert!(color_props_in_record(&image_record, "mixed").is_err());
    }

    #[test]
    fn define_prop_builds_typed_modifiers_without_highlight() {
        let draft =
            |prop_type: &str, color: &str, diameter: f64, inside_diameter: f64| DefinePropDraft {
                path: 1,
                selected_id: "path-1".to_string(),
                prop_assignment: vec![1],
                playback_time: 0.0,
                prop_type: prop_type.to_string(),
                color: color.to_string(),
                diameter,
                inside_diameter,
                image_source: "ball.png".to_string(),
                image_width: 10.0,
            };

        assert_eq!(
            define_prop_modifier(&draft("ball", "#ff0000", 10.0, 20.0)).unwrap(),
            None
        );
        assert_eq!(
            define_prop_modifier(&draft("ball", "#0000ff", 12.0, 20.0))
                .unwrap()
                .as_deref(),
            Some("color=blue;diam=12")
        );
        let ring = define_prop_modifier(&draft("ring", "#00ff00", 30.0, 22.0))
            .unwrap()
            .unwrap();
        assert_eq!(ring, "color=green;outside=30;inside=22");
        assert!(!ring.contains("highlight"));
    }

    #[test]
    fn ladder_context_enablement_matches_item_types() {
        let record = PatternRecord::siteswap("Cascade", "pattern=3");
        let spec = AnimationSpec::from_record(&record).unwrap();
        let diagram = ladder_diagram(&spec).unwrap();
        let edge = diagram.edges.first().unwrap();
        let transition = diagram
            .transitions
            .iter()
            .find(|transition| transition.transition == TransitionKind::Throw)
            .unwrap();

        assert!(selected_ladder_can_add_at_context(&spec, ""));
        assert!(selected_ladder_can_add_at_context(&spec, &edge.id));
        assert!(selected_ladder_can_define_prop(&spec, &edge.id));
        assert!(selected_ladder_can_define_prop(&spec, &transition.id));
        assert!(selected_ladder_can_define_throw(&spec, &transition.id));
        assert!(!selected_ladder_can_add_at_context(&spec, &transition.id));
        assert!(!selected_ladder_can_remove_position(&spec, &transition.id));
    }

    #[test]
    fn pattern_transform_commands_write_valid_structured_jml() {
        let record = PatternRecord::siteswap("Cascade".to_string(), "pattern=3".to_string());
        let source_xml = record_to_pattern_jml(&record).unwrap();
        let source = MhnJmlPattern::from_jml_xml(&source_xml).unwrap();

        let swapped_record =
            transform_pattern_record(&record, PatternTransform::SwapHands).unwrap();
        let swapped_xml = record_to_pattern_jml(&swapped_record).unwrap();
        let swapped = MhnJmlPattern::from_jml_xml(&swapped_xml).unwrap();
        assert!(
            source
                .events
                .iter()
                .all(|event| swapped.events.iter().any(|candidate| {
                    candidate.juggler == event.juggler
                        && candidate.hand == 1 - event.hand
                        && (candidate.t - event.t).abs() < 1e-9
                        && (candidate.x - event.x).abs() < 1e-9
                }))
        );

        let flipped_record = transform_pattern_record(&record, PatternTransform::FlipX).unwrap();
        let flipped_xml = record_to_pattern_jml(&flipped_record).unwrap();
        let flipped = MhnJmlPattern::from_jml_xml(&flipped_xml).unwrap();
        assert!(
            source
                .events
                .iter()
                .all(|event| flipped.events.iter().any(|candidate| {
                    candidate.juggler == event.juggler
                        && candidate.hand == 1 - event.hand
                        && (candidate.t - event.t).abs() < 1e-9
                        && (candidate.x + event.x).abs() < 1e-9
                }))
        );

        let reversed_record =
            transform_pattern_record(&record, PatternTransform::FlipTime).unwrap();
        let reversed_xml = record_to_pattern_jml(&reversed_record).unwrap();
        let reversed = MhnJmlPattern::from_jml_xml(&reversed_xml).unwrap();
        assert!(reversed.assert_valid().is_ok());
        assert_eq!(reversed.events.len(), source.events.len());

        let optimizable = PatternRecord::siteswap("531".to_string(), "pattern=531".to_string());
        let optimized_record =
            transform_pattern_record(&optimizable, PatternTransform::Optimize).unwrap();
        let optimized_xml = record_to_pattern_jml(&optimized_record).unwrap();
        let optimized = MhnJmlPattern::from_jml_xml(&optimized_xml).unwrap();
        assert!(optimized.assert_valid().is_ok());
        assert_eq!(optimized_record.notation.as_deref(), Some("jml"));
    }
}
