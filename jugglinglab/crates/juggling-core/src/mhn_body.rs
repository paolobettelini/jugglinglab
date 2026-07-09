use crate::util::{expand_repeats, parse_finite_double, split_on_char_outside_parens};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyPosition {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub t: f64,
    pub angle: f64,
    pub juggler: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MhnBody {
    config: String,
    number_of_jugglers: usize,
    number_of_beats: Vec<usize>,
    number_of_positions_per_beat: Vec<Vec<usize>>,
    body_positions: Vec<Vec<Vec<Option<BodyCoordinate>>>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BodyCoordinate {
    angle: f64,
    x: f64,
    y: f64,
    z: f64,
}

impl MhnBody {
    pub fn parse(config: &str) -> Result<Self, String> {
        let clean_str = expand_repeats(config)
            .chars()
            .filter(|ch| !matches!(ch, '<' | '>' | '{' | '}'))
            .collect::<String>();
        let juggler_strings = clean_str
            .split(['|', '!'])
            .map(str::to_string)
            .collect::<Vec<_>>();

        let number_of_jugglers = juggler_strings.len();
        if number_of_jugglers == 0 {
            return Err("Empty body parameter".to_string());
        }

        let mut body_positions = Vec::with_capacity(number_of_jugglers);
        for juggler_str in &juggler_strings {
            let mut beats = Vec::new();
            for beat_str in split_on_char_outside_parens(juggler_str.trim(), '.') {
                let mut positions = parse_beat(&beat_str)?;
                if positions.is_empty() {
                    positions.push(None);
                }
                beats.push(positions);
            }
            body_positions.push(beats);
        }

        let number_of_beats = body_positions.iter().map(Vec::len).collect::<Vec<_>>();
        let number_of_positions_per_beat = body_positions
            .iter()
            .map(|beats| beats.iter().map(Vec::len).collect())
            .collect::<Vec<_>>();

        Ok(Self {
            config: config.to_string(),
            number_of_jugglers,
            number_of_beats,
            number_of_positions_per_beat,
            body_positions,
        })
    }

    pub fn config(&self) -> &str {
        &self.config
    }

    pub fn number_of_jugglers(&self) -> usize {
        self.number_of_jugglers
    }

    pub fn get_period(&self, juggler: usize) -> usize {
        self.number_of_beats[self.juggler_index(juggler)]
    }

    pub fn get_number_of_positions(&self, juggler: usize, pos: usize) -> usize {
        self.number_of_positions_per_beat[self.juggler_index(juggler)]
            .get(pos)
            .copied()
            .unwrap_or(0)
    }

    pub fn get_position(&self, juggler: usize, pos: usize, index: usize) -> Option<BodyPosition> {
        let juggler_index = self.juggler_index(juggler);
        if pos >= self.get_period(juggler) || index >= self.get_number_of_positions(juggler, pos) {
            return None;
        }

        self.body_positions[juggler_index][pos][index].map(|coord| BodyPosition {
            x: coord.x,
            y: coord.y,
            z: coord.z,
            t: 0.0,
            angle: coord.angle,
            juggler,
        })
    }

    fn juggler_index(&self, juggler: usize) -> usize {
        juggler.saturating_sub(1) % self.number_of_jugglers
    }
}

fn parse_beat(beat_str: &str) -> Result<Vec<Option<BodyCoordinate>>, String> {
    let chars = beat_str.chars().collect::<Vec<_>>();
    let mut coord_tokens = Vec::new();
    let mut pos = 0usize;

    while pos < chars.len() {
        match chars[pos] {
            ch if ch.is_whitespace() => pos += 1,
            '-' => {
                coord_tokens.push(None);
                pos += 1;
            }
            '(' => {
                let Some(close_index) = chars[pos + 1..]
                    .iter()
                    .position(|ch| *ch == ')')
                    .map(|offset| pos + 1 + offset)
                else {
                    return Err("Missing ')' in body parameter".to_string());
                };
                let coord_str = chars[pos + 1..close_index].iter().collect::<String>();
                coord_tokens.push(Some(parse_coordinate(&coord_str)?));
                pos = close_index + 1;
            }
            ch => {
                return Err(format!("Invalid character in body parameter: {ch}"));
            }
        }
    }

    Ok(coord_tokens)
}

fn parse_coordinate(coord_str: &str) -> Result<BodyCoordinate, String> {
    let parts = coord_str
        .split(',')
        .map(|part| parse_finite_double(part.trim()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Invalid coordinate in body parameter".to_string())?;

    Ok(BodyCoordinate {
        angle: parts.first().copied().unwrap_or(0.0),
        x: parts.get(1).copied().unwrap_or(0.0),
        y: parts.get(2).copied().unwrap_or(0.0),
        z: parts.get(3).copied().unwrap_or(100.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_body_period_1() {
        let body = MhnBody::parse("(0)...(90)...(180)...(270)...").unwrap();
        assert_eq!(body.get_period(1), 12);
    }

    #[test]
    fn parses_body_period_2() {
        let body = MhnBody::parse("<(0)...(90)...(180)...(270)...|(0)...(90)...>").unwrap();
        assert_eq!(body.get_period(1), 12);
        assert_eq!(body.get_period(2), 6);
    }

    #[test]
    fn parses_body_positions() {
        let body = MhnBody::parse("(45,10,20,130)-.").unwrap();
        assert_eq!(body.get_number_of_positions(1, 0), 2);
        assert_eq!(
            body.get_position(1, 0, 0),
            Some(BodyPosition {
                x: 10.0,
                y: 20.0,
                z: 130.0,
                t: 0.0,
                angle: 45.0,
                juggler: 1,
            })
        );
        assert_eq!(body.get_position(1, 0, 1), None);
    }

    #[test]
    fn empty_beat_is_resting_position() {
        let body = MhnBody::parse(".").unwrap();
        assert_eq!(body.get_period(1), 1);
        assert_eq!(body.get_number_of_positions(1, 0), 1);
        assert_eq!(body.get_position(1, 0, 0), None);
    }
}
