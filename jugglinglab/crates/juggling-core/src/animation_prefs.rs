use crate::parameter_list::ParameterList;
use crate::util::to_string_rounded;
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShowGround {
    #[default]
    Auto,
    On,
    Off,
}

impl ShowGround {
    pub fn as_index(self) -> usize {
        match self {
            Self::Auto => 0,
            Self::On => 1,
            Self::Off => 2,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Auto),
            1 => Some(Self::On),
            2 => Some(Self::Off),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DefaultView {
    #[default]
    None,
    Simple,
    Edit,
    Pattern,
    Selection,
}

impl DefaultView {
    pub fn parameter_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Simple => Some("simple"),
            Self::Edit => Some("visual_editor"),
            Self::Pattern => Some("pattern_editor"),
            Self::Selection => Some("selection_editor"),
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("simple") {
            Some(Self::Simple)
        } else if value.eq_ignore_ascii_case("visual_editor") {
            Some(Self::Edit)
        } else if value.eq_ignore_ascii_case("pattern_editor") {
            Some(Self::Pattern)
        } else if value.eq_ignore_ascii_case("selection_editor") {
            Some(Self::Selection)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationPrefs {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub slowdown: f64,
    pub border_pixels: i32,
    pub show_ground: ShowGround,
    pub stereo: bool,
    pub start_paused: bool,
    pub mouse_pause: bool,
    pub catch_sound: bool,
    pub bounce_sound: bool,
    pub default_camera_angle: Option<[f64; 2]>,
    pub default_view: DefaultView,
    pub hide_jugglers: Vec<i32>,
}

impl AnimationPrefs {
    pub const WIDTH_DEFAULT: i32 = 400;
    pub const HEIGHT_DEFAULT: i32 = 450;
    pub const FPS_DEFAULT: f64 = 33.3;
    pub const SLOWDOWN_DEFAULT: f64 = 2.0;
    pub const BORDER_PIXELS_DEFAULT: i32 = 0;

    pub fn parse(source: Option<&str>) -> Result<Self, String> {
        let mut parameters = ParameterList::parse(source)?;
        let result = Self::from_parameters(&mut parameters)?;
        parameters.error_if_parameters_left()?;
        Ok(result)
    }

    pub fn from_parameters(parameters: &mut ParameterList) -> Result<Self, String> {
        let mut result = Self::default();

        if let Some(value) = parameters.remove_parameter("width") {
            result.width = parse_i32(&value, "width")?;
        }
        if let Some(value) = parameters.remove_parameter("height") {
            result.height = parse_i32(&value, "height")?;
        }
        if let Some(value) = parameters.remove_parameter("fps") {
            result.fps = parse_f64(&value, "fps")?;
        }
        if let Some(value) = parameters.remove_parameter("slowdown") {
            result.slowdown = parse_f64(&value, "slowdown")?;
        }
        if let Some(value) = parameters.remove_parameter("border") {
            result.border_pixels = parse_i32(&value, "border")?;
        }
        if let Some(value) = parameters.remove_parameter("showground") {
            result.show_ground = if value.eq_ignore_ascii_case("auto") {
                ShowGround::Auto
            } else if ["true", "on", "yes"]
                .iter()
                .any(|candidate| value.eq_ignore_ascii_case(candidate))
            {
                ShowGround::On
            } else if ["false", "off", "no"]
                .iter()
                .any(|candidate| value.eq_ignore_ascii_case(candidate))
            {
                ShowGround::Off
            } else {
                return Err(format!("Unrecognized \"showground\" value: {value}"));
            };
        }
        if let Some(value) = parameters.remove_parameter("stereo") {
            result.stereo = kotlin_boolean(&value);
        }
        if let Some(value) = parameters.remove_parameter("startpaused") {
            result.start_paused = kotlin_boolean(&value);
        }
        if let Some(value) = parameters.remove_parameter("mousepause") {
            result.mouse_pause = kotlin_boolean(&value);
        }
        if let Some(value) = parameters.remove_parameter("catchsound") {
            result.catch_sound = kotlin_boolean(&value);
        }
        if let Some(value) = parameters.remove_parameter("bouncesound") {
            result.bounce_sound = kotlin_boolean(&value);
        }
        if let Some(value) = parameters.remove_parameter("camangle") {
            let clean = value.replace(['(', ')', '{', '}'], "");
            let tokens = clean.split(',').collect::<Vec<_>>();
            if tokens.len() > 2 {
                return Err("Too many elements given in \"camangle\" value".to_string());
            }
            let mut angle = [0.0, 90.0];
            for (index, token) in tokens.into_iter().enumerate() {
                if !token.trim().is_empty() {
                    angle[index] = parse_f64(token.trim(), "camangle")?;
                }
            }
            result.default_camera_angle = Some(angle);
        }
        if let Some(value) = parameters.remove_parameter("view") {
            result.default_view = DefaultView::parse(&value)
                .ok_or_else(|| format!("Unrecognized view type: \"{value}\""))?;
        }
        if let Some(value) = parameters.remove_parameter("hidejugglers") {
            let clean = value.replace(['(', ')'], "");
            result.hide_jugglers = clean
                .split(',')
                .filter_map(|token| {
                    let token = token.trim();
                    (!token.is_empty()).then_some(token)
                })
                .map(|token| parse_i32(token, "hidejugglers"))
                .collect::<Result<Vec<_>, _>>()?;
        }

        Ok(result)
    }
}

impl Default for AnimationPrefs {
    fn default() -> Self {
        Self {
            width: Self::WIDTH_DEFAULT,
            height: Self::HEIGHT_DEFAULT,
            fps: Self::FPS_DEFAULT,
            slowdown: Self::SLOWDOWN_DEFAULT,
            border_pixels: Self::BORDER_PIXELS_DEFAULT,
            show_ground: ShowGround::Auto,
            stereo: false,
            start_paused: false,
            mouse_pause: false,
            catch_sound: false,
            bounce_sound: false,
            default_camera_angle: None,
            default_view: DefaultView::None,
            hide_jugglers: Vec::new(),
        }
    }
}

impl fmt::Display for AnimationPrefs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parameters = Vec::new();
        if self.width != Self::WIDTH_DEFAULT {
            parameters.push(format!("width={}", self.width));
        }
        if self.height != Self::HEIGHT_DEFAULT {
            parameters.push(format!("height={}", self.height));
        }
        if self.fps != Self::FPS_DEFAULT {
            parameters.push(format!("fps={}", to_string_rounded(self.fps, 2)));
        }
        if self.slowdown != Self::SLOWDOWN_DEFAULT {
            parameters.push(format!("slowdown={}", to_string_rounded(self.slowdown, 2)));
        }
        if self.border_pixels != Self::BORDER_PIXELS_DEFAULT {
            parameters.push(format!("border={}", self.border_pixels));
        }
        match self.show_ground {
            ShowGround::Auto => {}
            ShowGround::On => parameters.push("showground=true".to_string()),
            ShowGround::Off => parameters.push("showground=false".to_string()),
        }
        push_bool(&mut parameters, "stereo", self.stereo);
        push_bool(&mut parameters, "startpaused", self.start_paused);
        push_bool(&mut parameters, "mousepause", self.mouse_pause);
        push_bool(&mut parameters, "catchsound", self.catch_sound);
        push_bool(&mut parameters, "bouncesound", self.bounce_sound);
        if let Some([yaw, pitch]) = self.default_camera_angle {
            parameters.push(format!(
                "camangle=({},{})",
                kotlin_double_string(yaw),
                kotlin_double_string(pitch)
            ));
        }
        if let Some(name) = self.default_view.parameter_name() {
            parameters.push(format!("view={name}"));
        }
        if !self.hide_jugglers.is_empty() {
            parameters.push(format!(
                "hidejugglers=({})",
                self.hide_jugglers
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ));
        }
        write!(f, "{}", parameters.join(";"))
    }
}

fn parse_i32(value: &str, name: &str) -> Result<i32, String> {
    value
        .trim()
        .parse::<i32>()
        .map_err(|_| format!("Number format error in \"{name}\" value"))
}

fn parse_f64(value: &str, name: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("Number format error in \"{name}\" value"))
}

fn kotlin_boolean(value: &str) -> bool {
    value.eq_ignore_ascii_case("true")
}

fn push_bool(parameters: &mut Vec<String>, name: &str, value: bool) {
    if value {
        parameters.push(format!("{name}=true"));
    }
}

fn kotlin_double_string(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_serialize_to_an_empty_string() {
        assert_eq!(AnimationPrefs::default().to_string(), "");
    }

    #[test]
    fn parses_and_serializes_all_original_fields_in_order() {
        let source = concat!(
            "width=640;height=480;fps=25;slowdown=3.5;border=12;",
            "showground=off;stereo=true;startpaused=true;mousepause=true;",
            "catchsound=true;bouncesound=true;camangle=(340,70);",
            "view=selection_editor;hidejugglers=(2,4)"
        );
        let prefs = AnimationPrefs::parse(Some(source)).unwrap();

        assert_eq!(prefs.show_ground, ShowGround::Off);
        assert_eq!(prefs.default_view, DefaultView::Selection);
        assert_eq!(prefs.default_camera_angle, Some([340.0, 70.0]));
        assert_eq!(prefs.hide_jugglers, vec![2, 4]);
        assert_eq!(
            prefs.to_string(),
            concat!(
                "width=640;height=480;fps=25;slowdown=3.5;border=12;",
                "showground=false;stereo=true;startpaused=true;mousepause=true;",
                "catchsound=true;bouncesound=true;camangle=(340.0,70.0);",
                "view=selection_editor;hidejugglers=(2,4)"
            )
        );
    }

    #[test]
    fn showground_accepts_the_original_aliases() {
        for value in ["true", "on", "YES"] {
            assert_eq!(
                AnimationPrefs::parse(Some(&format!("showground={value}")))
                    .unwrap()
                    .show_ground,
                ShowGround::On
            );
        }
        for value in ["false", "off", "NO"] {
            assert_eq!(
                AnimationPrefs::parse(Some(&format!("showground={value}")))
                    .unwrap()
                    .show_ground,
                ShowGround::Off
            );
        }
    }

    #[test]
    fn camera_angle_defaults_to_ninety_degrees_for_one_value() {
        assert_eq!(
            AnimationPrefs::parse(Some("camangle={12.5}"))
                .unwrap()
                .default_camera_angle,
            Some([12.5, 90.0])
        );
    }

    #[test]
    fn kotlin_booleans_treat_every_value_except_true_as_false() {
        assert!(!AnimationPrefs::parse(Some("stereo=yes")).unwrap().stereo);
        assert!(AnimationPrefs::parse(Some("stereo=TRUE")).unwrap().stereo);
    }

    #[test]
    fn rejects_unknown_parameters_and_views() {
        assert!(
            AnimationPrefs::parse(Some("width=400;unknown=1"))
                .unwrap_err()
                .contains("Unused parameter")
        );
        assert!(
            AnimationPrefs::parse(Some("view=edit"))
                .unwrap_err()
                .contains("Unrecognized view type")
        );
    }
}
