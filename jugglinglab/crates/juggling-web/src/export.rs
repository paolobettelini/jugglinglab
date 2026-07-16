use crate::canvas::{self, RenderSettings};
use color_quant::NeuQuant;
use gif::{Encoder, Frame, Repeat};
use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use juggling_core::animation::{AnimationKind, AnimationSpec};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Blob, BlobEvent, BlobPropertyBag, CanvasRenderingContext2d, Event, HtmlCanvasElement,
    MediaRecorder, MediaRecorderOptions, MediaStream, MediaStreamTrack, window,
};

const GIF_MAX_DIMENSION: u32 = u16::MAX as u32;
const ANTIALIAS_SCALE: u32 = 2;
const MP4_MIME_TYPES: [&str; 3] = [
    "video/mp4;codecs=avc1.42E01E",
    "video/mp4;codecs=avc1",
    "video/mp4",
];
const WEBM_MIME_TYPES: [&str; 3] = [
    "video/webm;codecs=vp9",
    "video/webm;codecs=vp8",
    "video/webm",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationExportFormat {
    Gif,
    WebM,
    Mp4,
}

impl AnimationExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Gif => "gif",
            Self::WebM => "webm",
            Self::Mp4 => "mp4",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationExportOptions {
    pub format: AnimationExportFormat,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub slowdown: f64,
    pub antialiasing: bool,
    pub show_title: bool,
}

pub struct ExportedAnimation {
    pub blob: Blob,
    pub filename: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FramePlan {
    frames_per_loop: usize,
    prop_period: usize,
    total_frames: usize,
    gif_delay_hundredths: u16,
    frame_duration_ms: f64,
    pattern_period_secs: f64,
}

impl FramePlan {
    fn new(spec: &AnimationSpec, options: &AnimationExportOptions) -> Result<Self, String> {
        if !spec.period_secs.is_finite() || spec.period_secs <= 0.0 {
            return Err("Animation export requires a positive pattern period".to_string());
        }
        if !options.fps.is_finite() || !(1.0..=60.0).contains(&options.fps) {
            return Err("Frames per second must be between 1 and 60".to_string());
        }
        if !options.slowdown.is_finite() || options.slowdown <= 0.0 {
            return Err("Slowdown factor must be greater than zero".to_string());
        }
        let AnimationKind::Jml(jml) = &spec.kind else {
            return Err("The current pattern has no physical animation to export".to_string());
        };
        if jml.layout.is_none() {
            return Err("The current pattern has no physical animation to export".to_string());
        }

        let (frames_per_loop, gif_delay_hundredths, frame_duration_ms) = match options.format {
            AnimationExportFormat::Gif => {
                let delay = (100.0 / options.fps).round().max(1.0) as u16;
                let achieved_fps = 100.0 / delay as f64;
                let frames = (spec.period_secs * options.slowdown * achieved_fps)
                    .round()
                    .max(1.0) as usize;
                (frames, delay, delay as f64 * 10.0)
            }
            AnimationExportFormat::WebM | AnimationExportFormat::Mp4 => {
                let frames = (spec.period_secs * options.slowdown * options.fps)
                    .round()
                    .max(1.0) as usize;
                (frames, 0, 1000.0 / options.fps)
            }
        };
        let prop_period = jml.period_with_props().max(1);
        Ok(Self {
            frames_per_loop,
            prop_period,
            total_frames: frames_per_loop.saturating_mul(prop_period),
            gif_delay_hundredths,
            frame_duration_ms,
            pattern_period_secs: spec.period_secs,
        })
    }

    fn simulation_time(self, frame: usize) -> f64 {
        frame as f64 / self.frames_per_loop as f64 * self.pattern_period_secs
    }
}

struct ExportSurface {
    render_canvas: HtmlCanvasElement,
    output_canvas: HtmlCanvasElement,
    output_context: CanvasRenderingContext2d,
    width: u32,
    height: u32,
    render_scale: u32,
}

struct CanvasCapture {
    stream: MediaStream,
    frame_request: Option<CanvasFrameRequest>,
}

struct CanvasFrameRequest {
    target: JsValue,
    function: Function,
}

impl CanvasCapture {
    fn new(canvas: &HtmlCanvasElement, fps: f64) -> Result<Self, String> {
        let stream = canvas
            .capture_stream_with_frame_request_rate(0.0)
            .map_err(|error| js_error("Unable to capture export canvas", error))?;
        let track = first_video_track(&stream)?;
        if let Some(frame_request) = canvas_frame_request(&stream, &track) {
            return Ok(Self {
                stream,
                frame_request: Some(frame_request),
            });
        }

        // Some engines support canvas capture but not explicit frame requests.
        // Recreate the stream at the requested rate so recording still works.
        stop_stream_tracks(&stream);
        let stream = canvas
            .capture_stream_with_frame_request_rate(fps)
            .map_err(|error| js_error("Unable to capture export canvas", error))?;
        first_video_track(&stream)?;
        Ok(Self {
            stream,
            frame_request: None,
        })
    }

    fn request_frame(&self) -> Result<(), String> {
        let Some(request) = &self.frame_request else {
            return Ok(());
        };
        request
            .function
            .call0(&request.target)
            .map(|_| ())
            .map_err(|error| js_error("Unable to request video export frame", error))
    }
}

impl ExportSurface {
    fn new(width: u32, height: u32, antialiasing: bool) -> Result<Self, String> {
        let render_canvas = create_canvas()?;
        let output_canvas = if antialiasing {
            create_canvas()?
        } else {
            render_canvas.clone()
        };
        output_canvas.set_width(width);
        output_canvas.set_height(height);
        let output_context = canvas_context(&output_canvas)?;
        output_context.set_image_smoothing_enabled(antialiasing);
        Ok(Self {
            render_canvas,
            output_canvas,
            output_context,
            width,
            height,
            render_scale: if antialiasing { ANTIALIAS_SCALE } else { 1 },
        })
    }

    fn render(
        &self,
        spec: &AnimationSpec,
        settings: &RenderSettings,
        time: f64,
    ) -> Result<(), String> {
        canvas::render_export_frame(
            &self.render_canvas,
            spec,
            settings,
            time,
            self.width,
            self.height,
            self.render_scale,
        )?;
        if self.render_scale > 1 {
            self.output_context
                .set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
                .map_err(|_| "Unable to prepare antialiased export frame".to_string())?;
            self.output_context
                .clear_rect(0.0, 0.0, self.width as f64, self.height as f64);
            self.output_context
                .draw_image_with_html_canvas_element_and_dw_and_dh(
                    &self.render_canvas,
                    0.0,
                    0.0,
                    self.width as f64,
                    self.height as f64,
                )
                .map_err(|_| "Unable to downsample antialiased export frame".to_string())?;
        }
        Ok(())
    }

    fn rgba(&self) -> Result<Vec<u8>, String> {
        self.output_context
            .get_image_data(0.0, 0.0, self.width as f64, self.height as f64)
            .map(|image| image.data().0)
            .map_err(|_| {
                "Unable to read export pixels; an external image prop may not allow cross-origin export"
                    .to_string()
            })
    }
}

pub fn mp4_supported() -> bool {
    mp4_mime_type().is_some()
}

pub fn webm_supported() -> bool {
    webm_mime_type().is_some()
}

pub async fn export_animation(
    spec: AnimationSpec,
    mut settings: RenderSettings,
    options: AnimationExportOptions,
    cancelled: Arc<AtomicBool>,
    progress: Rc<dyn Fn(usize, usize)>,
) -> Result<ExportedAnimation, String> {
    validate_dimensions(&options)?;
    let plan = FramePlan::new(&spec, &options)?;
    settings.paused = true;
    settings.speed = 1.0;
    settings.catch_sound = false;
    settings.bounce_sound = false;
    settings.selected_event = None;
    settings.selected_position = None;
    settings.position_edit_handle = None;
    settings.show_title = options.show_title;
    settings.show_axes = false;

    let blob = match options.format {
        AnimationExportFormat::Gif => {
            export_gif(&spec, &settings, &options, plan, &cancelled, &progress).await?
        }
        AnimationExportFormat::WebM => {
            let mime_type = webm_mime_type().ok_or_else(|| {
                "WebM export is not supported by this browser; GIF export remains available"
                    .to_string()
            })?;
            export_recorded_video(
                &spec, &settings, &options, plan, &cancelled, &progress, mime_type, "WebM",
            )
            .await?
        }
        AnimationExportFormat::Mp4 => {
            let mime_type = mp4_mime_type().ok_or_else(|| {
                "MP4/H.264 export is not supported by this browser; GIF export remains available"
                    .to_string()
            })?;
            export_recorded_video(
                &spec, &settings, &options, plan, &cancelled, &progress, mime_type, "MP4",
            )
            .await?
        }
    };
    Ok(ExportedAnimation {
        filename: animation_filename(&spec.title, options.format),
        blob,
    })
}

async fn export_gif(
    spec: &AnimationSpec,
    settings: &RenderSettings,
    options: &AnimationExportOptions,
    plan: FramePlan,
    cancelled: &Arc<AtomicBool>,
    progress: &Rc<dyn Fn(usize, usize)>,
) -> Result<Blob, String> {
    let surface = ExportSurface::new(options.width, options.height, options.antialiasing)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Animation export cancelled".to_string());
    }
    surface.render(spec, settings, plan.simulation_time(0))?;
    let mut first_frame_rgba = surface.rgba()?;
    let quantizer = NeuQuant::new(10, 256, &first_frame_rgba);
    let global_palette = quantizer.color_map_rgb();
    let mut output = Vec::new();
    {
        let mut encoder = Encoder::new(
            &mut output,
            options.width as u16,
            options.height as u16,
            &global_palette,
        )
        .map_err(|error| format!("Unable to start GIF encoder: {error}"))?;
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|error| format!("Unable to configure GIF loop: {error}"))?;

        for frame_index in 0..plan.total_frames {
            if cancelled.load(Ordering::Relaxed) {
                return Err("Animation export cancelled".to_string());
            }
            let rgba = if frame_index == 0 {
                std::mem::take(&mut first_frame_rgba)
            } else {
                surface.render(spec, settings, plan.simulation_time(frame_index))?;
                surface.rgba()?
            };
            let indexed = rgba
                .chunks_exact(4)
                .map(|pixel| quantizer.index_of(pixel) as u8)
                .collect::<Vec<_>>();
            let mut frame = Frame::from_indexed_pixels(
                options.width as u16,
                options.height as u16,
                indexed,
                None,
            );
            frame.delay = plan.gif_delay_hundredths;
            encoder
                .write_frame(&frame)
                .map_err(|error| format!("Unable to encode GIF frame: {error}"))?;
            progress(frame_index + 1, plan.total_frames);
            wait_ms(0).await?;
        }
    }
    blob_from_bytes(&output, "image/gif")
}

async fn export_recorded_video(
    spec: &AnimationSpec,
    settings: &RenderSettings,
    options: &AnimationExportOptions,
    plan: FramePlan,
    cancelled: &Arc<AtomicBool>,
    progress: &Rc<dyn Fn(usize, usize)>,
    mime_type: &'static str,
    format_name: &'static str,
) -> Result<Blob, String> {
    let surface = ExportSurface::new(options.width, options.height, options.antialiasing)?;
    let capture = CanvasCapture::new(&surface.output_canvas, options.fps)?;
    let stream = &capture.stream;
    let recorder_options = MediaRecorderOptions::new();
    recorder_options.set_mime_type(mime_type);
    recorder_options.set_video_bits_per_second(8_000_000);
    let recorder =
        MediaRecorder::new_with_media_stream_and_media_recorder_options(stream, &recorder_options)
            .map_err(|error| js_error(&format!("Unable to create {format_name} encoder"), error))?;

    let chunks = Array::new();
    let chunks_for_event = chunks.clone();
    let on_data = Closure::<dyn FnMut(BlobEvent)>::new(move |event: BlobEvent| {
        if let Some(data) = event.data().filter(|blob| blob.size() > 0.0) {
            chunks_for_event.push(&data);
        }
    });
    recorder.set_ondataavailable(Some(on_data.as_ref().unchecked_ref()));

    let resolve_slot = Rc::new(RefCell::new(None::<Function>));
    let reject_slot = Rc::new(RefCell::new(None::<Function>));
    let stop_promise = Promise::new(&mut |resolve, reject| {
        *resolve_slot.borrow_mut() = Some(resolve);
        *reject_slot.borrow_mut() = Some(reject);
    });
    let resolve_on_stop = Rc::clone(&resolve_slot);
    let on_stop = Closure::<dyn FnMut(Event)>::new(move |_| {
        if let Some(resolve) = resolve_on_stop.borrow_mut().take() {
            resolve.call0(&JsValue::NULL).ok();
        }
    });
    recorder.set_onstop(Some(on_stop.as_ref().unchecked_ref()));
    let reject_on_error = Rc::clone(&reject_slot);
    let on_error = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        if let Some(reject) = reject_on_error.borrow_mut().take() {
            reject.call1(&JsValue::NULL, event.as_ref()).ok();
        }
    });
    recorder.set_onerror(Some(on_error.as_ref().unchecked_ref()));

    if let Err(error) = recorder.start() {
        stop_stream_tracks(stream);
        return Err(js_error(
            &format!("Unable to start {format_name} encoder"),
            error,
        ));
    }

    let mut frame_error = None;
    for frame_index in 0..plan.total_frames {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        if let Err(error) = surface.render(spec, settings, plan.simulation_time(frame_index)) {
            frame_error = Some(error);
            break;
        }
        if let Err(error) = capture.request_frame() {
            frame_error = Some(error);
            break;
        }
        progress(frame_index + 1, plan.total_frames);
        if let Err(error) = wait_ms(plan.frame_duration_ms.round().max(1.0) as i32).await {
            frame_error = Some(error);
            break;
        }
    }

    let stop_result = recorder
        .stop()
        .map_err(|error| js_error(&format!("Unable to finish {format_name} encoder"), error));
    let finished = match stop_result {
        Ok(()) => JsFuture::from(stop_promise)
            .await
            .map(|_| ())
            .map_err(|error| js_error(&format!("{format_name} encoder failed"), error)),
        Err(error) => Err(error),
    };
    stop_stream_tracks(stream);
    recorder.set_ondataavailable(None);
    recorder.set_onstop(None);
    recorder.set_onerror(None);
    finished?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Animation export cancelled".to_string());
    }
    if let Some(error) = frame_error {
        return Err(error);
    }

    let blob_options = BlobPropertyBag::new();
    blob_options.set_type(mime_type);
    Blob::new_with_blob_sequence_and_options(&chunks, &blob_options)
        .map_err(|error| js_error(&format!("Unable to assemble {format_name} file"), error))
}

fn validate_dimensions(options: &AnimationExportOptions) -> Result<(), String> {
    if !(64..=4096).contains(&options.width) || !(64..=4096).contains(&options.height) {
        return Err("Export width and height must be between 64 and 4096 pixels".to_string());
    }
    if options.format == AnimationExportFormat::Gif
        && (options.width > GIF_MAX_DIMENSION || options.height > GIF_MAX_DIMENSION)
    {
        return Err("GIF dimensions exceed the format limit".to_string());
    }
    Ok(())
}

fn create_canvas() -> Result<HtmlCanvasElement, String> {
    window()
        .and_then(|window| window.document())
        .ok_or_else(|| "Document is unavailable for animation export".to_string())?
        .create_element("canvas")
        .map_err(|error| js_error("Unable to create animation export canvas", error))?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| "Unable to create animation export canvas".to_string())
}

fn canvas_context(canvas: &HtmlCanvasElement) -> Result<CanvasRenderingContext2d, String> {
    canvas
        .get_context("2d")
        .map_err(|error| js_error("Unable to create animation export canvas", error))?
        .ok_or_else(|| "2D canvas is unavailable for animation export".to_string())?
        .dyn_into::<CanvasRenderingContext2d>()
        .map_err(|_| "Unable to create animation export canvas".to_string())
}

fn first_video_track(stream: &MediaStream) -> Result<JsValue, String> {
    let track = stream.get_video_tracks().get(0);
    if track.is_null() || track.is_undefined() {
        Err("Canvas capture did not provide a video track".to_string())
    } else {
        Ok(track)
    }
}

fn canvas_frame_request(stream: &MediaStream, track: &JsValue) -> Option<CanvasFrameRequest> {
    if let Some(function) = function_property(track, "requestFrame") {
        return Some(CanvasFrameRequest {
            target: track.clone(),
            function,
        });
    }

    let target = JsValue::from(stream.clone());
    function_property(&target, "requestFrame")
        .map(|function| CanvasFrameRequest { target, function })
}

fn function_property(target: &JsValue, name: &str) -> Option<Function> {
    Reflect::get(target, &JsValue::from_str(name))
        .ok()?
        .dyn_into::<Function>()
        .ok()
}

fn stop_stream_tracks(stream: &MediaStream) {
    for track in stream.get_video_tracks().iter() {
        if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
            track.stop();
        }
    }
}

fn mp4_mime_type() -> Option<&'static str> {
    supported_media_recorder_mime_type(&MP4_MIME_TYPES)
}

fn webm_mime_type() -> Option<&'static str> {
    supported_media_recorder_mime_type(&WEBM_MIME_TYPES)
}

fn supported_media_recorder_mime_type(candidates: &'static [&'static str]) -> Option<&'static str> {
    if !Reflect::has(&js_sys::global(), &JsValue::from_str("MediaRecorder")).unwrap_or(false) {
        return None;
    }
    candidates
        .iter()
        .copied()
        .find(|mime| MediaRecorder::is_type_supported(mime))
}

fn blob_from_bytes(bytes: &[u8], mime_type: &str) -> Result<Blob, String> {
    let parts = Array::new();
    parts.push(&Uint8Array::from(bytes));
    let options = BlobPropertyBag::new();
    options.set_type(mime_type);
    Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(|error| js_error("Unable to create animation file", error))
}

async fn wait_ms(milliseconds: i32) -> Result<(), String> {
    let Some(window) = window() else {
        return Err("Window is unavailable during animation export".to_string());
    };
    let promise = Promise::new(&mut |resolve, _reject| {
        let callback = Closure::once_into_js(move || {
            resolve.call0(&JsValue::NULL).ok();
        });
        window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.unchecked_ref(),
                milliseconds,
            )
            .ok();
    });
    JsFuture::from(promise)
        .await
        .map(|_| ())
        .map_err(|error| js_error("Animation export timer failed", error))
}

fn animation_filename(title: &str, format: AnimationExportFormat) -> String {
    let mut stem = title
        .trim()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    while stem.contains("__") {
        stem = stem.replace("__", "_");
    }
    let stem = stem.trim_matches('_');
    let stem = if stem.is_empty() {
        "jugglinglab-animation"
    } else {
        stem
    };
    format!("{stem}.{}", format.extension())
}

fn js_error(context: &str, error: JsValue) -> String {
    error
        .as_string()
        .map(|message| format!("{context}: {message}"))
        .unwrap_or_else(|| context.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use juggling_core::jml::PatternRecord;

    fn cascade() -> AnimationSpec {
        AnimationSpec::from_record(&PatternRecord::siteswap(
            "Three ball cascade".to_string(),
            "pattern=3".to_string(),
        ))
        .unwrap()
    }

    #[test]
    fn gif_frame_plan_matches_original_hundredth_second_timing() {
        let spec = cascade();
        let options = AnimationExportOptions {
            format: AnimationExportFormat::Gif,
            width: 400,
            height: 450,
            fps: 33.3,
            slowdown: 2.0,
            antialiasing: false,
            show_title: true,
        };

        let plan = FramePlan::new(&spec, &options).unwrap();

        assert_eq!(plan.gif_delay_hundredths, 3);
        assert_eq!(
            plan.frames_per_loop,
            (spec.period_secs * 2.0 * (100.0 / 3.0)).round() as usize
        );
        assert_eq!(plan.total_frames, plan.frames_per_loop * plan.prop_period);
        assert!((plan.simulation_time(plan.frames_per_loop) - spec.period_secs).abs() < 1e-9);
    }

    #[test]
    fn animation_export_filename_is_browser_safe() {
        assert_eq!(
            animation_filename("  Mills Mess: 3 balls  ", AnimationExportFormat::Gif),
            "Mills_Mess_3_balls.gif"
        );
        assert_eq!(
            animation_filename("***", AnimationExportFormat::Mp4),
            "jugglinglab-animation.mp4"
        );
        assert_eq!(
            animation_filename("Cascade", AnimationExportFormat::WebM),
            "Cascade.webm"
        );
    }
}
