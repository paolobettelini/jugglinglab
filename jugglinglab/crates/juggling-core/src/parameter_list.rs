use std::fmt;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParameterList {
    names: Vec<String>,
    values: Vec<String>,
}

impl ParameterList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(source: Option<&str>) -> Result<Self, String> {
        let mut list = Self::new();
        list.read_parameters(source)?;
        Ok(list)
    }

    pub fn number_of_parameters(&self) -> usize {
        self.names.len()
    }

    pub fn add_parameter(&mut self, name: impl Into<String>, value: impl Into<String>) -> bool {
        let name = name.into();
        let value = value.into();

        for index in (0..self.number_of_parameters()).rev() {
            if name.eq_ignore_ascii_case(&self.names[index]) {
                self.values[index] = value;
                return true;
            }
        }

        self.names.push(name);
        self.values.push(value);
        false
    }

    pub fn get_parameter(&self, name: &str) -> Option<&str> {
        for index in (0..self.number_of_parameters()).rev() {
            if name.eq_ignore_ascii_case(&self.names[index]) {
                return Some(&self.values[index]);
            }
        }
        None
    }

    pub fn remove_parameter(&mut self, name: &str) -> Option<String> {
        for index in (0..self.number_of_parameters()).rev() {
            if name.eq_ignore_ascii_case(&self.names[index]) {
                self.names.remove(index);
                return Some(self.values.remove(index));
            }
        }
        None
    }

    pub fn get_parameter_name(&self, index: usize) -> Option<&str> {
        self.names.get(index).map(String::as_str)
    }

    pub fn get_parameter_value(&self, index: usize) -> Option<&str> {
        self.values.get(index).map(String::as_str)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.names
            .iter()
            .map(String::as_str)
            .zip(self.values.iter().map(String::as_str))
    }

    pub fn read_parameters(&mut self, source: Option<&str>) -> Result<(), String> {
        let Some(source) = source else {
            return Ok(());
        };
        let clean_source = source.replace(['\n', '\r'], "");

        for token in clean_source.split(';') {
            if let Some(index) = token.find('=').filter(|index| *index > 0) {
                let name = token[..index].trim();
                let value = token[index + 1..].trim();
                if !name.is_empty() {
                    self.add_parameter(name, value);
                }
            } else {
                let name = token.trim();
                if !name.is_empty() {
                    return Err(format!("Parameter without a value: {name}"));
                }
            }
        }

        Ok(())
    }

    pub fn error_if_parameters_left(&self) -> Result<(), String> {
        match self.number_of_parameters() {
            0 => Ok(()),
            1 => Err(format!("Unused parameter: \"{}\"", self.names[0])),
            _ => Err(format!(
                "Unused parameters: {}",
                self.names
                    .iter()
                    .map(|name| format!("\"{name}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

impl fmt::Display for ParameterList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for index in 0..self.number_of_parameters() {
            if index != 0 {
                write!(f, ";")?;
            }
            write!(f, "{}={}", self.names[index], self.values[index])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_replaces_parameters_case_insensitively() {
        let list = ParameterList::parse(Some("pattern=3;BPS=4;bps=5")).unwrap();
        assert_eq!(list.number_of_parameters(), 2);
        assert_eq!(list.get_parameter("bps"), Some("5"));
        assert_eq!(list.to_string(), "pattern=3;BPS=5");
    }

    #[test]
    fn removes_parameters() {
        let mut list = ParameterList::parse(Some("pattern=531;title=Cascade")).unwrap();
        assert_eq!(list.remove_parameter("TITLE"), Some("Cascade".to_string()));
        assert_eq!(list.to_string(), "pattern=531");
    }

    #[test]
    fn rejects_tokens_without_values() {
        let err = ParameterList::parse(Some("pattern=3;broken")).unwrap_err();
        assert!(err.contains("broken"));
    }
}
