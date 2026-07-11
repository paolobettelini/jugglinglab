use juggling_core::animation::{AnimationKind, AnimationSpec, JmlAnimation, Point3};
use juggling_core::layout::{JugglerFrame, LaidoutPattern, LayoutBounds, LayoutEvent};
use juggling_core::mhn_hands::Coordinate;
use juggling_core::prop::{PropKind, PropSpec};
use std::cell::RefCell;
use std::collections::HashMap;
use wasm_bindgen::{JsCast, closure::Closure};
use web_sys::{
    CanvasRenderingContext2d, CanvasWindingRule, HtmlCanvasElement, HtmlImageElement, window,
};

#[derive(Clone, Debug, PartialEq)]
pub struct RenderSettings {
    pub theme: String,
    pub speed: f64,
    pub zoom: f64,
    pub camera_yaw: f64,
    pub camera_pitch: f64,
    pub camera_pan_x: f64,
    pub camera_pan_y: f64,
    pub camera_pan_z: f64,
    pub paused: bool,
    pub show_trails: bool,
    pub show_grid: bool,
    pub selected_event: Option<EventSelection>,
    pub selected_position: Option<usize>,
    pub position_edit_handle: Option<PositionEditHandle>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventSelection {
    pub primary_index: usize,
    pub time: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventEditHandle {
    Xz,
    Y,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventEditorHit {
    pub primary_index: usize,
    pub event_time: f64,
    pub image_hand: usize,
    pub handle: EventEditHandle,
    pub local_x_dx: f64,
    pub local_x_dy: f64,
    pub local_y_dx: f64,
    pub local_y_dy: f64,
    pub z_dx: f64,
    pub z_dy: f64,
}

#[derive(Clone, Debug)]
struct EventEditorHitObject {
    hit: EventEditorHit,
    shape: HitShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PositionEditHandle {
    Xy,
    Z,
    Angle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PositionEditorHit {
    pub position_index: usize,
    pub handle: PositionEditHandle,
    pub center_x: f64,
    pub center_y: f64,
    pub local_x_dx: f64,
    pub local_x_dy: f64,
    pub local_y_dx: f64,
    pub local_y_dy: f64,
    pub z_dx: f64,
    pub z_dy: f64,
}

#[derive(Clone, Debug)]
struct PositionEditorHitObject {
    hit: PositionEditorHit,
    shape: HitShape,
}

struct CanvasAnimator {
    interval_id: i32,
    _closure: Closure<dyn FnMut()>,
}

#[derive(Clone, Debug)]
struct PlaybackClock {
    spec_key: Option<String>,
    time: f64,
    last_wall_ms: f64,
}

#[derive(Clone, Debug)]
pub struct HitObject {
    label: String,
    shape: HitShape,
}

#[derive(Clone, Debug)]
enum HitShape {
    Circle {
        x: f64,
        y: f64,
        radius: f64,
    },
    Rect {
        left: f64,
        top: f64,
        right: f64,
        bottom: f64,
    },
    Segment {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        radius: f64,
    },
    Polygon(Vec<(f64, f64)>),
}

impl HitObject {
    fn contains(&self, x: f64, y: f64) -> bool {
        self.shape.contains(x, y)
    }
}

impl HitShape {
    fn contains(&self, x: f64, y: f64) -> bool {
        match *self {
            HitShape::Circle {
                x: cx,
                y: cy,
                radius,
            } => {
                let dx = x - cx;
                let dy = y - cy;
                dx * dx + dy * dy <= radius * radius
            }
            HitShape::Rect {
                left,
                top,
                right,
                bottom,
            } => x >= left && x <= right && y >= top && y <= bottom,
            HitShape::Segment {
                x1,
                y1,
                x2,
                y2,
                radius,
            } => point_segment_distance(x, y, x1, y1, x2, y2) <= radius,
            HitShape::Polygon(ref points) => point_in_polygon(x, y, points),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ScreenPoint {
    x: f64,
    y: f64,
    z: f64,
    perspective: f64,
}

#[derive(Clone, Copy, Debug)]
struct RenderCamera {
    zoom: f64,
    yaw: f64,
    pitch: f64,
    matrix: Matrix4,
}

const DEFAULT_BALL_RADIUS_CM: f64 = 5.0;
const RING_POLYSIDES: usize = 200;
const TRAIL_MAX_WORLD_STEP_CM: f64 = 95.0;
const TRAIL_MAX_SCREEN_STEP_PX: f64 = 220.0;

#[derive(Clone, Copy, Debug)]
struct Matrix4 {
    m00: f64,
    m01: f64,
    m02: f64,
    m03: f64,
    m10: f64,
    m11: f64,
    m12: f64,
    m13: f64,
    m20: f64,
    m21: f64,
    m22: f64,
    m23: f64,
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[derive(Clone, Debug)]
struct RenderObject {
    kind: RenderObjectKind,
    coords: Vec<ScreenPoint>,
    bounds: Bounds,
    covering: Vec<usize>,
}

#[derive(Clone, Debug)]
enum RenderObjectKind {
    Prop {
        point: Point3,
        path: usize,
        prop: PropSpec,
        metrics: Prop2DMetrics,
    },
    Body {
        juggler: usize,
    },
    Line {
        juggler: usize,
    },
    Trail {
        alpha: f64,
    },
}

#[derive(Clone, Debug)]
struct Prop2DMetrics {
    width: f64,
    height: f64,
    center_x: f64,
    center_y: f64,
    grip_x: f64,
    grip_y: f64,
    shape: Prop2DShape,
}

#[derive(Clone, Debug)]
enum Prop2DShape {
    Ball,
    Square,
    Ring {
        outer: Vec<(f64, f64)>,
        inner: Vec<(f64, f64)>,
    },
    Image {
        source: String,
    },
    Fallback,
}

#[derive(Clone, Copy, Debug)]
struct Rgba {
    red: f64,
    green: f64,
    blue: f64,
    alpha: f64,
}

thread_local! {
    static ANIMATOR: RefCell<Option<CanvasAnimator>> = const { RefCell::new(None) };
    static LAST_HITS: RefCell<Vec<HitObject>> = const { RefCell::new(Vec::new()) };
    static LAST_EVENT_HITS: RefCell<Vec<EventEditorHitObject>> = const { RefCell::new(Vec::new()) };
    static LAST_POSITION_HITS: RefCell<Vec<PositionEditorHitObject>> = const { RefCell::new(Vec::new()) };
    static IMAGE_CACHE: RefCell<HashMap<String, HtmlImageElement>> = RefCell::new(HashMap::new());
    static PLAYBACK_CLOCK: RefCell<PlaybackClock> = RefCell::new(PlaybackClock {
        spec_key: None,
        time: 0.0,
        last_wall_ms: 0.0,
    });
}

impl Matrix4 {
    fn identity() -> Self {
        Self {
            m00: 1.0,
            m01: 0.0,
            m02: 0.0,
            m03: 0.0,
            m10: 0.0,
            m11: 1.0,
            m12: 0.0,
            m13: 0.0,
            m20: 0.0,
            m21: 0.0,
            m22: 1.0,
            m23: 0.0,
        }
    }

    fn shift(dx: f64, dy: f64, dz: f64) -> Self {
        let mut matrix = Self::identity();
        matrix.m03 = dx;
        matrix.m13 = dy;
        matrix.m23 = dz;
        matrix
    }

    fn scale(dx: f64, dy: f64, dz: f64) -> Self {
        let mut matrix = Self::identity();
        matrix.m00 = dx;
        matrix.m11 = dy;
        matrix.m22 = dz;
        matrix
    }

    fn uniform_scale(scale: f64) -> Self {
        Self::scale(scale, scale, scale)
    }

    fn rotate(dx: f64, dy: f64, dz: f64) -> Self {
        let mut out = Self::identity();
        if dx != 0.0 {
            let mut matrix = Self::identity();
            let sine = dx.sin();
            let cosine = dx.cos();
            matrix.m11 = cosine;
            matrix.m12 = sine;
            matrix.m21 = -sine;
            matrix.m22 = cosine;
            out.transform(matrix);
        }
        if dy != 0.0 {
            let mut matrix = Self::identity();
            let sine = dy.sin();
            let cosine = dy.cos();
            matrix.m00 = cosine;
            matrix.m02 = sine;
            matrix.m20 = -sine;
            matrix.m22 = cosine;
            out.transform(matrix);
        }
        if dz != 0.0 {
            let mut matrix = Self::identity();
            let sine = dz.sin();
            let cosine = dz.cos();
            matrix.m00 = cosine;
            matrix.m01 = sine;
            matrix.m10 = -sine;
            matrix.m11 = cosine;
            out.transform(matrix);
        }
        out
    }

    fn transform(&mut self, next: Self) {
        let current = *self;
        self.m00 = next.m00 * current.m00 + next.m01 * current.m10 + next.m02 * current.m20;
        self.m01 = next.m00 * current.m01 + next.m01 * current.m11 + next.m02 * current.m21;
        self.m02 = next.m00 * current.m02 + next.m01 * current.m12 + next.m02 * current.m22;
        self.m03 =
            next.m00 * current.m03 + next.m01 * current.m13 + next.m02 * current.m23 + next.m03;
        self.m10 = next.m10 * current.m00 + next.m11 * current.m10 + next.m12 * current.m20;
        self.m11 = next.m10 * current.m01 + next.m11 * current.m11 + next.m12 * current.m21;
        self.m12 = next.m10 * current.m02 + next.m11 * current.m12 + next.m12 * current.m22;
        self.m13 =
            next.m10 * current.m03 + next.m11 * current.m13 + next.m12 * current.m23 + next.m13;
        self.m20 = next.m20 * current.m00 + next.m21 * current.m10 + next.m22 * current.m20;
        self.m21 = next.m20 * current.m01 + next.m21 * current.m11 + next.m22 * current.m21;
        self.m22 = next.m20 * current.m02 + next.m21 * current.m12 + next.m22 * current.m22;
        self.m23 =
            next.m20 * current.m03 + next.m21 * current.m13 + next.m22 * current.m23 + next.m23;
    }

    fn project(self, point: Point3) -> ScreenPoint {
        let x = point.x;
        let y = point.z;
        let z = point.y;
        ScreenPoint {
            x: x * self.m00 + y * self.m01 + z * self.m02 + self.m03,
            y: x * self.m10 + y * self.m11 + z * self.m12 + self.m13,
            z: x * self.m20 + y * self.m21 + z * self.m22 + self.m23,
            perspective: 1.0,
        }
    }
}

impl RenderCamera {
    fn for_layout(
        width: f64,
        height: f64,
        settings: &RenderSettings,
        bounds: Option<LayoutBounds>,
        jugglers: usize,
    ) -> Self {
        let Some(bounds) = bounds else {
            let zoom = settings.zoom * (height / 520.0).clamp(0.72, 1.7);
            let center = Point3 {
                x: settings.camera_pan_x,
                y: settings.camera_pan_y,
                z: 75.0 + settings.camera_pan_z,
            };
            return Self {
                zoom,
                yaw: settings.camera_yaw,
                pitch: settings.camera_pitch,
                matrix: Self::build_matrix(
                    center,
                    width / 2.0 - zoom * center.x,
                    height / 2.0 + zoom * center.z,
                    zoom,
                    settings.camera_yaw,
                    settings.camera_pitch,
                ),
            };
        };

        let mut adjusted_min = bounds.min;
        let mut adjusted_max = bounds.max;
        if jugglers <= 1 {
            adjusted_min.z -= 0.3 * adjusted_min.y.abs().max(adjusted_max.y.abs());
            adjusted_max.z += 5.0;
        } else {
            let max_xy = adjusted_min
                .x
                .abs()
                .max(adjusted_max.x.abs())
                .max(adjusted_min.y.abs())
                .max(adjusted_max.y.abs());
            adjusted_min.z -= 0.4 * max_xy;
            adjusted_max.z += 0.4 * max_xy;
        }

        let max_abs_x = adjusted_min.x.abs().max(adjusted_max.x.abs()).max(1.0);
        adjusted_min.x = -max_abs_x;
        adjusted_max.x = max_abs_x;

        let viewport_width = (width * 0.84).max(1.0);
        let viewport_height = (height * 0.76).max(1.0);
        let zoom_orig = (viewport_width / (adjusted_max.x - adjusted_min.x).max(1.0))
            .min(viewport_height / (adjusted_max.z - adjusted_min.z).max(1.0))
            .clamp(0.05, 20.0);
        let zoom = zoom_orig * settings.zoom;
        let vertical_midpoint = 0.5 * (adjusted_max.z + adjusted_min.z);
        let limit = (height * 0.5) / zoom.max(0.001);
        let zoom_center_z = bounds.zoom_center.z;
        let center_z = vertical_midpoint.clamp(zoom_center_z - limit, zoom_center_z + limit);
        let center = Point3 {
            x: bounds.zoom_center.x + settings.camera_pan_x,
            y: bounds.zoom_center.y + settings.camera_pan_y,
            z: center_z + settings.camera_pan_z,
        };

        Self {
            zoom,
            yaw: settings.camera_yaw,
            pitch: settings.camera_pitch,
            matrix: Self::build_matrix(
                center,
                width / 2.0 - zoom * center.x,
                height / 2.0 + zoom * center.z,
                zoom,
                settings.camera_yaw,
                settings.camera_pitch,
            ),
        }
    }

    fn project(self, point: Point3) -> ScreenPoint {
        let mut projected = self.matrix.project(point);
        projected.perspective = ((self.zoom * DEFAULT_BALL_RADIUS_CM) / 8.5).powi(2);
        projected
    }

    fn build_matrix(
        center: Point3,
        origin_x: f64,
        origin_y: f64,
        zoom: f64,
        yaw: f64,
        pitch: f64,
    ) -> Matrix4 {
        let camera_center_x = center.x;
        let camera_center_y = center.z;
        let camera_center_z = center.y;
        let mut matrix = Matrix4::shift(-camera_center_x, -camera_center_y, -camera_center_z);
        matrix.transform(Matrix4::rotate(0.0, std::f64::consts::PI - yaw, 0.0));
        matrix.transform(Matrix4::rotate(
            0.5 * std::f64::consts::PI - pitch,
            0.0,
            0.0,
        ));
        matrix.transform(Matrix4::shift(
            camera_center_x,
            camera_center_y,
            camera_center_z,
        ));
        matrix.transform(Matrix4::scale(1.0, -1.0, 1.0));
        matrix.transform(Matrix4::uniform_scale(zoom));
        matrix.transform(Matrix4::shift(origin_x, origin_y, 0.0));
        matrix
    }
}

impl RenderObject {
    fn prop(point: Point3, path: usize, prop: PropSpec, camera: &RenderCamera) -> Self {
        let coord = camera.project(point);
        let metrics = prop_2d_metrics(&prop, camera.zoom, camera.yaw, camera.pitch);
        Self {
            kind: RenderObjectKind::Prop {
                point,
                path,
                prop,
                metrics: metrics.clone(),
            },
            coords: vec![coord],
            bounds: Bounds {
                left: coord.x - metrics.center_x,
                top: coord.y - metrics.center_y,
                right: coord.x - metrics.center_x + metrics.width,
                bottom: coord.y - metrics.center_y + metrics.height,
            },
            covering: Vec::new(),
        }
    }

    fn body(frame: &JugglerFrame, juggler: usize, camera: &RenderCamera) -> Self {
        let coords = [
            frame.left_shoulder,
            frame.right_shoulder,
            frame.right_waist,
            frame.left_waist,
            frame.left_head_bottom,
            frame.left_head_top,
            frame.right_head_bottom,
            frame.right_head_top,
        ]
        .into_iter()
        .map(point_from_coordinate)
        .map(|point| camera.project(point))
        .collect::<Vec<_>>();
        Self {
            kind: RenderObjectKind::Body { juggler },
            bounds: Bounds::from_points(&coords, 2.0),
            coords,
            covering: Vec::new(),
        }
    }

    fn line(juggler: usize, start: Coordinate, end: Coordinate, camera: &RenderCamera) -> Self {
        let coords = [start, end]
            .into_iter()
            .map(point_from_coordinate)
            .map(|point| camera.project(point))
            .collect::<Vec<_>>();
        Self {
            kind: RenderObjectKind::Line { juggler },
            bounds: Bounds::from_points(&coords, 4.0),
            coords,
            covering: Vec::new(),
        }
    }

    fn trail(start: Coordinate, end: Coordinate, alpha: f64, camera: &RenderCamera) -> Self {
        let coords = [start, end]
            .into_iter()
            .map(point_from_coordinate)
            .map(|point| camera.project(point))
            .collect::<Vec<_>>();
        Self {
            kind: RenderObjectKind::Trail { alpha },
            bounds: Bounds::from_points(&coords, 2.0),
            coords,
            covering: Vec::new(),
        }
    }

    fn is_covering(&self, other: &RenderObject) -> bool {
        if !self.bounds.overlaps(other.bounds) {
            return false;
        }

        match (&self.kind, &other.kind) {
            (RenderObjectKind::Prop { .. }, RenderObjectKind::Prop { .. }) => {
                self.coords[0].z < other.coords[0].z
            }
            (RenderObjectKind::Prop { .. }, RenderObjectKind::Body { .. }) => {
                plane_depth_at(other, self.coords[0].x, self.coords[0].y)
                    .is_some_and(|depth| self.coords[0].z < depth)
            }
            (
                RenderObjectKind::Prop { .. },
                RenderObjectKind::Line { .. } | RenderObjectKind::Trail { .. },
            ) => box_covering_line(self, other) == 1,
            (RenderObjectKind::Body { .. }, RenderObjectKind::Prop { .. }) => {
                plane_depth_at(self, other.coords[0].x, other.coords[0].y)
                    .is_some_and(|depth| depth < other.coords[0].z)
            }
            (RenderObjectKind::Body { .. }, RenderObjectKind::Body { .. }) => {
                self.coords
                    .iter()
                    .zip(other.coords.iter())
                    .take(4)
                    .map(|(left, right)| left.z - right.z)
                    .sum::<f64>()
                    < 0.0
            }
            (
                RenderObjectKind::Body { .. },
                RenderObjectKind::Line { .. } | RenderObjectKind::Trail { .. },
            ) => box_covering_line(self, other) == 1,
            (
                RenderObjectKind::Line { .. } | RenderObjectKind::Trail { .. },
                RenderObjectKind::Prop { .. } | RenderObjectKind::Body { .. },
            ) => box_covering_line(other, self) == -1,
            (
                RenderObjectKind::Line { .. } | RenderObjectKind::Trail { .. },
                RenderObjectKind::Line { .. } | RenderObjectKind::Trail { .. },
            ) => false,
        }
    }
}

impl Bounds {
    fn from_points(points: &[ScreenPoint], padding: f64) -> Self {
        let mut bounds = Self {
            left: f64::INFINITY,
            top: f64::INFINITY,
            right: f64::NEG_INFINITY,
            bottom: f64::NEG_INFINITY,
        };
        for point in points {
            bounds.left = bounds.left.min(point.x);
            bounds.top = bounds.top.min(point.y);
            bounds.right = bounds.right.max(point.x);
            bounds.bottom = bounds.bottom.max(point.y);
        }
        Self {
            left: bounds.left - padding,
            top: bounds.top - padding,
            right: bounds.right + padding,
            bottom: bounds.bottom + padding,
        }
    }

    fn overlaps(self, other: Bounds) -> bool {
        self.right > other.left
            && self.left < other.right
            && self.bottom > other.top
            && self.top < other.bottom
    }

    fn contains(self, x: f64, y: f64) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

pub fn start(canvas: HtmlCanvasElement, spec: AnimationSpec, settings: RenderSettings) {
    stop();

    let Some(win) = window() else {
        return;
    };
    let Some(ctx) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|ctx| ctx.dyn_into::<CanvasRenderingContext2d>().ok())
    else {
        return;
    };

    let spec_key = playback_spec_key(&spec);
    let now_ms = js_sys::Date::now();
    let current_time = PLAYBACK_CLOCK.with(|slot| {
        let mut clock = slot.borrow_mut();
        if clock.spec_key.as_deref() != Some(spec_key.as_str()) {
            clock.spec_key = Some(spec_key.clone());
            clock.time = 0.0;
        }
        clock.last_wall_ms = now_ms;
        clock.time
    });
    draw(&canvas, &ctx, &spec, &settings, current_time);
    if settings.paused {
        return;
    }

    let closure = Closure::wrap(Box::new(move || {
        let now_ms = js_sys::Date::now();
        let time = PLAYBACK_CLOCK.with(|slot| {
            let mut clock = slot.borrow_mut();
            if clock.spec_key.as_deref() != Some(spec_key.as_str()) {
                clock.spec_key = Some(spec_key.clone());
                clock.time = 0.0;
            }
            let delta = ((now_ms - clock.last_wall_ms) / 1000.0).clamp(0.0, 0.25);
            clock.last_wall_ms = now_ms;
            clock.time += delta * settings.speed.max(0.05);
            clock.time
        });
        draw(&canvas, &ctx, &spec, &settings, time);
    }) as Box<dyn FnMut()>);

    if let Ok(interval_id) = win.set_interval_with_callback_and_timeout_and_arguments_0(
        closure.as_ref().unchecked_ref(),
        16,
    ) {
        ANIMATOR.with(|slot| {
            *slot.borrow_mut() = Some(CanvasAnimator {
                interval_id,
                _closure: closure,
            });
        });
    }
}

fn playback_spec_key(spec: &AnimationSpec) -> String {
    format!(
        "{}|{}|{}|{:.6}",
        spec.source_label, spec.title, spec.ball_count, spec.period_secs
    )
}

pub fn start_by_id(canvas_id: &str, spec: AnimationSpec, settings: RenderSettings) {
    let Some(canvas) = window()
        .and_then(|win| win.document())
        .and_then(|document| document.get_element_by_id(canvas_id))
        .and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok())
    else {
        return;
    };

    start(canvas, spec, settings);
}

pub fn playback_time(spec: &AnimationSpec) -> f64 {
    let spec_key = playback_spec_key(spec);
    PLAYBACK_CLOCK.with(|slot| {
        let clock = slot.borrow();
        if clock.spec_key.as_deref() == Some(spec_key.as_str()) {
            clock.time
        } else {
            0.0
        }
    })
}

pub fn set_playback_time(spec: &AnimationSpec, time: f64) {
    let spec_key = playback_spec_key(spec);
    PLAYBACK_CLOCK.with(|slot| {
        let mut clock = slot.borrow_mut();
        clock.spec_key = Some(spec_key);
        clock.time = time.max(0.0);
        clock.last_wall_ms = js_sys::Date::now();
    });
}

pub fn hit_test_by_id(canvas_id: &str, client_x: f64, client_y: f64) -> Option<String> {
    let canvas = window()
        .and_then(|win| win.document())
        .and_then(|document| document.get_element_by_id(canvas_id))
        .and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok())?;
    let rect = canvas.get_bounding_client_rect();
    let x = client_x - rect.left();
    let y = client_y - rect.top();

    LAST_HITS.with(|hits| {
        hits.borrow()
            .iter()
            .rev()
            .find(|hit| hit.contains(x, y))
            .map(|hit| hit.label.clone())
    })
}

pub fn position_editor_hit_by_id(
    canvas_id: &str,
    client_x: f64,
    client_y: f64,
) -> Option<PositionEditorHit> {
    let canvas = window()
        .and_then(|win| win.document())
        .and_then(|document| document.get_element_by_id(canvas_id))
        .and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok())?;
    let rect = canvas.get_bounding_client_rect();
    let x = client_x - rect.left();
    let y = client_y - rect.top();
    LAST_POSITION_HITS.with(|hits| {
        hits.borrow()
            .iter()
            .rev()
            .find(|item| item.shape.contains(x, y))
            .map(|item| item.hit.clone())
    })
}

pub fn event_editor_hit_by_id(
    canvas_id: &str,
    client_x: f64,
    client_y: f64,
) -> Option<EventEditorHit> {
    let canvas = window()
        .and_then(|win| win.document())
        .and_then(|document| document.get_element_by_id(canvas_id))
        .and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok())?;
    let rect = canvas.get_bounding_client_rect();
    let x = client_x - rect.left();
    let y = client_y - rect.top();
    LAST_EVENT_HITS.with(|hits| {
        hits.borrow()
            .iter()
            .rev()
            .find(|item| item.shape.contains(x, y))
            .map(|item| item.hit.clone())
    })
}

pub fn stop() {
    ANIMATOR.with(|slot| {
        if let Some(animator) = slot.borrow_mut().take() {
            if let Some(win) = window() {
                win.clear_interval_with_handle(animator.interval_id);
            }
        }
    });
}

fn draw(
    canvas: &HtmlCanvasElement,
    ctx: &CanvasRenderingContext2d,
    spec: &AnimationSpec,
    settings: &RenderSettings,
    time: f64,
) {
    let (width, height, dpr) = resize_canvas(canvas);
    ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0).ok();

    let palette = Palette::for_theme(&settings.theme);
    LAST_HITS.with(|hits| hits.borrow_mut().clear());
    LAST_EVENT_HITS.with(|hits| hits.borrow_mut().clear());
    LAST_POSITION_HITS.with(|hits| hits.borrow_mut().clear());
    ctx.set_fill_style_str(palette.background);
    ctx.fill_rect(0.0, 0.0, width, height);

    draw_stage(ctx, width, height, &palette);
    match &spec.kind {
        AnimationKind::Jml(jml) => {
            if jml.layout.is_some() {
                draw_jml_layout_scene(ctx, width, height, jml, settings, &palette, time);
            } else {
                draw_unavailable_message(
                    ctx,
                    width,
                    height,
                    "This JML pattern did not produce a physical layout.",
                    &palette,
                );
            }
        }
        AnimationKind::Unavailable(message) => {
            draw_unavailable_message(ctx, width, height, message, &palette);
        }
    }

    draw_hud(ctx, spec, width, &palette);
    draw_axes(ctx, settings, &palette);
}

fn resize_canvas(canvas: &HtmlCanvasElement) -> (f64, f64, f64) {
    let rect = canvas.get_bounding_client_rect();
    let width = rect.width().max(320.0);
    let height = rect.height().max(240.0);
    let dpr = window()
        .map(|win| win.device_pixel_ratio())
        .unwrap_or(1.0)
        .clamp(1.0, 2.5);
    let pixel_width = (width * dpr).round() as u32;
    let pixel_height = (height * dpr).round() as u32;
    if canvas.width() != pixel_width {
        canvas.set_width(pixel_width);
    }
    if canvas.height() != pixel_height {
        canvas.set_height(pixel_height);
    }
    (width, height, dpr)
}

fn draw_stage(ctx: &CanvasRenderingContext2d, width: f64, height: f64, palette: &Palette) {
    let gradient = ctx.create_linear_gradient(0.0, 0.0, width, height);
    gradient.add_color_stop(0.0, palette.background).ok();
    gradient.add_color_stop(1.0, palette.background_alt).ok();
    #[allow(deprecated)]
    ctx.set_fill_style(&gradient);
    ctx.fill_rect(0.0, 0.0, width, height);
}

fn draw_jml_layout_scene(
    ctx: &CanvasRenderingContext2d,
    width: f64,
    height: f64,
    jml: &JmlAnimation,
    settings: &RenderSettings,
    palette: &Palette,
    time: f64,
) {
    let Some(layout) = &jml.layout else {
        return;
    };
    let t = time.rem_euclid(jml.period_secs);
    let mut objects = Vec::new();
    let camera = RenderCamera::for_layout(
        width,
        height,
        settings,
        layout.overall_bounds(),
        jml.jugglers,
    );

    for jug in 1..=jml.jugglers {
        if let Ok(frame) = layout.juggler_frame(jug, t) {
            push_juggler_render_objects(&mut objects, &frame, jug, &camera);
            push_juggler_frame_hits(&frame, jug, &camera);
        }
    }

    for path in 1..=layout.number_of_paths {
        if let Ok(coord) = layout.path_coordinate(path, t) {
            let point = point_from_coordinate(coord);
            if settings.show_trails {
                if let Ok(trail) = layout.path_trail_coordinates(path, t, 0.32, 18) {
                    push_trail_render_objects(&mut objects, &camera, &trail);
                }
            }
            let prop = jml
                .prop_for_path_at_time(path, time)
                .cloned()
                .unwrap_or_else(|| PropSpec::default_for_type("ball"));
            objects.push(RenderObject::prop(point, path, prop, &camera));
        }
    }

    if settings.show_grid {
        push_ground_render_objects(&mut objects, jml, &camera);
    }

    if settings.selected_position.is_some() && camera.pitch < 70.0_f64.to_radians() {
        draw_position_grid(ctx, &camera, width, height, palette);
    }

    for index in sorted_render_order(&mut objects) {
        draw_render_object(ctx, &objects[index], palette);
    }
    if let Some(selection) = settings.selected_event {
        draw_event_editor(ctx, &camera, jml, selection, palette);
    }
    if let Some(position_index) = settings.selected_position {
        draw_position_editor(
            ctx,
            &camera,
            jml,
            position_index,
            settings.position_edit_handle,
            palette,
        );
    }
}

fn draw_event_editor(
    ctx: &CanvasRenderingContext2d,
    camera: &RenderCamera,
    jml: &JmlAnimation,
    selection: EventSelection,
    palette: &Palette,
) {
    const EVENT_BOX_HALF_CM: f64 = 5.0;
    const NEIGHBOR_BOX_HALF_CM: f64 = 2.0;
    const Y_HANDLE_CM: f64 = 10.0;
    const HAND_PATH_STEP_SECS: f64 = 0.005;
    let Some(layout) = &jml.layout else {
        return;
    };
    let Some(selected_index) = selected_layout_event_index(layout, selection, jml.period_secs)
    else {
        return;
    };
    let selected = &layout.events[selected_index];
    let juggler = selected.event.juggler;
    let hand = selected.event.hand;
    let mut visible_indices = vec![selected_index];
    let mut hand_path_start = selected.event.t;
    let mut hand_path_end = selected.event.t;

    for (index, candidate) in layout.events.iter().enumerate().skip(selected_index + 1) {
        if candidate.event.juggler != juggler || candidate.event.hand != hand {
            continue;
        }
        hand_path_end = hand_path_end.max(candidate.event.t);
        if candidate.primary_index == selected.primary_index {
            break;
        }
        visible_indices.push(index);
        if event_has_throw_or_catch(candidate) {
            break;
        }
    }
    for (index, candidate) in layout.events[..selected_index].iter().enumerate().rev() {
        if candidate.event.juggler != juggler || candidate.event.hand != hand {
            continue;
        }
        hand_path_start = hand_path_start.min(candidate.event.t);
        if candidate.primary_index == selected.primary_index {
            break;
        }
        visible_indices.push(index);
        if event_has_throw_or_catch(candidate) {
            break;
        }
    }

    draw_hand_path_overlay(
        ctx,
        camera,
        layout,
        juggler,
        hand,
        hand_path_start,
        hand_path_end,
        HAND_PATH_STEP_SECS,
        palette,
    );

    for index in visible_indices
        .iter()
        .copied()
        .filter(|index| *index != selected_index)
    {
        let event = &layout.events[index];
        let center_world = point_from_coordinate(event.global_coordinate);
        let angle = layout
            .juggler_angle(event.event.juggler, event.event.t)
            .unwrap_or(0.0)
            .to_radians();
        let (axis_x, _, axis_z) = event_projected_axes(*camera, center_world, angle);
        draw_event_xz_box(
            ctx,
            camera.project(center_world),
            axis_x,
            axis_z,
            NEIGHBOR_BOX_HALF_CM,
            palette,
        );
    }

    let center_world = point_from_coordinate(selected.global_coordinate);
    let center = camera.project(center_world);
    let angle = layout
        .juggler_angle(selected.event.juggler, selected.event.t)
        .unwrap_or(0.0)
        .to_radians();
    let (axis_x, axis_y, axis_z) = event_projected_axes(*camera, center_world, angle);
    let xz_area = (axis_x.0 * axis_z.1 - axis_x.1 * axis_z.0).abs();
    let show_xz = xz_area > 0.015;
    let show_y = axis_y.0.hypot(axis_y.1) > 0.08;
    let geometry = EventEditorHit {
        primary_index: selection.primary_index,
        event_time: selected.event.t,
        image_hand: selected.event.hand,
        handle: EventEditHandle::Xz,
        local_x_dx: axis_x.0,
        local_x_dy: axis_x.1,
        local_y_dx: axis_y.0,
        local_y_dy: axis_y.1,
        z_dx: axis_z.0,
        z_dy: axis_z.1,
    };

    ctx.set_fill_style_str(palette.highlight);
    ctx.begin_path();
    ctx.arc(center.x, center.y, 2.0, 0.0, std::f64::consts::TAU)
        .ok();
    ctx.fill();

    if show_xz {
        let corners = event_xz_corners(center, axis_x, axis_z, EVENT_BOX_HALF_CM);
        draw_event_xz_box(ctx, center, axis_x, axis_z, EVENT_BOX_HALF_CM, palette);
        LAST_EVENT_HITS.with(|hits| {
            hits.borrow_mut().push(EventEditorHitObject {
                hit: geometry.clone(),
                shape: HitShape::Polygon(corners.to_vec()),
            });
        });
    }

    if show_y {
        let front = (
            center.x + axis_y.0 * Y_HANDLE_CM,
            center.y + axis_y.1 * Y_HANDLE_CM,
        );
        let back = (
            center.x - axis_y.0 * Y_HANDLE_CM,
            center.y - axis_y.1 * Y_HANDLE_CM,
        );
        ctx.set_stroke_style_str(palette.highlight);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(front.0, front.1);
        ctx.line_to(back.0, back.1);
        ctx.stroke();
        draw_event_axis_arrow(ctx, front, axis_y, true);
        draw_event_axis_arrow(ctx, back, axis_y, false);
        let mut hit = geometry;
        hit.handle = EventEditHandle::Y;
        LAST_EVENT_HITS.with(|hits| {
            hits.borrow_mut().push(EventEditorHitObject {
                hit,
                shape: HitShape::Segment {
                    x1: front.0,
                    y1: front.1,
                    x2: back.0,
                    y2: back.1,
                    radius: 6.0,
                },
            });
        });
    }
}

fn selected_layout_event_index(
    layout: &LaidoutPattern,
    selection: EventSelection,
    period_secs: f64,
) -> Option<usize> {
    layout
        .events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.primary_index == selection.primary_index)
        .min_by(|(_, left), (_, right)| {
            cyclic_time_distance(left.event.t, selection.time, period_secs)
                .total_cmp(&cyclic_time_distance(
                    right.event.t,
                    selection.time,
                    period_secs,
                ))
                .then_with(|| left.event.t.total_cmp(&right.event.t))
        })
        .map(|(index, _)| index)
}

fn cyclic_time_distance(left: f64, right: f64, period: f64) -> f64 {
    if period <= 0.0 {
        return (left - right).abs();
    }
    let delta = (left - right).rem_euclid(period);
    delta.min(period - delta)
}

fn event_has_throw_or_catch(event: &LayoutEvent) -> bool {
    event
        .event
        .transitions
        .iter()
        .any(|transition| transition.transition_type.is_throw_or_catch())
}

fn event_projected_axes(
    camera: RenderCamera,
    center: Point3,
    juggler_angle: f64,
) -> ((f64, f64), (f64, f64), (f64, f64)) {
    let local_x = Point3 {
        x: juggler_angle.cos(),
        y: juggler_angle.sin(),
        z: 0.0,
    };
    let local_y = Point3 {
        x: -juggler_angle.sin(),
        y: juggler_angle.cos(),
        z: 0.0,
    };
    let world_z = Point3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    (
        projected_axis(camera, center, local_x),
        projected_axis(camera, center, local_y),
        projected_axis(camera, center, world_z),
    )
}

fn event_xz_corners(
    center: ScreenPoint,
    axis_x: (f64, f64),
    axis_z: (f64, f64),
    half_size: f64,
) -> [(f64, f64); 4] {
    [
        screen_offset(center, axis_x, axis_z, -half_size, -half_size),
        screen_offset(center, axis_x, axis_z, -half_size, half_size),
        screen_offset(center, axis_x, axis_z, half_size, half_size),
        screen_offset(center, axis_x, axis_z, half_size, -half_size),
    ]
}

fn draw_event_xz_box(
    ctx: &CanvasRenderingContext2d,
    center: ScreenPoint,
    axis_x: (f64, f64),
    axis_z: (f64, f64),
    half_size: f64,
    palette: &Palette,
) {
    let corners = event_xz_corners(center, axis_x, axis_z, half_size);
    ctx.set_stroke_style_str(palette.highlight);
    ctx.set_line_width(1.25);
    ctx.begin_path();
    ctx.move_to(corners[0].0, corners[0].1);
    for point in &corners[1..] {
        ctx.line_to(point.0, point.1);
    }
    ctx.close_path();
    ctx.stroke();
}

fn draw_event_axis_arrow(
    ctx: &CanvasRenderingContext2d,
    endpoint: (f64, f64),
    axis: (f64, f64),
    forward: bool,
) {
    let length = axis.0.hypot(axis.1);
    if length <= 1e-9 {
        return;
    }
    let direction = if forward { 1.0 } else { -1.0 };
    let ux = axis.0 / length * direction;
    let uy = axis.1 / length * direction;
    let px = -uy;
    let py = ux;
    ctx.begin_path();
    ctx.move_to(endpoint.0, endpoint.1);
    ctx.line_to(
        endpoint.0 - ux * 5.0 + px * 2.5,
        endpoint.1 - uy * 5.0 + py * 2.5,
    );
    ctx.move_to(endpoint.0, endpoint.1);
    ctx.line_to(
        endpoint.0 - ux * 5.0 - px * 2.5,
        endpoint.1 - uy * 5.0 - py * 2.5,
    );
    ctx.stroke();
}

#[allow(clippy::too_many_arguments)]
fn draw_hand_path_overlay(
    ctx: &CanvasRenderingContext2d,
    camera: &RenderCamera,
    layout: &LaidoutPattern,
    juggler: usize,
    hand: usize,
    start_time: f64,
    end_time: f64,
    requested_step: f64,
    palette: &Palette,
) {
    let duration = (end_time - start_time).max(0.0);
    if duration <= 1e-9 {
        return;
    }
    let sample_count = (duration / requested_step).ceil().clamp(1.0, 2000.0) as usize;
    let mut previous = layout
        .hand_coordinate(juggler, hand, start_time)
        .ok()
        .map(point_from_coordinate)
        .map(|point| camera.project(point));
    ctx.set_stroke_style_str(palette.trail);
    ctx.set_line_width(1.25);
    let mut dash_phase = 0.0;
    for sample in 1..=sample_count {
        let time = start_time + duration * sample as f64 / sample_count as f64;
        let Some(current) = layout
            .hand_coordinate(juggler, hand, time)
            .ok()
            .map(point_from_coordinate)
            .map(|point| camera.project(point))
        else {
            previous = None;
            continue;
        };
        if let Some(previous) = previous {
            if layout.is_hand_holding(juggler, hand, time + 0.0001) {
                ctx.begin_path();
                ctx.move_to(previous.x, previous.y);
                ctx.line_to(current.x, current.y);
                ctx.stroke();
            } else {
                draw_dashed_segment(ctx, previous, current, 5.7, 5.0, &mut dash_phase);
            }
        }
        previous = Some(current);
    }
}

fn draw_dashed_segment(
    ctx: &CanvasRenderingContext2d,
    start: ScreenPoint,
    end: ScreenPoint,
    dash: f64,
    gap: f64,
    phase: &mut f64,
) {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = dx.hypot(dy);
    if length <= 1e-9 {
        return;
    }
    let ux = dx / length;
    let uy = dy / length;
    let cycle = dash + gap;
    let mut offset = 0.0;
    while offset < length {
        let in_dash = *phase < dash;
        let boundary = if in_dash { dash } else { cycle };
        let step = (boundary - *phase).min(length - offset);
        if in_dash {
            ctx.begin_path();
            ctx.move_to(start.x + ux * offset, start.y + uy * offset);
            ctx.line_to(
                start.x + ux * (offset + step),
                start.y + uy * (offset + step),
            );
            ctx.stroke();
        }
        offset += step;
        *phase = (*phase + step).rem_euclid(cycle);
    }
}

fn draw_position_grid(
    ctx: &CanvasRenderingContext2d,
    camera: &RenderCamera,
    width: f64,
    height: f64,
    palette: &Palette,
) {
    const GRID_SPACING_CM: f64 = 20.0;
    const MAX_GRID_LINES: i32 = 240;
    let origin = camera.project(Point3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    });
    let x_step_point = camera.project(Point3 {
        x: GRID_SPACING_CM,
        y: 0.0,
        z: 0.0,
    });
    let y_step_point = camera.project(Point3 {
        x: 0.0,
        y: GRID_SPACING_CM,
        z: 0.0,
    });
    let axis_x = (x_step_point.x - origin.x, x_step_point.y - origin.y);
    let axis_y = (y_step_point.x - origin.x, y_step_point.y - origin.y);
    let determinant = axis_x.0 * axis_y.1 - axis_x.1 * axis_y.0;
    if determinant.abs() <= 1e-6 {
        return;
    }

    let mut m_min = i32::MAX;
    let mut m_max = i32::MIN;
    let mut n_min = i32::MAX;
    let mut n_max = i32::MIN;
    for (x, y) in [(0.0, 0.0), (width, 0.0), (0.0, height), (width, height)] {
        let Some((m, n)) = solve_screen_vectors(
            x - origin.x,
            y - origin.y,
            axis_x.0,
            axis_x.1,
            axis_y.0,
            axis_y.1,
        ) else {
            return;
        };
        m_min = m_min.min(m.floor() as i32);
        m_max = m_max.max(m.ceil() as i32);
        n_min = n_min.min(n.floor() as i32);
        n_max = n_max.max(n.ceil() as i32);
    }
    limit_grid_range(&mut m_min, &mut m_max, MAX_GRID_LINES);
    limit_grid_range(&mut n_min, &mut n_max, MAX_GRID_LINES);

    ctx.set_stroke_style_str(palette.trail);
    ctx.set_global_alpha(0.48);
    for m in m_min..=m_max {
        let start = (
            origin.x + m as f64 * axis_x.0 + n_min as f64 * axis_y.0,
            origin.y + m as f64 * axis_x.1 + n_min as f64 * axis_y.1,
        );
        let end = (
            origin.x + m as f64 * axis_x.0 + n_max as f64 * axis_y.0,
            origin.y + m as f64 * axis_x.1 + n_max as f64 * axis_y.1,
        );
        ctx.set_line_width(if m == 0 { 2.0 } else { 0.75 });
        ctx.begin_path();
        ctx.move_to(start.0, start.1);
        ctx.line_to(end.0, end.1);
        ctx.stroke();
    }
    for n in n_min..=n_max {
        let start = (
            origin.x + m_min as f64 * axis_x.0 + n as f64 * axis_y.0,
            origin.y + m_min as f64 * axis_x.1 + n as f64 * axis_y.1,
        );
        let end = (
            origin.x + m_max as f64 * axis_x.0 + n as f64 * axis_y.0,
            origin.y + m_max as f64 * axis_x.1 + n as f64 * axis_y.1,
        );
        ctx.set_line_width(if n == 0 { 2.0 } else { 0.75 });
        ctx.begin_path();
        ctx.move_to(start.0, start.1);
        ctx.line_to(end.0, end.1);
        ctx.stroke();
    }
    ctx.set_global_alpha(1.0);
}

fn solve_screen_vectors(
    dx: f64,
    dy: f64,
    ax: f64,
    ay: f64,
    bx: f64,
    by: f64,
) -> Option<(f64, f64)> {
    let determinant = ax * by - ay * bx;
    (determinant.abs() > 1e-9).then(|| {
        (
            (by * dx - bx * dy) / determinant,
            (-ay * dx + ax * dy) / determinant,
        )
    })
}

fn limit_grid_range(minimum: &mut i32, maximum: &mut i32, limit: i32) {
    if *maximum - *minimum <= limit {
        return;
    }
    let midpoint = (*minimum as i64 + *maximum as i64) / 2;
    *minimum = (midpoint - (limit / 2) as i64) as i32;
    *maximum = *minimum + limit;
}

fn draw_position_editor(
    ctx: &CanvasRenderingContext2d,
    camera: &RenderCamera,
    jml: &JmlAnimation,
    position_index: usize,
    active_handle: Option<PositionEditHandle>,
    palette: &Palette,
) {
    const BOX_HALF_CM: f64 = 10.0;
    const BOX_HIT_HALF_CM: f64 = 15.0;
    const Z_HANDLE_CM: f64 = 20.0;
    const ANGLE_HANDLE_CM: f64 = 20.0;
    let Some(position) = jml.positions.get(position_index) else {
        return;
    };
    let angle = position.angle.to_radians();
    let center_world = Point3 {
        x: position.x,
        y: position.y,
        z: position.z,
    };
    let local_x = Point3 {
        x: angle.cos(),
        y: angle.sin(),
        z: 0.0,
    };
    let local_y = Point3 {
        x: -angle.sin(),
        y: angle.cos(),
        z: 0.0,
    };
    let world_z = Point3 {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    let center = camera.project(center_world);
    let axis_x = projected_axis(*camera, center_world, local_x);
    let axis_y = projected_axis(*camera, center_world, local_y);
    let axis_z = projected_axis(*camera, center_world, world_z);
    let pitch_diff = angle_difference(camera.pitch - std::f64::consts::FRAC_PI_2);
    let show_xy = pitch_diff > 20.0_f64.to_radians();
    let show_angle = show_xy;
    let show_z = pitch_diff < 60.0_f64.to_radians();
    let dragging = active_handle.is_some();
    let draw_xy = show_xy || dragging;
    let draw_z = show_z && (!dragging || active_handle == Some(PositionEditHandle::Z));
    let draw_angle = show_angle && (!dragging || active_handle == Some(PositionEditHandle::Angle));
    let geometry = PositionEditorHit {
        position_index,
        handle: PositionEditHandle::Xy,
        center_x: center.x,
        center_y: center.y,
        local_x_dx: axis_x.0,
        local_x_dy: axis_x.1,
        local_y_dx: axis_y.0,
        local_y_dy: axis_y.1,
        z_dx: axis_z.0,
        z_dy: axis_z.1,
    };

    ctx.set_stroke_style_str(palette.highlight);
    ctx.set_fill_style_str(palette.highlight);
    ctx.set_line_width(1.25);
    ctx.begin_path();
    ctx.arc(center.x, center.y, 3.0, 0.0, std::f64::consts::TAU)
        .ok();
    ctx.fill();

    let mut xy_hit_shape = None;
    if draw_xy {
        let corners = [
            screen_offset(center, axis_x, axis_y, -BOX_HALF_CM, -BOX_HALF_CM),
            screen_offset(center, axis_x, axis_y, -BOX_HALF_CM, BOX_HALF_CM),
            screen_offset(center, axis_x, axis_y, BOX_HALF_CM, BOX_HALF_CM),
            screen_offset(center, axis_x, axis_y, BOX_HALF_CM, -BOX_HALF_CM),
        ];
        ctx.begin_path();
        ctx.move_to(corners[0].0, corners[0].1);
        for point in &corners[1..] {
            ctx.line_to(point.0, point.1);
        }
        ctx.close_path();
        ctx.stroke();
        if show_xy {
            let hit_corners = [
                screen_offset(center, axis_x, axis_y, -BOX_HIT_HALF_CM, -BOX_HIT_HALF_CM),
                screen_offset(center, axis_x, axis_y, -BOX_HIT_HALF_CM, BOX_HIT_HALF_CM),
                screen_offset(center, axis_x, axis_y, BOX_HIT_HALF_CM, BOX_HIT_HALF_CM),
                screen_offset(center, axis_x, axis_y, BOX_HIT_HALF_CM, -BOX_HIT_HALF_CM),
            ];
            xy_hit_shape = Some(HitShape::Polygon(hit_corners.to_vec()));
        }
    }

    if draw_z {
        let z_end = (
            center.x + axis_z.0 * Z_HANDLE_CM,
            center.y + axis_z.1 * Z_HANDLE_CM,
        );
        ctx.begin_path();
        ctx.move_to(center.x, center.y);
        ctx.line_to(z_end.0, z_end.1);
        ctx.stroke();
        draw_editor_handle(ctx, z_end.0, z_end.1, 4.0);
        let mut hit = geometry.clone();
        hit.handle = PositionEditHandle::Z;
        LAST_POSITION_HITS.with(|hits| {
            hits.borrow_mut().push(PositionEditorHitObject {
                hit,
                shape: HitShape::Segment {
                    x1: center.x,
                    y1: center.y,
                    x2: z_end.0,
                    y2: z_end.1,
                    radius: 6.0,
                },
            });
        });
    }

    if draw_angle {
        let angle_end = (
            center.x - axis_y.0 * ANGLE_HANDLE_CM,
            center.y - axis_y.1 * ANGLE_HANDLE_CM,
        );
        ctx.begin_path();
        ctx.move_to(center.x, center.y);
        ctx.line_to(angle_end.0, angle_end.1);
        ctx.stroke();
        draw_editor_handle(ctx, angle_end.0, angle_end.1, 5.0);
        let mut hit = geometry.clone();
        hit.handle = PositionEditHandle::Angle;
        LAST_POSITION_HITS.with(|hits| {
            hits.borrow_mut().push(PositionEditorHitObject {
                hit,
                shape: HitShape::Circle {
                    x: angle_end.0,
                    y: angle_end.1,
                    radius: 8.0,
                },
            });
        });
    }

    if active_handle == Some(PositionEditHandle::Angle) {
        let start = (center.x - axis_y.0 * 250.0, center.y - axis_y.1 * 250.0);
        let end = (center.x + axis_y.0 * 250.0, center.y + axis_y.1 * 250.0);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(start.0, start.1);
        ctx.line_to(end.0, end.1);
        ctx.stroke();
    } else if active_handle == Some(PositionEditHandle::Z) || camera.pitch < 70.0_f64.to_radians() {
        let ground = camera.project(Point3 {
            x: position.x,
            y: position.y,
            z: 0.0,
        });
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(center.x, center.y);
        ctx.line_to(ground.x, ground.y);
        ctx.stroke();
        ctx.begin_path();
        ctx.arc(ground.x, ground.y, 3.5, 0.0, std::f64::consts::TAU)
            .ok();
        ctx.fill();
        if active_handle == Some(PositionEditHandle::Z) {
            ctx.set_fill_style_str(palette.text_muted);
            ctx.set_font("13px system-ui, sans-serif");
            ctx.fill_text(
                &format!("z = {:.1} cm", position.z),
                ground.x + 7.0,
                ground.y + 24.0,
            )
            .ok();
        }
    }

    // Plane movement wins at the shared center; Z remains available on the
    // exposed shaft and arrow head above the plane.
    if let Some(shape) = xy_hit_shape {
        LAST_POSITION_HITS.with(|hits| {
            hits.borrow_mut().push(PositionEditorHitObject {
                hit: geometry,
                shape,
            });
        });
    }
}

fn projected_axis(camera: RenderCamera, center: Point3, axis: Point3) -> (f64, f64) {
    let projected_center = camera.project(center);
    let projected_axis = camera.project(Point3 {
        x: center.x + axis.x,
        y: center.y + axis.y,
        z: center.z + axis.z,
    });
    (
        projected_axis.x - projected_center.x,
        projected_axis.y - projected_center.y,
    )
}

fn screen_offset(
    center: ScreenPoint,
    axis_x: (f64, f64),
    axis_y: (f64, f64),
    x: f64,
    y: f64,
) -> (f64, f64) {
    (
        center.x + axis_x.0 * x + axis_y.0 * y,
        center.y + axis_x.1 * x + axis_y.1 * y,
    )
}

fn draw_editor_handle(ctx: &CanvasRenderingContext2d, x: f64, y: f64, radius: f64) {
    ctx.begin_path();
    ctx.arc(x, y, radius, 0.0, std::f64::consts::TAU).ok();
    ctx.fill();
}

fn angle_difference(mut angle: f64) -> f64 {
    while angle > std::f64::consts::PI {
        angle -= std::f64::consts::TAU;
    }
    while angle <= -std::f64::consts::PI {
        angle += std::f64::consts::TAU;
    }
    angle.abs()
}

fn push_juggler_render_objects(
    objects: &mut Vec<RenderObject>,
    frame: &JugglerFrame,
    juggler: usize,
    camera: &RenderCamera,
) {
    objects.push(RenderObject::body(frame, juggler, camera));
    push_arm_render_objects(
        objects,
        juggler,
        frame.left_shoulder,
        frame.left_elbow,
        frame.left_hand,
        camera,
    );
    push_arm_render_objects(
        objects,
        juggler,
        frame.right_shoulder,
        frame.right_elbow,
        frame.right_hand,
        camera,
    );
}

fn push_arm_render_objects(
    objects: &mut Vec<RenderObject>,
    juggler: usize,
    shoulder: Coordinate,
    elbow: Option<Coordinate>,
    hand: Coordinate,
    camera: &RenderCamera,
) {
    if let Some(elbow) = elbow {
        objects.push(RenderObject::line(juggler, shoulder, elbow, camera));
        objects.push(RenderObject::line(juggler, elbow, hand, camera));
    } else {
        objects.push(RenderObject::line(juggler, shoulder, hand, camera));
    }
}

fn push_ground_render_objects(
    objects: &mut Vec<RenderObject>,
    jml: &JmlAnimation,
    camera: &RenderCamera,
) {
    let prop_min_z = jml.props.iter().map(PropSpec::min_z_cm).fold(0.0, f64::min);

    for index in 0..18 {
        let (start, end) = if index < 9 {
            let x = -50.0 + 100.0 * index as f64 / 8.0;
            (
                Coordinate {
                    x,
                    y: -50.0,
                    z: prop_min_z,
                },
                Coordinate {
                    x,
                    y: 50.0,
                    z: prop_min_z,
                },
            )
        } else {
            let y = -50.0 + 100.0 * (index - 9) as f64 / 8.0;
            (
                Coordinate {
                    x: -50.0,
                    y,
                    z: prop_min_z,
                },
                Coordinate {
                    x: 50.0,
                    y,
                    z: prop_min_z,
                },
            )
        };
        objects.push(RenderObject::line(0, start, end, camera));
    }
}

fn push_juggler_frame_hits(frame: &JugglerFrame, juggler: usize, camera: &RenderCamera) {
    let left_hand = camera.project(point_from_coordinate(frame.left_hand));
    let right_hand = camera.project(point_from_coordinate(frame.right_hand));
    push_hit(
        &format!("Juggler {juggler} left hand"),
        left_hand.x,
        left_hand.y,
        12.0,
    );
    push_hit(
        &format!("Juggler {juggler} right hand"),
        right_hand.x,
        right_hand.y,
        12.0,
    );
}

fn sorted_render_order(objects: &mut [RenderObject]) -> Vec<usize> {
    for object in objects.iter_mut() {
        object.covering.clear();
    }

    for i in 0..objects.len() {
        for j in 0..objects.len() {
            if i != j && objects[i].is_covering(&objects[j]) {
                objects[i].covering.push(j);
            }
        }
    }

    let mut order = Vec::with_capacity(objects.len());
    let mut drawn = vec![false; objects.len()];
    for pass in 0..2 {
        loop {
            let mut changed = false;
            for i in 0..objects.len() {
                if drawn[i] {
                    continue;
                }
                if objects[i].covering.iter().all(|covered| drawn[*covered]) {
                    drawn[i] = true;
                    order.push(i);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        for i in 0..objects.len() {
            if drawn[i] {
                continue;
            }
            if pass == 0
                && !matches!(
                    objects[i].kind,
                    RenderObjectKind::Line { .. } | RenderObjectKind::Trail { .. }
                )
            {
                continue;
            }
            drawn[i] = true;
            order.push(i);
        }
    }
    order
}

fn draw_render_object(ctx: &CanvasRenderingContext2d, object: &RenderObject, palette: &Palette) {
    match &object.kind {
        RenderObjectKind::Prop {
            point,
            path,
            prop,
            metrics,
        } => {
            let _ = point;
            draw_prop_object(ctx, &object.coords, *path, prop, metrics, palette);
        }
        RenderObjectKind::Body { juggler } => {
            draw_body_object(ctx, &object.coords, palette);
            push_rect_hit(
                &format!("Juggler {juggler} body"),
                object.bounds.left,
                object.bounds.top,
                object.bounds.right,
                object.bounds.bottom,
            );
        }
        RenderObjectKind::Line { juggler } => {
            draw_line_object(ctx, &object.coords, *juggler, palette);
            if *juggler > 0 && object.coords.len() >= 2 {
                push_segment_hit(
                    &format!("Juggler {juggler} arm"),
                    object.coords[0].x,
                    object.coords[0].y,
                    object.coords[1].x,
                    object.coords[1].y,
                    7.0,
                );
            }
        }
        RenderObjectKind::Trail { alpha } => {
            if object.coords.len() < 2 {
                return;
            }
            ctx.save();
            ctx.set_stroke_style_str(palette.trail);
            ctx.set_line_width(1.6);
            ctx.set_global_alpha(*alpha);
            ctx.begin_path();
            ctx.move_to(object.coords[0].x, object.coords[0].y);
            ctx.line_to(object.coords[1].x, object.coords[1].y);
            ctx.stroke();
            ctx.restore();
        }
    }
}

fn draw_prop_object(
    ctx: &CanvasRenderingContext2d,
    coords: &[ScreenPoint],
    path: usize,
    prop: &PropSpec,
    metrics: &Prop2DMetrics,
    palette: &Palette,
) {
    let Some(point) = coords.first().copied() else {
        return;
    };
    let color = prop.color.as_deref().unwrap_or_else(|| palette.ball(path));
    let top_left_x = point.x - metrics.grip_x;
    let top_left_y = point.y - metrics.grip_y;

    ctx.save();
    ctx.set_fill_style_str(color);
    match &metrics.shape {
        Prop2DShape::Square => {
            draw_square_prop_shape(ctx, top_left_x, top_left_y, metrics, color, prop.highlight);
        }
        Prop2DShape::Ring { outer, inner } => {
            ctx.begin_path();
            trace_polygon(ctx, top_left_x, top_left_y, outer);
            trace_polygon(ctx, top_left_x, top_left_y, inner);
            ctx.fill_with_canvas_winding_rule(CanvasWindingRule::Evenodd);
        }
        Prop2DShape::Image { source } => {
            draw_image_prop_shape(ctx, top_left_x, top_left_y, metrics, source, palette);
        }
        _ => {
            draw_ball_prop_shape(ctx, top_left_x, top_left_y, metrics, color, prop.highlight);
        }
    }
    ctx.restore();
    let hit_radius = 0.5 * metrics.width.max(metrics.height) + 6.0;
    push_hit(&format!("Prop path {path}"), point.x, point.y, hit_radius);
}

fn draw_ball_prop_shape(
    ctx: &CanvasRenderingContext2d,
    left: f64,
    top: f64,
    metrics: &Prop2DMetrics,
    color: &str,
    highlight: bool,
) {
    if highlight {
        draw_highlight_layers(ctx, left, top, metrics.width, color, true);
        return;
    }

    ctx.set_fill_style_str(color);
    ctx.begin_path();
    ctx.arc(
        left + metrics.width / 2.0,
        top + metrics.height / 2.0,
        0.5 * metrics.width.min(metrics.height),
        0.0,
        std::f64::consts::TAU,
    )
    .ok();
    ctx.fill();
}

fn draw_square_prop_shape(
    ctx: &CanvasRenderingContext2d,
    left: f64,
    top: f64,
    metrics: &Prop2DMetrics,
    color: &str,
    highlight: bool,
) {
    if highlight {
        draw_highlight_layers(ctx, left, top, metrics.width, color, false);
        return;
    }

    ctx.set_fill_style_str(color);
    ctx.fill_rect(left, top, metrics.width, metrics.height);
}

fn draw_image_prop_shape(
    ctx: &CanvasRenderingContext2d,
    left: f64,
    top: f64,
    metrics: &Prop2DMetrics,
    source: &str,
    palette: &Palette,
) {
    if let Some(image) = cached_image(source) {
        if image.complete() && image.natural_width() > 0 {
            ctx.draw_image_with_html_image_element_and_dw_and_dh(
                &image,
                left,
                top,
                metrics.width,
                metrics.height,
            )
            .ok();
            return;
        }
    }

    ctx.save();
    ctx.set_stroke_style_str(palette.figure);
    ctx.set_fill_style_str(palette.background_alt);
    ctx.set_line_width(1.0);
    ctx.fill_rect(left, top, metrics.width, metrics.height);
    ctx.stroke_rect(left, top, metrics.width, metrics.height);
    ctx.restore();
}

fn cached_image(source: &str) -> Option<HtmlImageElement> {
    let url = image_source_url(source);
    IMAGE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(image) = cache.get(&url) {
            return Some(image.clone());
        }
        let image = HtmlImageElement::new().ok()?;
        image.set_src(&url);
        cache.insert(url, image.clone());
        Some(image)
    })
}

fn image_source_url(source: &str) -> String {
    let decoded = source.trim().replace("%3B", ";").replace("%3b", ";");
    let trimmed = decoded.as_str();
    if trimmed.contains('/') || trimmed.starts_with("data:") {
        trimmed.to_string()
    } else {
        format!("./assets/{trimmed}")
    }
}

fn draw_highlight_layers(
    ctx: &CanvasRenderingContext2d,
    left: f64,
    top: f64,
    pixel_size: f64,
    color: &str,
    oval: bool,
) {
    let Some(base) = Rgba::parse(color) else {
        ctx.set_fill_style_str(color);
        if oval {
            ctx.begin_path();
            ctx.arc(
                left + pixel_size / 2.0,
                top + pixel_size / 2.0,
                pixel_size / 2.0,
                0.0,
                std::f64::consts::TAU,
            )
            .ok();
            ctx.fill();
        } else {
            ctx.fill_rect(left, top, pixel_size, pixel_size);
        }
        return;
    };

    let highlight_layers = pixel_size / 1.2;
    if highlight_layers <= 0.0 {
        return;
    }

    let mut current = Rgba {
        red: base.red / 2.5,
        green: base.green / 2.5,
        blue: base.blue / 2.5,
        alpha: base.alpha,
    };
    ctx.set_fill_style_str(&current.to_css());
    draw_highlight_primitive(ctx, left, top, pixel_size, oval);

    for i in 0..highlight_layers.trunc() as usize {
        current.red = (current.red + 1.0 / highlight_layers).min(1.0);
        current.green = (current.green + 1.0 / highlight_layers).min(1.0);
        current.blue = (current.blue + 1.0 / highlight_layers).min(1.0);
        ctx.set_fill_style_str(&current.to_css());

        let i = i as f64;
        let layer_left = left + i / 1.1;
        let layer_top = top + i / 2.5;
        let layer_size = (pixel_size - i * 1.3).max(0.0);
        draw_highlight_primitive(ctx, layer_left, layer_top, layer_size, oval);
    }
}

fn draw_highlight_primitive(
    ctx: &CanvasRenderingContext2d,
    left: f64,
    top: f64,
    size: f64,
    oval: bool,
) {
    if oval {
        ctx.begin_path();
        ctx.arc(
            left + size / 2.0,
            top + size / 2.0,
            size / 2.0,
            0.0,
            std::f64::consts::TAU,
        )
        .ok();
        ctx.fill();
    } else {
        ctx.fill_rect(left, top, size, size);
    }
}

impl Rgba {
    fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if let Some(hex) = trimmed.strip_prefix('#') {
            return Self::parse_hex(hex);
        }
        if let Some(args) = trimmed
            .strip_prefix("rgb(")
            .and_then(|value| value.strip_suffix(')'))
        {
            return Self::parse_rgb_args(args, false);
        }
        if let Some(args) = trimmed
            .strip_prefix("rgba(")
            .and_then(|value| value.strip_suffix(')'))
        {
            return Self::parse_rgb_args(args, true);
        }
        None
    }

    fn parse_hex(hex: &str) -> Option<Self> {
        if hex.len() != 6 {
            return None;
        }
        let red = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let green = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let blue = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(Self::from_255(red, green, blue, 255.0))
    }

    fn parse_rgb_args(args: &str, has_alpha: bool) -> Option<Self> {
        let tokens = args.split(',').map(str::trim).collect::<Vec<_>>();
        if (!has_alpha && tokens.len() != 3) || (has_alpha && tokens.len() != 4) {
            return None;
        }
        let red = tokens[0].parse::<u8>().ok()?;
        let green = tokens[1].parse::<u8>().ok()?;
        let blue = tokens[2].parse::<u8>().ok()?;
        let alpha = if has_alpha {
            tokens[3].parse::<f64>().ok()?.clamp(0.0, 1.0) * 255.0
        } else {
            255.0
        };
        Some(Self::from_255(red, green, blue, alpha))
    }

    fn from_255(red: u8, green: u8, blue: u8, alpha: f64) -> Self {
        Self {
            red: red as f64 / 255.0,
            green: green as f64 / 255.0,
            blue: blue as f64 / 255.0,
            alpha: (alpha / 255.0).clamp(0.0, 1.0),
        }
    }

    fn to_css(self) -> String {
        format!(
            "rgba({},{},{},{:.3})",
            (self.red * 255.0).round().clamp(0.0, 255.0) as u8,
            (self.green * 255.0).round().clamp(0.0, 255.0) as u8,
            (self.blue * 255.0).round().clamp(0.0, 255.0) as u8,
            self.alpha
        )
    }
}

fn draw_body_object(ctx: &CanvasRenderingContext2d, coords: &[ScreenPoint], palette: &Palette) {
    if coords.len() < 8 {
        return;
    }

    ctx.save();
    ctx.set_line_join("round");
    ctx.set_stroke_style_str(palette.figure);
    ctx.set_fill_style_str(palette.background_alt);
    ctx.set_line_width(2.4);
    ctx.set_global_alpha(0.92);

    ctx.begin_path();
    ctx.move_to(coords[0].x, coords[0].y);
    ctx.line_to(coords[1].x, coords[1].y);
    ctx.line_to(coords[2].x, coords[2].y);
    ctx.line_to(coords[3].x, coords[3].y);
    ctx.close_path();
    ctx.fill();
    ctx.stroke();

    ctx.begin_path();
    ctx.move_to(coords[4].x, coords[4].y);
    ctx.line_to(coords[5].x, coords[5].y);
    ctx.line_to(coords[7].x, coords[7].y);
    ctx.line_to(coords[6].x, coords[6].y);
    ctx.close_path();
    ctx.fill();
    ctx.stroke();
    ctx.restore();
}

fn draw_line_object(
    ctx: &CanvasRenderingContext2d,
    coords: &[ScreenPoint],
    juggler: usize,
    palette: &Palette,
) {
    if coords.len() < 2 {
        return;
    }

    ctx.save();
    let line_color = if juggler > 0 {
        palette.figure
    } else {
        palette.grid
    };
    ctx.set_stroke_style_str(line_color);
    ctx.set_fill_style_str(line_color);
    ctx.set_line_width(if juggler > 0 { 4.0 } else { 1.0 });
    ctx.set_line_cap("round");
    ctx.begin_path();
    ctx.move_to(coords[0].x, coords[0].y);
    ctx.line_to(coords[1].x, coords[1].y);
    ctx.stroke();

    if juggler > 0 {
        for point in coords {
            ctx.begin_path();
            ctx.arc(point.x, point.y, 3.2, 0.0, std::f64::consts::TAU)
                .ok();
            ctx.fill();
        }
    }
    ctx.restore();
}

fn push_trail_render_objects(
    objects: &mut Vec<RenderObject>,
    camera: &RenderCamera,
    points: &[Coordinate],
) {
    if points.len() < 2 {
        return;
    }

    let mut previous_world = None::<Point3>;
    let mut previous_coordinate = None::<Coordinate>;
    let mut previous_screen = None::<ScreenPoint>;
    for (index, coordinate) in points.iter().copied().enumerate() {
        let world = point_from_coordinate(coordinate);
        let point = camera.project(world);
        let discontinuous = previous_world
            .is_some_and(|previous| point_distance(previous, world) > TRAIL_MAX_WORLD_STEP_CM)
            || previous_screen.is_some_and(|previous| {
                screen_distance(previous.x, previous.y, point.x, point.y) > TRAIL_MAX_SCREEN_STEP_PX
            });
        if index > 0 && !discontinuous {
            if let Some(previous) = previous_coordinate {
                let alpha = 0.12 + 0.43 * (index as f64 / (points.len() - 1) as f64);
                objects.push(RenderObject::trail(previous, coordinate, alpha, camera));
            }
        }
        previous_world = Some(world);
        previous_coordinate = Some(coordinate);
        previous_screen = Some(point);
    }
}

fn draw_hud(ctx: &CanvasRenderingContext2d, spec: &AnimationSpec, width: f64, palette: &Palette) {
    ctx.save();
    ctx.set_fill_style_str(palette.text_muted);
    ctx.set_font("12px Inter, system-ui, sans-serif");
    let label = format!("{} | {} prop", spec.title, spec.ball_count);
    ctx.fill_text(&label, 88.0, 28.0).ok();
    ctx.set_global_alpha(0.16);
    ctx.set_fill_style_str(palette.figure);
    ctx.fill_rect(width - 104.0, 20.0, 82.0, 2.0);
    ctx.restore();
}

fn draw_unavailable_message(
    ctx: &CanvasRenderingContext2d,
    width: f64,
    height: f64,
    message: &str,
    palette: &Palette,
) {
    ctx.save();
    ctx.set_fill_style_str(palette.highlight);
    ctx.set_font("600 16px Inter, system-ui, sans-serif");
    ctx.set_text_align("center");
    ctx.fill_text("Physical renderer unavailable", width / 2.0, height * 0.44)
        .ok();
    ctx.set_fill_style_str(palette.text_muted);
    ctx.set_font("13px Inter, system-ui, sans-serif");
    ctx.fill_text(message, width / 2.0, height * 0.44 + 28.0)
        .ok();
    ctx.set_text_align("start");
    ctx.restore();
}

fn draw_axes(ctx: &CanvasRenderingContext2d, settings: &RenderSettings, palette: &Palette) {
    let theta = settings.camera_yaw;
    let phi = settings.camera_pitch;
    let axis_len = 30.0;
    let xy_len = axis_len * phi.cos();
    let z_len = axis_len * phi.sin();
    let cx = 38.0;
    let cy = 48.0;
    let xx = cx - axis_len * theta.cos();
    let xy = cy + xy_len * theta.sin();
    let yx = cx + axis_len * theta.sin();
    let yy = cy + xy_len * theta.cos();
    let zx = cx;
    let zy = cy - z_len;

    ctx.save();
    ctx.set_stroke_style_str(palette.highlight);
    ctx.set_fill_style_str(palette.highlight);
    ctx.set_line_width(1.0);
    for (x, y, label) in [(xx, xy, "x"), (yx, yy, "y"), (zx, zy, "z")] {
        ctx.begin_path();
        ctx.move_to(cx, cy);
        ctx.line_to(x, y);
        ctx.stroke();
        ctx.begin_path();
        ctx.arc(x, y, 2.5, 0.0, std::f64::consts::TAU).ok();
        ctx.fill();
        ctx.set_font("12px Inter, system-ui, sans-serif");
        ctx.fill_text(label, x - 3.0, y - 6.0).ok();
    }
    ctx.restore();
}

fn push_hit(label: &str, x: f64, y: f64, radius: f64) {
    LAST_HITS.with(|hits| {
        hits.borrow_mut().push(HitObject {
            label: label.to_string(),
            shape: HitShape::Circle { x, y, radius },
        });
    });
}

fn push_rect_hit(label: &str, left: f64, top: f64, right: f64, bottom: f64) {
    LAST_HITS.with(|hits| {
        hits.borrow_mut().push(HitObject {
            label: label.to_string(),
            shape: HitShape::Rect {
                left,
                top,
                right,
                bottom,
            },
        });
    });
}

fn push_segment_hit(label: &str, x1: f64, y1: f64, x2: f64, y2: f64, radius: f64) {
    LAST_HITS.with(|hits| {
        hits.borrow_mut().push(HitObject {
            label: label.to_string(),
            shape: HitShape::Segment {
                x1,
                y1,
                x2,
                y2,
                radius,
            },
        });
    });
}

fn prop_2d_metrics(prop: &PropSpec, zoom: f64, yaw: f64, pitch: f64) -> Prop2DMetrics {
    match &prop.kind {
        PropKind::Square => square_prop_metrics(prop.diameter, zoom),
        PropKind::Ring => ring_prop_metrics(
            prop.diameter,
            prop.inside_diameter.unwrap_or(prop.diameter * 0.8),
            zoom,
            yaw,
            pitch,
        ),
        PropKind::Ball => ball_prop_metrics(prop.diameter, zoom),
        PropKind::Image => image_prop_metrics(prop, zoom),
        PropKind::Unknown(_) => {
            let mut metrics = ball_prop_metrics(prop.diameter, zoom);
            metrics.shape = Prop2DShape::Fallback;
            metrics
        }
    }
}

fn ball_prop_metrics(diameter: f64, zoom: f64) -> Prop2DMetrics {
    let pixel_size = prop_pixel_size(diameter, zoom);
    Prop2DMetrics {
        width: pixel_size,
        height: pixel_size,
        center_x: pixel_size / 2.0,
        center_y: pixel_size / 2.0,
        grip_x: pixel_size / 2.0,
        grip_y: pixel_size / 2.0,
        shape: Prop2DShape::Ball,
    }
}

fn square_prop_metrics(diameter: f64, zoom: f64) -> Prop2DMetrics {
    let pixel_size = prop_pixel_size(diameter, zoom);
    Prop2DMetrics {
        width: pixel_size,
        height: pixel_size,
        center_x: pixel_size / 2.0,
        center_y: pixel_size / 2.0,
        grip_x: pixel_size / 2.0,
        grip_y: pixel_size / 2.0,
        shape: Prop2DShape::Square,
    }
}

fn image_prop_metrics(prop: &PropSpec, zoom: f64) -> Prop2DMetrics {
    let source = prop
        .image_source
        .clone()
        .unwrap_or_else(|| "ball.png".to_string());
    let width_cm = prop.diameter;
    let height_cm = width_cm
        * loaded_image_aspect_ratio(&source).unwrap_or_else(|| {
            prop.image_aspect_ratio
                .unwrap_or_else(|| default_image_aspect_ratio(&source))
        });
    let pixel_width = prop_pixel_size(width_cm, zoom);
    let pixel_height = prop_pixel_size(height_cm, zoom);
    Prop2DMetrics {
        width: pixel_width,
        height: pixel_height,
        center_x: pixel_width / 2.0,
        center_y: pixel_height / 2.0,
        grip_x: pixel_width / 2.0,
        grip_y: pixel_height,
        shape: Prop2DShape::Image { source },
    }
}

fn loaded_image_aspect_ratio(source: &str) -> Option<f64> {
    let image = cached_image(source)?;
    let width = image.natural_width();
    let height = image.natural_height();
    if image.complete() && width > 0 && height > 0 {
        Some(height as f64 / width as f64)
    } else {
        None
    }
}

fn default_image_aspect_ratio(source: &str) -> f64 {
    match source.trim().rsplit('/').next().unwrap_or(source.trim()) {
        "ball.png" => 1.0,
        _ => 1.0,
    }
}

fn ring_prop_metrics(
    outside_diameter: f64,
    inside_diameter: f64,
    zoom: f64,
    yaw: f64,
    pitch: f64,
) -> Prop2DMetrics {
    let outside_pixel_diam = prop_pixel_size(outside_diameter, zoom);
    let inside_pixel_diam = prop_pixel_size(inside_diameter, zoom);

    let c0 = yaw.cos();
    let s0 = yaw.sin();
    let s1 = pitch.sin();

    let width = 2.0_f64.max((outside_pixel_diam * (s0 * s1).abs()).trunc());
    let height = 2.0_f64.max(outside_pixel_diam);

    let mut inside_width = (inside_pixel_diam * (s0 * s1).abs()).trunc();
    if (inside_width - width).abs() <= f64::EPSILON {
        inside_width -= 2.0;
    }
    inside_width = inside_width.max(0.0);

    let mut inside_height = inside_pixel_diam;
    if (inside_height - height).abs() <= f64::EPSILON {
        inside_height -= 2.0;
    }
    inside_height = inside_height.max(0.0);

    let denom = 1.0 - s0 * s0 * s1 * s1;
    let term1 = if denom > 0.0 {
        (c0 * c0 / denom).sqrt()
    } else {
        f64::INFINITY
    };
    let mut angle = if term1 < 1.0 { term1.acos() } else { 0.0 };
    if c0 * s0 > 0.0 {
        angle = -angle;
    }
    let sa = angle.sin();
    let ca = angle.cos();

    let (outer_raw, pxmin, pxmax, pymin, pymax) = ring_polygon(width, height, ca, sa, None, None);
    let bbwidth = (pxmax - pxmin + 1) as f64;
    let bbheight = (pymax - pymin + 1) as f64;
    let outer = outer_raw
        .into_iter()
        .map(|(x, y)| ((x - pxmin) as f64, (y - pymin) as f64))
        .collect::<Vec<_>>();
    let (inner_raw, _, _, _, _) = ring_polygon(inside_width, inside_height, ca, sa, None, None);
    let inner = inner_raw
        .into_iter()
        .map(|(x, y)| ((x - pxmin) as f64, (y - pymin) as f64))
        .collect::<Vec<_>>();

    let grip_x = if s0 < 0.0 { bbwidth - 1.0 } else { 0.0 };
    let bbw = sa * sa + ca * ca * (s0 * s1).abs();
    let dsq = s0 * s0 * s1 * s1 * ca * ca + sa * sa - bbw * bbw;
    let mut d = if dsq > 0.0 { dsq.sqrt() } else { 0.0 };
    if c0 > 0.0 {
        d = -d;
    }
    let grip_y = (outside_pixel_diam * d).trunc() + bbheight / 2.0;

    Prop2DMetrics {
        width: bbwidth,
        height: bbheight,
        center_x: bbwidth / 2.0,
        center_y: bbheight / 2.0,
        grip_x,
        grip_y,
        shape: Prop2DShape::Ring { outer, inner },
    }
}

fn prop_pixel_size(diameter: f64, zoom: f64) -> f64 {
    (0.5 + zoom * diameter).trunc().max(1.0)
}

fn ring_polygon(
    width: f64,
    height: f64,
    ca: f64,
    sa: f64,
    pxmin_override: Option<i32>,
    pymin_override: Option<i32>,
) -> (Vec<(i32, i32)>, i32, i32, i32, i32) {
    let mut points = Vec::with_capacity(RING_POLYSIDES);
    let mut pxmin = 0;
    let mut pxmax = 0;
    let mut pymin = 0;
    let mut pymax = 0;

    for i in 0..RING_POLYSIDES {
        let theta = i as f64 * std::f64::consts::TAU / RING_POLYSIDES as f64;
        let x = width * theta.cos() * 0.5;
        let y = height * theta.sin() * 0.5;
        let px = original_round(ca * x - sa * y);
        let py = original_round(ca * y + sa * x);
        if i == 0 || px < pxmin {
            pxmin = px;
        }
        if i == 0 || px > pxmax {
            pxmax = px;
        }
        if i == 0 || py < pymin {
            pymin = py;
        }
        if i == 0 || py > pymax {
            pymax = py;
        }
        points.push((px, py));
    }

    (
        points,
        pxmin_override.unwrap_or(pxmin),
        pxmax,
        pymin_override.unwrap_or(pymin),
        pymax,
    )
}

fn original_round(value: f64) -> i32 {
    (value + 0.5) as i32
}

fn trace_polygon(
    ctx: &CanvasRenderingContext2d,
    top_left_x: f64,
    top_left_y: f64,
    points: &[(f64, f64)],
) {
    let Some((first_x, first_y)) = points.first().copied() else {
        return;
    };
    ctx.move_to(top_left_x + first_x, top_left_y + first_y);
    for (x, y) in points.iter().skip(1) {
        ctx.line_to(top_left_x + *x, top_left_y + *y);
    }
    ctx.close_path();
}

fn point_from_coordinate(coordinate: Coordinate) -> Point3 {
    Point3 {
        x: coordinate.x,
        y: coordinate.y,
        z: coordinate.z,
    }
}

fn point_distance(left: Point3, right: Point3) -> f64 {
    let dx = left.x - right.x;
    let dy = left.y - right.y;
    let dz = left.z - right.z;
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn screen_distance(left_x: f64, left_y: f64, right_x: f64, right_y: f64) -> f64 {
    let dx = left_x - right_x;
    let dy = left_y - right_y;
    (dx * dx + dy * dy).sqrt()
}

fn point_segment_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f64::EPSILON {
        return screen_distance(px, py, x1, y1);
    }
    let t = (((px - x1) * dx + (py - y1) * dy) / length_squared).clamp(0.0, 1.0);
    let closest_x = x1 + t * dx;
    let closest_y = y1 + t * dy;
    screen_distance(px, py, closest_x, closest_y)
}

fn point_in_polygon(x: f64, y: f64, points: &[(f64, f64)]) -> bool {
    if points.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut previous = points.len() - 1;
    for current in 0..points.len() {
        let (xi, yi) = points[current];
        let (xj, yj) = points[previous];
        if ((yi > y) != (yj > y)) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        previous = current;
    }
    inside
}

fn plane_depth_at(object: &RenderObject, x: f64, y: f64) -> Option<f64> {
    let normal = box_plane_normal(object)?;
    depth_on_plane(object.coords[0], normal, x, y)
}

fn box_plane_normal(object: &RenderObject) -> Option<ScreenPoint> {
    match object.kind {
        RenderObjectKind::Body { .. } if object.coords.len() >= 3 => Some(vector_product(
            object.coords[0],
            object.coords[1],
            object.coords[2],
        )),
        RenderObjectKind::Prop { .. } => Some(ScreenPoint {
            x: 0.0,
            y: 0.0,
            z: 1.0,
            perspective: 1.0,
        }),
        _ => None,
    }
}

fn vector_product(v1: ScreenPoint, v2: ScreenPoint, v3: ScreenPoint) -> ScreenPoint {
    let ax = v2.x - v1.x;
    let ay = v2.y - v1.y;
    let az = v2.z - v1.z;
    let bx = v3.x - v1.x;
    let by = v3.y - v1.y;
    let bz = v3.z - v1.z;
    ScreenPoint {
        x: ay * bz - by * az,
        y: az * bx - bz * ax,
        z: ax * by - bx * ay,
        perspective: 1.0,
    }
}

fn depth_on_plane(origin: ScreenPoint, normal: ScreenPoint, x: f64, y: f64) -> Option<f64> {
    if normal.z.abs() <= f64::EPSILON {
        return None;
    }
    Some(origin.z - (normal.x * (x - origin.x) + normal.y * (y - origin.y)) / normal.z)
}

fn box_covering_line(box_object: &RenderObject, line_object: &RenderObject) -> i32 {
    if !matches!(
        box_object.kind,
        RenderObjectKind::Body { .. } | RenderObjectKind::Prop { .. }
    ) || !matches!(
        line_object.kind,
        RenderObjectKind::Line { .. } | RenderObjectKind::Trail { .. }
    ) || line_object.coords.len() < 2
    {
        return 0;
    }

    let Some(normal) = box_plane_normal(box_object) else {
        return 0;
    };
    if normal.z.abs() <= f64::EPSILON {
        return 0;
    }

    const SLOP: f64 = 3.0;
    let line0 = line_object.coords[0];
    let line1 = line_object.coords[1];
    let mut end_in_bounds = false;

    for point in [line0, line1] {
        if box_object.bounds.contains(point.x + 0.5, point.y + 0.5) {
            let Some(box_depth) = depth_on_plane(box_object.coords[0], normal, point.x, point.y)
            else {
                return 0;
            };
            if point.z < box_depth - SLOP {
                return -1;
            }
            end_in_bounds = true;
        }
    }
    if end_in_bounds {
        return 1;
    }

    let mut intersects = false;
    for x in [box_object.bounds.left, box_object.bounds.right] {
        if x < line0.x.min(line1.x) || x > line0.x.max(line1.x) {
            continue;
        }
        if (line1.x - line0.x).abs() <= f64::EPSILON {
            continue;
        }
        let y = line0.y + (line1.y - line0.y) * (x - line0.x) / (line1.x - line0.x);
        if y < box_object.bounds.top || y > box_object.bounds.bottom {
            continue;
        }
        intersects = true;
        let Some(box_depth) = depth_on_plane(box_object.coords[0], normal, x, y) else {
            return 0;
        };
        let line_depth = line0.z + (line1.z - line0.z) * (y - line0.y) / (line1.y - line0.y);
        if line_depth < box_depth - SLOP {
            return -1;
        }
    }

    for y in [box_object.bounds.top, box_object.bounds.bottom] {
        if y < line0.y.min(line1.y) || y > line0.y.max(line1.y) {
            continue;
        }
        if (line1.y - line0.y).abs() <= f64::EPSILON {
            continue;
        }
        let x = line0.x + (line1.x - line0.x) * (y - line0.y) / (line1.y - line0.y);
        if x < box_object.bounds.left || x > box_object.bounds.right {
            continue;
        }
        intersects = true;
        let Some(box_depth) = depth_on_plane(box_object.coords[0], normal, x, y) else {
            return 0;
        };
        let line_depth = line0.z + (line1.z - line0.z) * (x - line0.x) / (line1.x - line0.x);
        if line_depth < box_depth - SLOP {
            return -1;
        }
    }

    if intersects { 1 } else { 0 }
}

struct Palette {
    background: &'static str,
    background_alt: &'static str,
    grid: &'static str,
    figure: &'static str,
    trail: &'static str,
    text_muted: &'static str,
    highlight: &'static str,
    balls: &'static [&'static str],
}

impl Palette {
    fn for_theme(theme: &str) -> Self {
        match theme {
            "light" => Self {
                background: "#f7fafc",
                background_alt: "#e8edf1",
                grid: "#aab6c2",
                figure: "#28313d",
                trail: "#5f7a8d",
                text_muted: "#526070",
                highlight: "#0477bf",
                balls: &["#e23b5f", "#0477bf", "#d48a00", "#158463", "#7c54d8"],
            },
            "aurora" => Self {
                background: "#06130f",
                background_alt: "#102019",
                grid: "#315449",
                figure: "#d7fff2",
                trail: "#67d9b7",
                text_muted: "#a3c8bd",
                highlight: "#f2c94c",
                balls: &["#f2c94c", "#56ccf2", "#eb5757", "#9bffcb", "#bb6bd9"],
            },
            "contrast" => Self {
                background: "#080909",
                background_alt: "#151717",
                grid: "#3a3f3f",
                figure: "#f6f6ef",
                trail: "#c7d0d0",
                text_muted: "#aeb5b5",
                highlight: "#ffd166",
                balls: &["#ffd166", "#06d6a0", "#ef476f", "#66c7f4", "#f7f7f2"],
            },
            "atelier" => Self {
                background: "#f2f5f2",
                background_alt: "#e2ebe6",
                grid: "#aebdb5",
                figure: "#1d2a24",
                trail: "#668277",
                text_muted: "#63746b",
                highlight: "#247b68",
                balls: &["#bf3f61", "#247b68", "#d6952d", "#2f73b7", "#6f56a6"],
            },
            _ => Self {
                background: "#0b0f17",
                background_alt: "#151b24",
                grid: "#2b3544",
                figure: "#d8e2ee",
                trail: "#7b91ac",
                text_muted: "#9eacbd",
                highlight: "#41b3ff",
                balls: &["#ff5f7e", "#41b3ff", "#f6c85f", "#69d29b", "#b58cff"],
            },
        }
    }

    fn ball(&self, seed: usize) -> &'static str {
        self.balls[seed % self.balls.len()]
    }
}
