use crate::util::{
    expand_repeats, parse_finite_double, split_on_char_outside_parens, to_string_rounded,
};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coordinate {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MhnHands {
    config: String,
    number_of_jugglers: usize,
    number_of_beats: Vec<usize>,
    number_of_coords_per_beat: Vec<Vec<usize>>,
    catch_index_per_beat: Vec<Vec<usize>>,
    hand_coords: Vec<Vec<Vec<Option<Coordinate>>>>,
}

#[derive(Clone, Debug, PartialEq)]
struct BeatHands {
    coords: Vec<Option<Coordinate>>,
    catch_index: usize,
}

impl MhnHands {
    pub fn parse(config: &str) -> Result<Self, String> {
        let clean_str = expand_repeats(config)
            .chars()
            .filter(|ch| !matches!(ch, '<' | '>' | '{' | '}'))
            .collect::<String>();
        let juggler_strings = clean_str
            .split(['|', '!'])
            .map(str::to_string)
            .collect::<Vec<_>>();

        let mut parsed_jugglers = Vec::with_capacity(juggler_strings.len());
        for juggler_str in &juggler_strings {
            let mut parsed_beats = Vec::new();
            for beat_str in split_on_char_outside_parens(juggler_str.trim(), '.') {
                parsed_beats.push(parse_beat(&beat_str)?);
            }
            parsed_jugglers.push(parsed_beats);
        }

        let number_of_jugglers = parsed_jugglers.len();
        if number_of_jugglers == 0 {
            return Err("Empty hands parameter".to_string());
        }

        let number_of_beats = parsed_jugglers.iter().map(Vec::len).collect::<Vec<_>>();
        let number_of_coords_per_beat = parsed_jugglers
            .iter()
            .map(|beats| beats.iter().map(|beat| beat.coords.len()).collect())
            .collect::<Vec<_>>();
        let catch_index_per_beat = parsed_jugglers
            .iter()
            .map(|beats| beats.iter().map(|beat| beat.catch_index).collect())
            .collect::<Vec<_>>();
        let hand_coords = parsed_jugglers
            .into_iter()
            .map(|beats| beats.into_iter().map(|beat| beat.coords).collect())
            .collect::<Vec<_>>();

        Ok(Self {
            config: config.to_string(),
            number_of_jugglers,
            number_of_beats,
            number_of_coords_per_beat,
            catch_index_per_beat,
            hand_coords,
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

    pub fn get_number_of_coordinates(&self, juggler: usize, beat: usize) -> usize {
        self.number_of_coords_per_beat[self.juggler_index(juggler)]
            .get(beat)
            .copied()
            .unwrap_or(0)
    }

    pub fn get_catch_index(&self, juggler: usize, beat: usize) -> usize {
        self.catch_index_per_beat[self.juggler_index(juggler)]
            .get(beat)
            .copied()
            .unwrap_or(0)
    }

    pub fn get_coordinate(&self, juggler: usize, beat: usize, index: usize) -> Option<Coordinate> {
        let juggler_index = self.juggler_index(juggler);
        if beat >= self.get_period(juggler)
            || index >= self.get_number_of_coordinates(juggler, beat)
        {
            return None;
        }
        self.hand_coords[juggler_index][beat][index]
    }

    pub fn to_jugglinglab_string(&self) -> String {
        self.to_string()
    }

    fn juggler_index(&self, juggler: usize) -> usize {
        juggler.saturating_sub(1) % self.number_of_jugglers
    }
}

impl fmt::Display for MhnHands {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.number_of_jugglers > 1 {
            write!(f, "<")?;
        }

        for juggler_index in 0..self.number_of_jugglers {
            if juggler_index != 0 {
                write!(f, "|")?;
            }

            for beat_index in 0..self.number_of_beats[juggler_index] {
                let catch_index = self.catch_index_per_beat[juggler_index][beat_index];
                let coord_count = self.number_of_coords_per_beat[juggler_index][beat_index];

                for coord_index in 0..coord_count {
                    if coord_index == catch_index && coord_index != coord_count - 1 {
                        write!(f, "c")?;
                    }

                    if let Some(coord) = self.hand_coords[juggler_index][beat_index][coord_index] {
                        let c0 = to_string_rounded(coord.x, 4);
                        let c1 = to_string_rounded(coord.z, 4);
                        let c2 = to_string_rounded(coord.y, 4);
                        write!(f, "({c0}")?;
                        if c1 != "0" || c2 != "0" {
                            write!(f, ",{c1}")?;
                        }
                        if c2 != "0" {
                            write!(f, ",{c2}")?;
                        }
                        write!(f, ")")?;
                    } else {
                        write!(f, "-")?;
                    }
                }
                write!(f, ".")?;
            }
        }

        if self.number_of_jugglers > 1 {
            write!(f, ">")?;
        }
        Ok(())
    }
}

fn parse_beat(beat_str: &str) -> Result<BeatHands, String> {
    let chars = beat_str.chars().collect::<Vec<_>>();
    let mut coord_tokens = Vec::new();
    let mut catch_index = None;
    let mut got_throw = false;
    let mut pos = 0usize;

    while pos < chars.len() {
        match chars[pos] {
            ch if ch.is_whitespace() => pos += 1,
            '-' => {
                coord_tokens.push(None);
                pos += 1;
            }
            'T' | 't' => {
                if !coord_tokens.is_empty() {
                    return Err(
                        "In the hands parameter, t must be at the start of a beat".to_string()
                    );
                }
                if got_throw {
                    return Err("Too many throw markers in hands parameter".to_string());
                }
                got_throw = true;
                pos += 1;
            }
            'C' | 'c' => {
                if coord_tokens.is_empty() {
                    return Err(
                        "In the hands parameter, c cannot be at the start of a beat".to_string()
                    );
                }
                if catch_index.is_some() {
                    return Err("Too many catch markers in hands parameter".to_string());
                }
                catch_index = Some(coord_tokens.len());
                pos += 1;
            }
            '(' => {
                let Some(close_index) = chars[pos + 1..]
                    .iter()
                    .position(|ch| *ch == ')')
                    .map(|offset| pos + 1 + offset)
                else {
                    return Err("Missing ')' in hands parameter".to_string());
                };
                let coord_str = chars[pos + 1..close_index].iter().collect::<String>();
                coord_tokens.push(Some(parse_coordinate(&coord_str)?));
                pos = close_index + 1;
            }
            ch => {
                return Err(format!("Invalid character in hands parameter: {ch}"));
            }
        }
    }

    if coord_tokens.len() < 2 {
        return Err("The hands parameter needs at least two coordinates per beat".to_string());
    }
    if coord_tokens[0].is_none() {
        return Err("The hands parameter is missing the throw coordinate".to_string());
    }

    let final_catch_index = catch_index.unwrap_or(coord_tokens.len() - 1);
    if final_catch_index >= coord_tokens.len() || coord_tokens[final_catch_index].is_none() {
        return Err("The hands parameter is missing the catch coordinate".to_string());
    }

    Ok(BeatHands {
        coords: coord_tokens,
        catch_index: final_catch_index,
    })
}

fn parse_coordinate(coord_str: &str) -> Result<Coordinate, String> {
    let parts = coord_str
        .split(',')
        .map(|part| parse_finite_double(part.trim()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Invalid coordinate in hands parameter".to_string())?;

    Ok(Coordinate {
        x: parts.first().copied().unwrap_or(0.0),
        y: parts.get(2).copied().unwrap_or(0.0),
        z: parts.get(1).copied().unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hands_1() {
        let hands = MhnHands::parse("(30)-(22,20)c(15).").unwrap();
        assert_eq!(hands.to_string(), "(30)-(22,20)(15).");
        assert_eq!(hands.get_period(1), 1);
        assert_eq!(hands.get_number_of_coordinates(1, 0), 4);
        assert_eq!(hands.get_catch_index(1, 0), 3);
    }

    #[test]
    fn parses_hands_2() {
        let hands = MhnHands::parse("(-30)-(22,0,-5)c(15)-.").unwrap();
        assert_eq!(hands.to_string(), "(-30)-(22,0,-5)c(15)-.");
        assert_eq!(
            hands.get_coordinate(1, 0, 2),
            Some(Coordinate {
                x: 22.0,
                y: -5.0,
                z: 0.0
            })
        );
    }

    #[test]
    fn parses_hands_3() {
        let config = "<t(10)c(32.5)(0,45,-25).|(-30)(2.5).(30)(-2.5).(-30)(0).>";
        let hands = MhnHands::parse(config).unwrap();
        assert_eq!(
            hands.to_string(),
            "<(10)c(32.5)(0,45,-25).|(-30)(2.5).(30)(-2.5).(-30)(0).>"
        );
        assert_eq!(hands.number_of_jugglers(), 2);
        assert_eq!(hands.get_period(2), 3);
    }

    #[test]
    fn expands_repeated_beats_before_parsing() {
        let hands = MhnHands::parse("((30)(-30).)^2").unwrap();
        assert_eq!(hands.to_string(), "(30)(-30).(30)(-30).");
    }

    #[test]
    fn rejects_invalid_placeholders() {
        let err = MhnHands::parse("-(30).").unwrap_err();
        assert!(err.contains("throw"));
    }
}
