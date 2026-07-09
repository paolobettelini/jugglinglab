use crate::parameter_list::ParameterList;

#[derive(Clone, Debug, PartialEq)]
pub struct PropSpec {
    pub kind: PropKind,
    pub color: Option<String>,
    pub diameter: f64,
    pub inside_diameter: Option<f64>,
    pub image_source: Option<String>,
    pub image_aspect_ratio: Option<f64>,
    pub highlight: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropKind {
    Ball,
    Square,
    Ring,
    Image,
    Unknown(String),
}

impl PropSpec {
    pub fn from_jml(prop_type: &str, modifier: Option<&str>) -> Result<Self, String> {
        let mut spec = Self::default_for_type(prop_type);
        let params = ParameterList::parse(modifier)?;

        if let Some(color) = params.get_parameter("color") {
            spec.color = Some(css_color(color)?);
        }
        if let Some(highlight) = params.get_parameter("highlight") {
            spec.highlight = highlight.eq_ignore_ascii_case("true");
        }

        match spec.kind {
            PropKind::Ball | PropKind::Square => {
                if let Some(diam) = params.get_parameter("diam") {
                    spec.diameter = parse_positive(diam, "diam")?;
                }
            }
            PropKind::Ring => {
                if let Some(outside) = params.get_parameter("outside") {
                    spec.diameter = parse_positive(outside, "outside")?;
                }
                if let Some(inside) = params.get_parameter("inside") {
                    spec.inside_diameter = Some(parse_positive(inside, "inside")?);
                }
            }
            PropKind::Image => {
                if let Some(source) = params.get_parameter("image") {
                    spec.image_source = Some(source.trim().to_string());
                    spec.image_aspect_ratio = Some(default_image_aspect_ratio(source));
                }
                if let Some(width) = params.get_parameter("width") {
                    spec.diameter = parse_positive(width, "width")?;
                }
            }
            PropKind::Unknown(_) => {}
        }

        Ok(spec)
    }

    pub fn default_for_type(prop_type: &str) -> Self {
        let kind = if prop_type.eq_ignore_ascii_case("ball") {
            PropKind::Ball
        } else if prop_type.eq_ignore_ascii_case("square") {
            PropKind::Square
        } else if prop_type.eq_ignore_ascii_case("ring") {
            PropKind::Ring
        } else if prop_type.eq_ignore_ascii_case("image") {
            PropKind::Image
        } else {
            PropKind::Unknown(prop_type.to_string())
        };

        match kind {
            PropKind::Ring => Self {
                kind,
                color: Some(css_color_name("red").to_string()),
                diameter: 25.0,
                inside_diameter: Some(20.0),
                image_source: None,
                image_aspect_ratio: None,
                highlight: false,
            },
            PropKind::Image => Self {
                kind,
                color: None,
                diameter: 10.0,
                inside_diameter: None,
                image_source: Some("ball.png".to_string()),
                image_aspect_ratio: Some(1.0),
                highlight: false,
            },
            _ => Self {
                kind,
                color: Some(css_color_name("red").to_string()),
                diameter: 10.0,
                inside_diameter: None,
                image_source: None,
                image_aspect_ratio: None,
                highlight: false,
            },
        }
    }

    pub fn radius_cm(&self) -> f64 {
        match self.kind {
            PropKind::Image => self.diameter,
            _ => 0.5 * self.diameter,
        }
    }

    pub fn min_z_cm(&self) -> f64 {
        match self.kind {
            PropKind::Image => 0.0,
            _ => -self.radius_cm(),
        }
    }
}

fn parse_positive(value: &str, name: &str) -> Result<f64, String> {
    let parsed = value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("Invalid number for prop {name}"))?;
    if parsed > 0.0 && parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(format!("Invalid prop diameter for {name}"))
    }
}

fn css_color(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.contains(',') {
        let tokens = trimmed
            .trim_matches('{')
            .trim_matches('}')
            .split(',')
            .map(|token| token.trim().parse::<u8>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| format!("Invalid prop color: {value}"))?;
        return match tokens.as_slice() {
            [r, g, b] => Ok(format!("rgb({r},{g},{b})")),
            [r, g, b, a] => Ok(format!("rgba({r},{g},{b},{:.3})", *a as f64 / 255.0)),
            _ => Err(format!("Invalid prop color: {value}")),
        };
    }
    Ok(css_color_name(trimmed).to_string())
}

fn css_color_name(name: &str) -> &'static str {
    match name.to_ascii_lowercase().as_str() {
        "transparent" => "rgba(0,0,0,0)",
        "black" => "#000000",
        "blue" => "#0000ff",
        "cyan" => "#00ffff",
        "gray" | "grey" => "#808080",
        "green" => "#008000",
        "magenta" => "#ff00ff",
        "orange" => "#ffc800",
        "pink" => "#ffafaf",
        "red" => "#ff0000",
        "white" => "#ffffff",
        "yellow" => "#ffff00",
        _ => "#ff0000",
    }
}

fn default_image_aspect_ratio(source: &str) -> f64 {
    match source.trim().rsplit('/').next().unwrap_or(source.trim()) {
        "ball.png" => 1.0,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ball_diameter_and_color() {
        let prop = PropSpec::from_jml("ball", Some("diam=12;color=blue;highlight=true")).unwrap();
        assert_eq!(prop.kind, PropKind::Ball);
        assert_eq!(prop.diameter, 12.0);
        assert_eq!(prop.color.as_deref(), Some("#0000ff"));
        assert!(prop.highlight);
    }

    #[test]
    fn parses_ring_diameters() {
        let prop =
            PropSpec::from_jml("ring", Some("outside=30;inside=22;color={10,20,30}")).unwrap();
        assert_eq!(prop.kind, PropKind::Ring);
        assert_eq!(prop.diameter, 30.0);
        assert_eq!(prop.inside_diameter, Some(22.0));
        assert_eq!(prop.color.as_deref(), Some("rgb(10,20,30)"));
    }

    #[test]
    fn parses_image_source_and_width() {
        let prop = PropSpec::from_jml("image", Some("image=ball.png;width=16")).unwrap();
        assert_eq!(prop.kind, PropKind::Image);
        assert_eq!(prop.image_source.as_deref(), Some("ball.png"));
        assert_eq!(prop.diameter, 16.0);
        assert_eq!(prop.min_z_cm(), 0.0);
    }
}
