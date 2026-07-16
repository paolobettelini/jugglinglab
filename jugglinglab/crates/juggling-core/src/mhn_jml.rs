use crate::jml::{CURRENT_JML_VERSION, escape_xml, validate_jml_tree};
use crate::mhn_body::BodyPosition;
use crate::mhn_symmetry::MhnSymmetryType;
use crate::parameter_list::ParameterList;
use crate::permutation::{Permutation, lcm};
use crate::prop::{PropSpec, decode_image_source, encode_image_source};
use crate::util::{expand_repeats, to_string_rounded};
use roxmltree::{Document, Node};

#[derive(Clone, Debug, PartialEq)]
pub struct MhnJmlPattern {
    pub title: Option<String>,
    pub info: Option<String>,
    pub tags: Vec<String>,
    pub base_pattern_notation: Option<String>,
    pub base_pattern_config: Option<String>,
    pub number_of_jugglers: usize,
    pub number_of_paths: usize,
    pub props: Vec<MhnJmlProp>,
    pub prop_assignment: Vec<usize>,
    pub period_secs: f64,
    pub events: Vec<MhnJmlEvent>,
    pub positions: Vec<BodyPosition>,
    pub symmetries: Vec<MhnJmlSymmetry>,
    pub path_events: Vec<Vec<MhnPathEvent>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MhnJmlEvent {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub t: f64,
    pub juggler: usize,
    pub hand: usize,
    pub calcpos: bool,
    pub transitions: Vec<MhnJmlTransition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MhnJmlTransition {
    pub transition_type: MhnJmlTransitionType,
    pub path: usize,
    pub throw_type: Option<String>,
    pub throw_mod: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MhnJmlTransitionType {
    Throw,
    Catch,
    SoftCatch,
    GrabCatch,
    Holding,
}

impl MhnJmlTransitionType {
    pub fn is_catch(self) -> bool {
        matches!(self, Self::Catch | Self::SoftCatch | Self::GrabCatch)
    }

    pub fn is_throw_or_catch(self) -> bool {
        matches!(
            self,
            Self::Throw | Self::Catch | Self::SoftCatch | Self::GrabCatch
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MhnJmlSymmetry {
    pub symmetry_type: MhnSymmetryType,
    pub number_of_jugglers: usize,
    pub number_of_paths: usize,
    pub juggler_perm: Permutation,
    pub path_perm: Permutation,
    pub delay: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MhnJmlProp {
    pub prop_type: String,
    pub modifier: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MhnPathEvent {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub transition_type: MhnJmlTransitionType,
}

impl MhnJmlPattern {
    pub fn new(number_of_jugglers: usize, number_of_paths: usize, period_secs: f64) -> Self {
        Self {
            title: None,
            info: None,
            tags: Vec::new(),
            base_pattern_notation: None,
            base_pattern_config: None,
            number_of_jugglers,
            number_of_paths,
            props: Vec::new(),
            prop_assignment: Vec::new(),
            period_secs,
            events: Vec::new(),
            positions: Vec::new(),
            symmetries: Vec::new(),
            path_events: vec![Vec::new(); number_of_paths],
        }
    }

    pub fn from_jml_xml(xml: &str) -> Result<Self, String> {
        let wrapped = if xml.trim_start().starts_with("<pattern") {
            format!("<jml version=\"{CURRENT_JML_VERSION}\">{xml}</jml>")
        } else {
            strip_doctype(xml)
        };
        let doc = Document::parse(&wrapped).map_err(|err| format!("Invalid pattern JML: {err}"))?;
        let root = validate_jml_tree(&doc)?;
        let loading_version = root.attribute("version").unwrap_or(CURRENT_JML_VERSION);
        let pattern_node = root
            .children()
            .find(|node| node.has_tag_name("pattern"))
            .ok_or_else(|| "Missing <pattern> tag".to_string())?;
        let setup = pattern_node
            .children()
            .find(|node| node.has_tag_name("setup"))
            .ok_or_else(|| "Missing <setup> tag".to_string())?;

        let number_of_jugglers = parse_usize_attribute(setup, "jugglers", Some(1), "setup")?;
        let number_of_paths = parse_usize_attribute(setup, "paths", None, "setup")?;
        let mut model = Self::new(number_of_jugglers, number_of_paths, 0.0);

        model.title = pattern_node
            .children()
            .find(|node| node.has_tag_name("title"))
            .and_then(|node| node.text())
            .map(|text| text.replace(';', "").trim().to_string())
            .filter(|text| !text.is_empty());
        if let Some(info) = pattern_node
            .children()
            .find(|node| node.has_tag_name("info"))
        {
            model.info = info
                .text()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string);
            model.tags = info
                .attribute("tags")
                .map(|tags| {
                    tags.split(',')
                        .map(str::trim)
                        .filter(|tag| !tag.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
        }

        if let Some(base) = pattern_node
            .children()
            .find(|node| node.has_tag_name("basepattern"))
        {
            model.base_pattern_notation = Some(
                base.attribute("notation")
                    .ok_or_else(|| "Invalid JML: basepattern notation is required".to_string())?
                    .to_ascii_lowercase(),
            );
            model.base_pattern_config = Some(
                base.text()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .ok_or_else(|| "Invalid JML: basepattern config is empty".to_string())?
                    .to_string(),
            );
        }

        for prop in pattern_node
            .children()
            .filter(|node| node.has_tag_name("prop"))
        {
            let prop_type = prop.attribute("type").unwrap_or("ball").to_string();
            let modifier = prop.attribute("mod").map(str::to_string);
            PropSpec::from_jml(&prop_type, modifier.as_deref())?;
            model.props.push(MhnJmlProp::new(prop_type, modifier));
        }
        model.prop_assignment =
            parse_prop_assignment(setup.attribute("props"), number_of_paths, model.props.len())?;

        for symmetry in pattern_node
            .children()
            .filter(|node| node.has_tag_name("symmetry"))
        {
            model.symmetries.push(parse_jml_symmetry(
                symmetry,
                number_of_jugglers,
                number_of_paths,
            )?);
        }
        model.period_secs = model
            .symmetries
            .iter()
            .find(|symmetry| symmetry.symmetry_type == MhnSymmetryType::Delay)
            .map(|symmetry| symmetry.delay)
            .unwrap_or(0.0);

        for position in pattern_node
            .children()
            .filter(|node| node.has_tag_name("position"))
        {
            if position.children().any(|node| node.is_element()) {
                return Err("Invalid JML: position cannot contain subtags".to_string());
            }
            model.positions.push(BodyPosition {
                x: parse_f64_attribute(position, "x", Some(0.0), "position")?,
                y: parse_f64_attribute(position, "y", Some(0.0), "position")?,
                z: parse_f64_attribute(position, "z", Some(0.0), "position")?,
                t: parse_f64_attribute(position, "t", Some(0.0), "position")?,
                angle: parse_f64_attribute(position, "angle", Some(0.0), "position")?,
                juggler: parse_usize_attribute(position, "juggler", Some(1), "position")?,
            });
        }

        for event in pattern_node
            .children()
            .filter(|node| node.has_tag_name("event"))
        {
            let hand_attribute = event
                .attribute("hand")
                .ok_or_else(|| "Invalid JML: event hand is required".to_string())?;
            let (juggler, hand) = parse_hand(hand_attribute)?;
            if juggler == 0 || juggler > number_of_jugglers {
                return Err("Invalid JML: event juggler out of range".to_string());
            }
            let x = parse_f64_attribute(event, "x", Some(0.0), "event")?;
            let mut y = parse_f64_attribute(event, "y", Some(0.0), "event")?;
            let mut z = parse_f64_attribute(event, "z", Some(0.0), "event")?;
            if loading_version == "1.0" {
                std::mem::swap(&mut y, &mut z);
            }
            let mut parsed_event = MhnJmlEvent {
                x,
                y,
                z,
                t: parse_f64_attribute(event, "t", Some(0.0), "event")?,
                juggler,
                hand,
                calcpos: event
                    .attribute("calcpos")
                    .is_some_and(|value| value.eq_ignore_ascii_case("true")),
                transitions: Vec::new(),
            };

            for transition in event.children().filter(|node| node.is_element()) {
                parsed_event
                    .transitions
                    .push(parse_jml_transition(transition, number_of_paths)?);
            }
            model.events.push(parsed_event);
        }

        model.sort_events();
        model.rebuild_path_events();
        model.assert_valid()?;
        Ok(model)
    }

    pub fn rebuild_path_events(&mut self) {
        self.path_events = vec![Vec::new(); self.number_of_paths];
        for event in &self.events {
            for transition in &event.transitions {
                if transition.path == 0 || transition.path > self.number_of_paths {
                    continue;
                }
                self.path_events[transition.path - 1].push(MhnPathEvent {
                    t: event.t,
                    x: event.x,
                    y: event.y,
                    z: event.z,
                    transition_type: transition.transition_type,
                });
            }
        }
        for path in &mut self.path_events {
            path.sort_by(|a, b| a.t.total_cmp(&b.t));
        }
    }

    pub fn with_scaled_time(&self, scale: f64) -> Self {
        let mut scaled = self.clone();
        scaled.period_secs *= scale;
        for symmetry in &mut scaled.symmetries {
            if symmetry.delay > 0.0 {
                symmetry.delay *= scale;
            }
        }
        for position in &mut scaled.positions {
            position.t *= scale;
        }
        for event in &mut scaled.events {
            event.t *= scale;
        }
        scaled.rebuild_path_events();
        scaled
    }

    pub fn with_inverted_x_axis(&self, flip_x_coordinate: bool) -> Self {
        let mut inverted = self.clone();
        for event in &mut inverted.events {
            event.hand = 1 - event.hand;
            if flip_x_coordinate {
                event.x = -event.x;
            }
        }
        inverted.sort_events();
        inverted.rebuild_path_events();
        inverted
    }

    pub fn with_inverted_time(&self) -> Result<Self, String> {
        let loop_end = self.loop_end_time()?;
        let mut inverse_events = Vec::with_capacity(self.events.len());

        for event in &self.events {
            let mut inverse = event.clone();
            inverse.t = loop_end - event.t;
            inverse.transitions = event
                .transitions
                .iter()
                .map(|transition| match transition.transition_type {
                    MhnJmlTransitionType::Throw => Ok(MhnJmlTransition {
                        transition_type: MhnJmlTransitionType::Catch,
                        path: transition.path,
                        throw_type: None,
                        throw_mod: None,
                    }),
                    MhnJmlTransitionType::Catch
                    | MhnJmlTransitionType::SoftCatch
                    | MhnJmlTransitionType::GrabCatch => {
                        let source = self.previous_transition_for_path(event.t, transition.path)?;
                        if source.transition_type != MhnJmlTransitionType::Throw {
                            return Err("Error while inverting pattern time: prior transition is not a throw".to_string());
                        }
                        Ok(source)
                    }
                    MhnJmlTransitionType::Holding => Ok(transition.clone()),
                })
                .collect::<Result<Vec<_>, String>>()?;
            inverse_events.push(inverse);
        }

        let mut inverted = self.clone();
        for position in &mut inverted.positions {
            position.t = if (position.t - self.loop_start_time()).abs() > 1e-9 {
                loop_end - position.t
            } else {
                self.loop_start_time()
            };
        }
        for symmetry in &mut inverted.symmetries {
            if symmetry.symmetry_type != MhnSymmetryType::Switch {
                symmetry.path_perm = symmetry.path_perm.inverse();
            }
        }
        inverted.events = inverse_events;
        inverted.select_primary_events()?;
        inverted.sort_events();
        inverted.rebuild_path_events();
        Ok(inverted)
    }

    fn previous_transition_for_path(
        &self,
        before_time: f64,
        path: usize,
    ) -> Result<MhnJmlTransition, String> {
        let loop_time = self.loop_end_time()? - self.loop_start_time();
        let time_window = self.path_permutation()?.max_order().max(1) as f64 * loop_time;
        self.event_images_between(before_time - time_window, before_time)?
            .into_iter()
            .rev()
            .filter(|image| image.event.t < before_time - 1e-9)
            .find_map(|image| {
                image
                    .event
                    .transitions
                    .into_iter()
                    .find(|transition| transition.path == path)
            })
            .ok_or_else(|| {
                format!("Error while inverting pattern time: no prior transition for path {path}")
            })
    }

    pub fn write_jml(&self, write_title: bool, write_info: bool) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\"?>\n");
        out.push_str("<!DOCTYPE jml SYSTEM \"file://jml.dtd\">\n");
        out.push_str("<jml version=\"3\">\n");
        out.push_str("<pattern>\n");

        if write_title {
            if let Some(title) = self.title.as_deref().filter(|title| !title.is_empty()) {
                out.push_str(&format!("<title>{}</title>\n", escape_xml(title)));
            }
        }

        if write_info && (self.info.is_some() || !self.tags.is_empty()) {
            let tags = self.tags.join(",");
            match (self.info.as_deref(), tags.is_empty()) {
                (Some(info), true) => {
                    out.push_str(&format!("<info>{}</info>\n", escape_xml(info)));
                }
                (Some(info), false) => {
                    out.push_str(&format!(
                        "<info tags=\"{}\">{}</info>\n",
                        escape_xml(&tags),
                        escape_xml(info)
                    ));
                }
                (None, false) => {
                    out.push_str(&format!("<info tags=\"{}\"/>\n", escape_xml(&tags)));
                }
                (None, true) => {}
            }
        }

        if let (Some(notation), Some(config)) = (
            self.base_pattern_notation.as_deref(),
            self.base_pattern_config.as_deref(),
        ) {
            out.push_str(&format!(
                "<basepattern notation=\"{}\">\n",
                escape_xml(&notation.to_lowercase())
            ));
            out.push_str(&escape_xml(&format_base_pattern_config(config)));
            out.push('\n');
            out.push_str("</basepattern>\n");
        }

        for prop in &self.props {
            out.push_str(&prop.to_jml());
        }

        out.push_str(&format!(
            "<setup jugglers=\"{}\" paths=\"{}\" props=\"{}\"/>\n",
            self.number_of_jugglers,
            self.number_of_paths,
            self.prop_assignment
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));

        for symmetry in &self.symmetries {
            out.push_str(&symmetry.to_jml());
        }

        let mut positions = self.positions.clone();
        positions.sort_by(compare_positions);
        for position in &positions {
            out.push_str(&position_to_jml(position));
        }

        let mut events = self.events.clone();
        events.sort_by(compare_events);
        for event in &events {
            out.push_str(&event.to_jml());
        }

        out.push_str("</pattern>\n");
        out.push_str("</jml>\n");
        out
    }

    pub fn assert_valid(&self) -> Result<(), String> {
        if self.number_of_jugglers < 1 {
            return Err("Invalid JML: invalid juggler count".to_string());
        }
        if self.prop_assignment.len() != self.number_of_paths {
            return Err("Invalid JML: wrong number of prop assignments".to_string());
        }
        if self.number_of_paths > 0 && self.props.is_empty() {
            return Err("Invalid JML: paths exist without a prop definition".to_string());
        }
        for prop in &self.props {
            PropSpec::from_jml(&prop.prop_type, prop.modifier.as_deref())?;
        }
        for assigned in &self.prop_assignment {
            if *assigned == 0 || *assigned > self.props.len() {
                return Err(format!(
                    "Invalid JML: prop assignment {assigned} is out of range"
                ));
            }
        }
        for position in &self.positions {
            if position.juggler == 0 || position.juggler > self.number_of_jugglers {
                return Err(format!(
                    "Invalid JML: position juggler {} is out of range",
                    position.juggler
                ));
            }
            if ![
                position.x,
                position.y,
                position.z,
                position.t,
                position.angle,
            ]
            .into_iter()
            .all(f64::is_finite)
            {
                return Err("Invalid JML: position has a non-finite coordinate".to_string());
            }
        }
        for event in &self.events {
            if event.juggler == 0 || event.juggler > self.number_of_jugglers {
                return Err(format!(
                    "Invalid JML: event juggler {} is out of range",
                    event.juggler
                ));
            }
            if event.hand > 1 {
                return Err(format!("Invalid JML: event hand {} is invalid", event.hand));
            }
            if ![event.x, event.y, event.z, event.t]
                .into_iter()
                .all(f64::is_finite)
            {
                return Err("Invalid JML: event has a non-finite coordinate".to_string());
            }
        }
        for symmetry in &self.symmetries {
            if symmetry.number_of_jugglers != self.number_of_jugglers
                || symmetry.juggler_perm.size() != self.number_of_jugglers
                || symmetry.number_of_paths != self.number_of_paths
                || symmetry.path_perm.size() != self.number_of_paths
            {
                return Err("Invalid JML: symmetry dimensions do not match setup".to_string());
            }
            if !symmetry.delay.is_finite() {
                return Err("Invalid JML: symmetry delay is not finite".to_string());
            }
        }
        if self
            .symmetries
            .iter()
            .filter(|symmetry| symmetry.symmetry_type == MhnSymmetryType::Delay)
            .count()
            != 1
        {
            return Err("Invalid JML: exactly one delay symmetry is required".to_string());
        }

        let loop_end = self.loop_end_time()?;
        if loop_end < 0.001 {
            return Err("Invalid JML: loop time is too small".to_string());
        }

        let delay_perm = self.path_permutation()?;
        for symmetry in &self.symmetries {
            match symmetry.symmetry_type {
                MhnSymmetryType::SwitchDelay => {
                    if symmetry.path_perm.composed_with(Some(&symmetry.path_perm)) != *delay_perm {
                        return Err("Invalid JML: inconsistent switchdelay pperm".to_string());
                    }
                    if symmetry.juggler_perm.order() != 2 {
                        return Err("Invalid JML: inconsistent switchdelay jperm".to_string());
                    }
                }
                MhnSymmetryType::Switch => {
                    for path in 1..=self.number_of_paths {
                        if symmetry.path_perm.order_of(path as i32) != 2 {
                            return Err("Invalid JML: inconsistent switch pperm".to_string());
                        }
                    }
                    if symmetry.juggler_perm.order() != 2 {
                        return Err("Invalid JML: inconsistent switch jperm".to_string());
                    }
                }
                MhnSymmetryType::Delay => {}
            }
        }

        if self.number_of_paths != 0 {
            for prop in 1..=self.props.len() {
                if !self.prop_assignment.contains(&prop) {
                    return Err(format!("Invalid JML: prop {prop} is not assigned"));
                }
            }
        }

        let mut path_is_held = vec![None; self.number_of_paths];
        for image in self.all_event_images()? {
            for transition in &image.event.transitions {
                if transition.path == 0 || transition.path > self.number_of_paths {
                    return Err(format!(
                        "Invalid JML: path {} out of range",
                        transition.path
                    ));
                }
                let path_index = transition.path - 1;
                if transition.transition_type == MhnJmlTransitionType::Throw {
                    if path_is_held[path_index] == Some(false) {
                        return Err(format!(
                            "Invalid JML: two consecutive throws on path {}",
                            transition.path
                        ));
                    }
                    path_is_held[path_index] = Some(false);
                } else if transition.transition_type.is_catch() {
                    if path_is_held[path_index] == Some(true) {
                        return Err(format!(
                            "Invalid JML: two consecutive catches on path {}",
                            transition.path
                        ));
                    }
                    path_is_held[path_index] = Some(true);
                } else {
                    match transition.transition_type {
                        MhnJmlTransitionType::Holding => {
                            if path_is_held[path_index] == Some(false) {
                                return Err(format!(
                                    "Invalid JML: holding after throw on path {}",
                                    transition.path
                                ));
                            }
                            path_is_held[path_index] = Some(true);
                        }
                        MhnJmlTransitionType::Throw
                        | MhnJmlTransitionType::Catch
                        | MhnJmlTransitionType::SoftCatch
                        | MhnJmlTransitionType::GrabCatch => unreachable!(),
                    }
                }
            }
        }

        for path in 1..=self.number_of_paths {
            if path_is_held[path - 1].is_none() {
                return Err(format!("Invalid JML: path {path} has no events"));
            }
        }

        for juggler in 1..=self.number_of_jugglers {
            let mut positions = self
                .positions
                .iter()
                .filter(|position| position.juggler == juggler)
                .copied()
                .collect::<Vec<_>>();
            positions.sort_by(compare_positions);
            for index in 0..positions.len() {
                let position = positions[index];
                if position.t < self.loop_start_time() || position.t >= loop_end {
                    return Err(format!(
                        "Invalid JML: position outside loop for juggler {juggler}"
                    ));
                }
                let next = if index < positions.len() - 1 {
                    positions[index + 1].t
                } else {
                    positions[0].t + loop_end - position.t
                };
                let gap = if index < positions.len() - 1 {
                    next - position.t
                } else {
                    next
                };
                if gap < 0.001 {
                    return Err(format!(
                        "Invalid JML: positions too close for juggler {juggler}"
                    ));
                }
            }
        }

        let all_events = self.all_event_images()?;
        for juggler in 1..=self.number_of_jugglers {
            for hand in 0..2 {
                let hand_events = all_events
                    .iter()
                    .filter(|image| image.event.juggler == juggler && image.event.hand == hand)
                    .collect::<Vec<_>>();
                for pair in hand_events.windows(2) {
                    if pair[1].event.t - pair[0].event.t < 0.001 {
                        return Err(format!(
                            "Invalid JML: events too close for hand {}:{}",
                            juggler,
                            hand_name(hand)
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn apply_prop_colors(&mut self, color_string: &str) -> Result<(), String> {
        if self.props.iter().any(|prop| !prop.is_colorable()) {
            return Err("setPropColors(): not colorable".to_string());
        }

        let color_list = self.color_list(color_string)?;
        let mut new_props: Vec<MhnJmlProp> = Vec::new();
        let mut new_prop_assignment = vec![1usize; self.number_of_paths];

        for path_index in 0..self.number_of_paths {
            let old_prop_index = self
                .prop_assignment
                .get(path_index)
                .copied()
                .unwrap_or(1)
                .saturating_sub(1);
            let old_prop = self
                .props
                .get(old_prop_index)
                .ok_or_else(|| "Invalid prop assignment".to_string())?;
            let mut params = ParameterList::parse(old_prop.modifier.as_deref())?;
            params.remove_parameter("color");
            params.add_parameter("color", &color_list[path_index % color_list.len()]);
            let modifier = (!params.to_string().is_empty()).then(|| params.to_string());
            let new_prop = MhnJmlProp {
                prop_type: old_prop.prop_type.clone(),
                modifier,
            };

            let assigned = match new_props.iter().position(|prop| prop == &new_prop) {
                Some(index) => index + 1,
                None => {
                    new_props.push(new_prop);
                    new_props.len()
                }
            };
            new_prop_assignment[path_index] = assigned;
        }

        self.props = new_props;
        self.prop_assignment = new_prop_assignment;
        Ok(())
    }

    fn color_list(&self, color_string: &str) -> Result<Vec<String>, String> {
        let trimmed = color_string.trim();
        match trimmed {
            "mixed" => Ok(PROP_COLOR_MIXED
                .iter()
                .map(|color| (*color).to_string())
                .collect()),
            "orbits" => {
                let delay_perm = self.path_permutation()?;
                let mut colors_by_orbit = vec![String::new(); self.number_of_paths];
                let mut color_index = 0usize;

                for path in 1..=self.number_of_paths {
                    if !colors_by_orbit[path - 1].is_empty() {
                        continue;
                    }
                    for mapped in delay_perm.cycle_of(path as i32) {
                        colors_by_orbit[mapped as usize - 1] =
                            PROP_COLOR_MIXED[color_index % PROP_COLOR_MIXED.len()].to_string();
                    }
                    color_index += 1;
                }
                Ok(colors_by_orbit)
            }
            "" => Err("Color string cannot be empty".to_string()),
            _ => expand_repeats(trimmed)
                .split('}')
                .filter(|token| !token.trim().is_empty())
                .map(|token| {
                    let color = token.replace('{', "").trim().to_string();
                    let parts = color.split(',').collect::<Vec<_>>();
                    match parts.len() {
                        1 => {
                            validate_prop_color_name(&color)?;
                            Ok(color)
                        }
                        3 => {
                            validate_prop_rgb(&parts, &color)?;
                            Ok(format!("{{{color}}}"))
                        }
                        _ => Err("Invalid color format".to_string()),
                    }
                })
                .collect(),
        }
    }

    pub fn sort_events(&mut self) {
        self.events.sort_by(|a, b| {
            a.t.total_cmp(&b.t)
                .then(a.juggler.cmp(&b.juggler))
                .then(a.hand.cmp(&b.hand))
        });
    }

    pub fn merge_coincident_events(&mut self) {
        self.sort_events();
        let mut merged: Vec<MhnJmlEvent> = Vec::with_capacity(self.events.len());

        for event in self.events.drain(..) {
            if let Some(last) = merged
                .last_mut()
                .filter(|last| events_are_coincident(last, &event))
            {
                if last.calcpos && !event.calcpos {
                    last.x = event.x;
                    last.y = event.y;
                    last.z = event.z;
                    last.calcpos = false;
                }
                last.transitions.extend(event.transitions);
            } else {
                merged.push(event);
            }
        }

        self.events = merged;
    }

    pub fn add_events_for_gaps(&mut self, max_gap_secs: f64) {
        if max_gap_secs <= 0.0 || self.period_secs <= 0.0 {
            return;
        }

        loop {
            let loop_start = self.loop_start_time();
            let Ok(loop_end) = self.loop_end_time() else {
                return;
            };
            let min_time = 2.0 * loop_start - loop_end;
            let max_time = 2.0 * loop_end - loop_start;
            let Ok(images) = self.event_images_between(min_time, max_time) else {
                return;
            };
            let mut additions = Vec::new();

            'scan: for hand in 0..2 {
                let mut start_events = vec![None::<MhnJmlEvent>; self.number_of_jugglers];

                for image in &images {
                    if image.event.hand != hand {
                        continue;
                    }
                    if image.event.t < min_time {
                        continue;
                    }
                    if image.event.t > max_time {
                        break;
                    }

                    let start = &mut start_events[image.event.juggler - 1];
                    if let Some(start_event) = start {
                        let gap = image.event.t - start_event.t;
                        if gap > max_gap_secs {
                            let num_add = (gap / max_gap_secs).floor() as usize;
                            let delta_t = gap / (num_add + 1) as f64;
                            for index in 1..=num_add {
                                additions.push(
                                    MhnJmlEvent::new(
                                        0.0,
                                        0.0,
                                        0.0,
                                        start_event.t + index as f64 * delta_t,
                                        image.event.juggler,
                                        hand,
                                    )
                                    .with_calcpos(true),
                                );
                            }
                            break 'scan;
                        }
                    }

                    *start = Some(image.event.clone());
                }
            }

            if additions.is_empty() {
                break;
            }
            self.events.extend(additions);
            self.sort_events();
        }
    }

    pub fn add_locations_for_incomplete_events(&mut self, resting_x: f64) -> Result<(), String> {
        if self.period_secs <= 0.0 {
            return Ok(());
        }

        for index in 0..self.events.len() {
            if !self.events[index].calcpos {
                continue;
            }

            let event = self.events[index].clone();
            let Some(start) = self.neighbor_position_event(&event, NeighborDirection::Previous)
            else {
                self.events[index].x = if event.hand == 0 {
                    resting_x
                } else {
                    -resting_x
                };
                self.events[index].y = 0.0;
                self.events[index].z = 0.0;
                self.events[index].calcpos = false;
                continue;
            };

            let Some(end) = self.neighbor_position_event(&event, NeighborDirection::Next) else {
                return Err("Error in addLocationsForIncompleteEventsToJml".to_string());
            };

            let duration = end.t - start.t;
            if duration.abs() < 1e-9 {
                return Err("Error in addLocationsForIncompleteEventsToJml".to_string());
            }

            let fraction = (event.t - start.t) / duration;
            self.events[index].x = start.x + fraction * (end.x - start.x);
            self.events[index].y = start.y + fraction * (end.y - start.y);
            self.events[index].z = start.z + fraction * (end.z - start.z);
            self.events[index].calcpos = false;
        }

        Ok(())
    }

    pub fn loop_start_time(&self) -> f64 {
        0.0
    }

    pub fn loop_end_time(&self) -> Result<f64, String> {
        self.symmetries
            .iter()
            .find(|symmetry| symmetry.symmetry_type == MhnSymmetryType::Delay)
            .map(|symmetry| symmetry.delay)
            .ok_or_else(|| "JML pattern missing delay symmetry".to_string())
    }

    pub fn path_permutation(&self) -> Result<&Permutation, String> {
        self.symmetries
            .iter()
            .find(|symmetry| symmetry.symmetry_type == MhnSymmetryType::Delay)
            .map(|symmetry| &symmetry.path_perm)
            .ok_or_else(|| "JML pattern missing path permutation".to_string())
    }

    pub fn event_images_between(
        &self,
        min_time: f64,
        max_time: f64,
    ) -> Result<Vec<MhnEventImage>, String> {
        let loop_time = self.loop_end_time()? - self.loop_start_time();
        if loop_time <= 0.0 {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        for primary_index in 0..self.events.len() {
            let images = MhnEventImages::new(self, primary_index)?;
            let loop_min = ((min_time - images.primary_time) / loop_time).floor() as isize - 2;
            let loop_max = ((max_time - images.primary_time) / loop_time).ceil() as isize + 2;

            for loop_index in loop_min..=loop_max {
                for juggler in 0..images.number_of_jugglers {
                    for hand in 0..2 {
                        for entry in 0..images.number_of_entries {
                            if images.entries[juggler][hand][entry].is_none() {
                                continue;
                            }
                            let image = images.event_image_at(juggler, hand, entry, loop_index);
                            if image.event.t >= min_time - 1e-9 && image.event.t <= max_time + 1e-9
                            {
                                result.push(image);
                            }
                        }
                    }
                }
            }
        }

        result.sort_by(|left, right| compare_events(&left.event, &right.event));
        Ok(result)
    }

    pub fn all_event_images(&self) -> Result<Vec<MhnEventImage>, String> {
        let loop_start = self.loop_start_time();
        let loop_end = self.loop_end_time()?;
        let loop_time = loop_end - loop_start;
        let time_window = self.path_permutation()?.max_order() as f64 * loop_time;
        self.event_images_between(loop_start - time_window, loop_end + time_window)
    }

    pub fn loop_event_images(&self) -> Result<Vec<MhnEventImage>, String> {
        let loop_start = self.loop_start_time();
        let loop_end = self.loop_end_time()?;
        Ok(self
            .all_event_images()?
            .into_iter()
            .filter(|image| {
                let truncated = truncated_time(image.event.t);
                truncated >= loop_start && truncated < loop_end
            })
            .collect())
    }

    pub fn select_primary_events(&mut self) -> Result<(), String> {
        let loop_events = self.loop_event_images()?;
        let mut selected = Vec::with_capacity(self.events.len());

        for primary_index in 0..self.events.len() {
            let image = loop_events
                .iter()
                .find(|image| image.primary_index == primary_index)
                .ok_or_else(|| "Error in selectPrimaryEvents".to_string())?;
            selected.push(image.event.clone());
        }

        self.events = selected;
        Ok(())
    }

    pub fn fix_holds(&mut self) -> Result<(), String> {
        if self.number_of_paths == 0 {
            return Ok(());
        }

        let mut holds_only = vec![false; self.number_of_paths];
        let mut finishing = false;

        for _ in 0..1000 {
            let loop_start = self.loop_start_time();
            let loop_end = self.loop_end_time()?;
            let loop_time = loop_end - loop_start;
            let time_window = self.path_permutation()?.max_order() as f64 * loop_time;
            let images = self.event_images_between(loop_start, loop_end + time_window)?;
            let mut holding_location = vec![HoldingLocation::Unknown; self.number_of_paths];
            let mut restart = false;

            for image in images {
                if image.event.t > loop_end + time_window {
                    break;
                }

                let mut paths_to_hold = holding_location
                    .iter()
                    .enumerate()
                    .filter_map(|(index, location)| {
                        (*location == HoldingLocation::Held(image.event.juggler, image.event.hand))
                            .then_some(index + 1)
                    })
                    .collect::<Vec<_>>();

                for transition in &image.event.transitions {
                    if transition.path == 0 || transition.path > self.number_of_paths {
                        return Err("error in fixHolds: path out of range".to_string());
                    }
                    paths_to_hold.retain(|path| *path != transition.path);
                    let location = holding_location[transition.path - 1];

                    if transition.transition_type.is_catch() {
                        if matches!(location, HoldingLocation::Held(_, _)) {
                            return Err("error 1 in fixHolds".to_string());
                        }
                        holding_location[transition.path - 1] =
                            HoldingLocation::Held(image.event.juggler, image.event.hand);
                    } else {
                        match transition.transition_type {
                            MhnJmlTransitionType::Throw => {
                                if let HoldingLocation::Held(juggler, hand) = location {
                                    if juggler != image.event.juggler || hand != image.event.hand {
                                        return Err("error 2 in fixHolds".to_string());
                                    }
                                }
                                holding_location[transition.path - 1] = HoldingLocation::Air;
                            }
                            MhnJmlTransitionType::Holding => {
                                if location != HoldingLocation::Unknown
                                    && location
                                        != HoldingLocation::Held(
                                            image.event.juggler,
                                            image.event.hand,
                                        )
                                {
                                    let path_primary =
                                        primary_path_for_image(&image, transition.path)?;
                                    let primary = self
                                        .events
                                        .get_mut(image.primary_index)
                                        .ok_or_else(|| "error 4 in fixHolds".to_string())?;
                                    let Some(transition_index) =
                                        primary.find_transition_index(path_primary)
                                    else {
                                        return Err("error 3 in fixHolds".to_string());
                                    };
                                    if primary.transitions[transition_index].transition_type
                                        != MhnJmlTransitionType::Holding
                                    {
                                        return Err("error 3 in fixHolds".to_string());
                                    }
                                    primary.transitions.remove(transition_index);
                                    restart = true;
                                    break;
                                }

                                if holds_only[transition.path - 1] {
                                    holding_location[transition.path - 1] = HoldingLocation::Held(
                                        image.event.juggler,
                                        image.event.hand,
                                    );
                                }
                            }
                            MhnJmlTransitionType::Catch
                            | MhnJmlTransitionType::SoftCatch
                            | MhnJmlTransitionType::GrabCatch => unreachable!(),
                        }
                    }
                }

                if restart {
                    break;
                }

                for path in paths_to_hold {
                    let path_primary = primary_path_for_image(&image, path)?;
                    let primary = self
                        .events
                        .get_mut(image.primary_index)
                        .ok_or_else(|| "error 6 in fixHolds".to_string())?;

                    if let Some(existing) = primary.get_path_transition(path_primary) {
                        if existing.transition_type == MhnJmlTransitionType::Holding {
                            continue;
                        }
                        return Err("error 5 in fixHolds".to_string());
                    }

                    primary.transitions.push(MhnJmlTransition {
                        transition_type: MhnJmlTransitionType::Holding,
                        path: path_primary,
                        throw_type: None,
                        throw_mod: None,
                    });
                    restart = true;
                    break;
                }

                if restart {
                    break;
                }
            }

            if restart {
                continue;
            }

            if !finishing {
                let new_holds_only = holding_location
                    .iter()
                    .map(|location| *location == HoldingLocation::Unknown)
                    .collect::<Vec<_>>();
                if new_holds_only.iter().any(|value| *value) {
                    holds_only = new_holds_only;
                    finishing = true;
                    continue;
                }
            }

            return Ok(());
        }

        Err("error 7 in fixHolds".to_string())
    }

    fn neighbor_position_event(
        &self,
        event: &MhnJmlEvent,
        direction: NeighborDirection,
    ) -> Option<EventImage> {
        let period = self.loop_end_time().ok()? - self.loop_start_time();
        let images = self
            .event_images_between(event.t - period, event.t + period)
            .ok()?;
        let mut best: Option<EventImage> = None;

        for candidate in images {
            if self
                .events
                .get(candidate.primary_index)
                .is_some_and(|primary| primary.calcpos)
                || candidate.event.juggler != event.juggler
                || candidate.event.hand != event.hand
                || !candidate.event.t.is_finite()
            {
                continue;
            }

            let in_range = match direction {
                NeighborDirection::Previous => {
                    candidate.event.t < event.t && candidate.event.t > event.t - period - 1e-9
                }
                NeighborDirection::Next => {
                    candidate.event.t > event.t && candidate.event.t < event.t + period + 1e-9
                }
            };
            if !in_range {
                continue;
            }

            let image = EventImage {
                x: candidate.event.x,
                y: candidate.event.y,
                z: candidate.event.z,
                t: candidate.event.t,
            };

            let replace = match (direction, &best) {
                (_, None) => true,
                (NeighborDirection::Previous, Some(best)) => image.t > best.t,
                (NeighborDirection::Next, Some(best)) => image.t < best.t,
            };
            if replace {
                best = Some(image);
            }
        }

        best
    }
}

impl MhnJmlProp {
    pub fn new(prop_type: impl Into<String>, modifier: Option<String>) -> Self {
        Self {
            prop_type: prop_type.into(),
            modifier,
        }
    }

    pub fn is_colorable(&self) -> bool {
        !self.prop_type.eq_ignore_ascii_case("image")
    }

    pub fn image_source(&self) -> Result<Option<String>, String> {
        if !self.prop_type.eq_ignore_ascii_case("image") {
            return Ok(None);
        }
        let parameters = ParameterList::parse(self.modifier.as_deref())?;
        Ok(parameters.get_parameter("image").map(decode_image_source))
    }

    pub fn set_image_source(&mut self, source: &str) -> Result<(), String> {
        if !self.prop_type.eq_ignore_ascii_case("image") {
            return Err("Only image props have an image source".to_string());
        }
        if source.trim().is_empty() {
            return Err("Image source cannot be empty".to_string());
        }
        let mut parameters = ParameterList::parse(self.modifier.as_deref())?;
        parameters.add_parameter("image", encode_image_source(source));
        let modifier = parameters.to_string();
        PropSpec::from_jml(&self.prop_type, Some(&modifier))?;
        self.modifier = Some(modifier);
        Ok(())
    }

    pub fn to_jml(&self) -> String {
        let modifier = self
            .modifier
            .as_ref()
            .map(|modifier| format!(" mod=\"{}\"", escape_xml(modifier)))
            .unwrap_or_default();
        format!(
            "<prop type=\"{}\"{modifier}/>\n",
            escape_xml(&self.prop_type)
        )
    }
}

impl MhnJmlEvent {
    pub fn new(x: f64, y: f64, z: f64, t: f64, juggler: usize, hand: usize) -> Self {
        Self {
            x,
            y,
            z,
            t,
            juggler,
            hand,
            calcpos: false,
            transitions: Vec::new(),
        }
    }

    pub fn with_calcpos(mut self, calcpos: bool) -> Self {
        self.calcpos = calcpos;
        self
    }

    pub fn with_transition(mut self, transition: MhnJmlTransition) -> Self {
        self.transitions.push(transition);
        self
    }

    pub fn get_path_transition(&self, path: usize) -> Option<&MhnJmlTransition> {
        self.transitions
            .iter()
            .find(|transition| transition.path == path)
    }

    pub fn find_transition_index(&self, path: usize) -> Option<usize> {
        self.transitions
            .iter()
            .position(|transition| transition.path == path)
    }

    pub fn to_jml(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "<event x=\"{}\" y=\"{}\" z=\"{}\" t=\"{}\" hand=\"{}:{}\">\n",
            to_string_rounded(self.x, 4),
            to_string_rounded(self.y, 4),
            to_string_rounded(self.z, 4),
            to_string_rounded(self.t, 4),
            self.juggler,
            hand_name(self.hand)
        ));
        for transition in &self.transitions {
            out.push_str(&transition.to_jml());
        }
        out.push_str("</event>\n");
        out
    }
}

impl MhnJmlTransition {
    pub fn to_jml(&self) -> String {
        match self.transition_type {
            MhnJmlTransitionType::Throw => {
                let mut out = format!("<throw path=\"{}\"", self.path);
                if let Some(throw_type) = &self.throw_type {
                    out.push_str(&format!(" type=\"{}\"", escape_xml(throw_type)));
                }
                if let Some(throw_mod) = &self.throw_mod {
                    out.push_str(&format!(" mod=\"{}\"", escape_xml(throw_mod)));
                }
                out.push_str("/>\n");
                out
            }
            MhnJmlTransitionType::Catch => format!("<catch path=\"{}\"/>\n", self.path),
            MhnJmlTransitionType::SoftCatch => {
                format!("<catch path=\"{}\" type=\"soft\"/>\n", self.path)
            }
            MhnJmlTransitionType::GrabCatch => {
                format!("<catch path=\"{}\" type=\"grab\"/>\n", self.path)
            }
            MhnJmlTransitionType::Holding => format!("<holding path=\"{}\"/>\n", self.path),
        }
    }
}

impl MhnJmlSymmetry {
    pub fn to_jml(&self) -> String {
        match self.symmetry_type {
            MhnSymmetryType::Delay => format!(
                "<symmetry type=\"delay\" pperm=\"{}\" delay=\"{}\"/>\n",
                self.path_perm,
                to_string_rounded(self.delay, 4)
            ),
            MhnSymmetryType::Switch => format!(
                "<symmetry type=\"switch\" jperm=\"{}\" pperm=\"{}\"/>\n",
                self.juggler_perm, self.path_perm
            ),
            MhnSymmetryType::SwitchDelay => format!(
                "<symmetry type=\"switchdelay\" jperm=\"{}\" pperm=\"{}\"/>\n",
                self.juggler_perm, self.path_perm
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MhnEventImage {
    pub event: MhnJmlEvent,
    pub primary_index: usize,
    pub path_perm_from_primary: Permutation,
    pub is_primary_image: bool,
}

#[derive(Clone, Debug)]
struct MhnEventImages {
    primary_index: usize,
    primary_event: MhnJmlEvent,
    number_of_jugglers: usize,
    number_of_paths: usize,
    loop_time: f64,
    loop_perm: Permutation,
    primary_juggler: usize,
    primary_hand: usize,
    primary_time: f64,
    number_of_entries: usize,
    entries: Vec<Vec<Vec<Option<Permutation>>>>,
}

#[derive(Clone, Debug)]
struct SymmetryImage {
    symmetry: MhnJmlSymmetry,
    period: usize,
    delta_entries: usize,
    switch_delay: bool,
}

impl MhnEventImages {
    fn new(pattern: &MhnJmlPattern, primary_index: usize) -> Result<Self, String> {
        let primary_event = pattern
            .events
            .get(primary_index)
            .ok_or_else(|| "Missing primary event".to_string())?
            .clone();
        let loop_time = pattern.loop_end_time()? - pattern.loop_start_time();
        let loop_perm = pattern.path_permutation()?.clone();
        let inverse_delay_perm = loop_perm.inverse();
        let mut symmetries = Vec::new();
        let mut number_of_entries = 1usize;

        for symmetry in &pattern.symmetries {
            match symmetry.symmetry_type {
                MhnSymmetryType::Delay => {}
                MhnSymmetryType::Switch => {
                    symmetries.push(SymmetryImage {
                        symmetry: symmetry.clone(),
                        period: symmetry.juggler_perm.order(),
                        delta_entries: 0,
                        switch_delay: false,
                    });
                }
                MhnSymmetryType::SwitchDelay => {
                    let period = symmetry.juggler_perm.order();
                    number_of_entries = lcm(number_of_entries, period);
                    symmetries.push(SymmetryImage {
                        symmetry: symmetry.clone(),
                        period,
                        delta_entries: 0,
                        switch_delay: true,
                    });
                }
            }
        }

        for symmetry in &mut symmetries {
            if symmetry.switch_delay {
                symmetry.delta_entries = number_of_entries / symmetry.period.max(1);
            }
        }

        let mut entries = vec![vec![vec![None; number_of_entries]; 2]; pattern.number_of_jugglers];
        entries[primary_event.juggler - 1][primary_event.hand][0] =
            Some(Permutation::identity(pattern.number_of_paths));

        let mut changed = true;
        while changed {
            changed = false;

            for symmetry in &symmetries {
                for juggler in 0..pattern.number_of_jugglers {
                    for hand in 0..2 {
                        for entry in 0..number_of_entries {
                            let Some(mut perm) = entries[juggler][hand][entry].clone() else {
                                continue;
                            };

                            let mut mapped_juggler =
                                symmetry.symmetry.juggler_perm.map(juggler as i32 + 1);
                            if mapped_juggler == 0 {
                                continue;
                            }

                            let mapped_hand = if mapped_juggler < 0 { 1 - hand } else { hand };
                            if mapped_juggler < 0 {
                                mapped_juggler = -mapped_juggler;
                            }
                            let mapped_juggler = mapped_juggler as usize - 1;
                            perm = perm.composed_with(Some(&symmetry.symmetry.path_perm));

                            let mut mapped_entry = entry + symmetry.delta_entries;
                            if mapped_entry >= number_of_entries {
                                perm = perm.composed_with(Some(&inverse_delay_perm));
                                mapped_entry -= number_of_entries;
                            }

                            match &entries[mapped_juggler][mapped_hand][mapped_entry] {
                                Some(existing) if existing != &perm => {
                                    return Err("Symmetries inconsistent".to_string());
                                }
                                Some(_) => {}
                                None => {
                                    entries[mapped_juggler][mapped_hand][mapped_entry] = Some(perm);
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            primary_index,
            primary_juggler: primary_event.juggler - 1,
            primary_hand: primary_event.hand,
            primary_time: primary_event.t,
            primary_event,
            number_of_jugglers: pattern.number_of_jugglers,
            number_of_paths: pattern.number_of_paths,
            loop_time,
            loop_perm,
            number_of_entries,
            entries,
        })
    }

    fn event_image_at(
        &self,
        juggler: usize,
        hand: usize,
        entry: usize,
        loop_index: isize,
    ) -> MhnEventImage {
        let is_primary_image = entry == 0
            && loop_index == 0
            && hand == self.primary_hand
            && juggler == self.primary_juggler;

        if is_primary_image {
            return MhnEventImage {
                event: self.primary_event.clone(),
                primary_index: self.primary_index,
                path_perm_from_primary: Permutation::identity(self.number_of_paths),
                is_primary_image: true,
            };
        }

        let mut path_perm = self.entries[juggler][hand][entry]
            .clone()
            .expect("event_image_at called only for populated entries");
        let loop_perm = self.loop_perm.powered(loop_index);
        path_perm = path_perm.composed_with(Some(&loop_perm));

        let mut event = self.primary_event.clone();
        if hand != self.primary_hand {
            event.x = -event.x;
        }
        event.t = self.primary_time
            + loop_index as f64 * self.loop_time
            + entry as f64 * (self.loop_time / self.number_of_entries as f64);
        event.juggler = juggler + 1;
        event.hand = hand;
        event.transitions = self
            .primary_event
            .transitions
            .iter()
            .map(|transition| MhnJmlTransition {
                path: path_perm.map(transition.path as i32) as usize,
                ..transition.clone()
            })
            .collect();

        MhnEventImage {
            event,
            primary_index: self.primary_index,
            path_perm_from_primary: path_perm,
            is_primary_image: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct EventImage {
    x: f64,
    y: f64,
    z: f64,
    t: f64,
}

#[derive(Clone, Copy, Debug)]
enum NeighborDirection {
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoldingLocation {
    Unknown,
    Air,
    Held(usize, usize),
}

const PROP_COLOR_MIXED: [&str; 10] = [
    "red", "green", "blue", "yellow", "cyan", "magenta", "orange", "pink", "gray", "black",
];

const PROP_COLOR_NAMES: [&str; 12] = [
    "transparent",
    "black",
    "blue",
    "cyan",
    "gray",
    "green",
    "magenta",
    "orange",
    "pink",
    "red",
    "white",
    "yellow",
];

fn validate_prop_color_name(color: &str) -> Result<(), String> {
    if PROP_COLOR_NAMES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(color.trim()))
    {
        Ok(())
    } else {
        Err(format!("Invalid prop color: {color}"))
    }
}

fn validate_prop_rgb(parts: &[&str], color: &str) -> Result<(), String> {
    for part in parts {
        part.trim()
            .parse::<u8>()
            .map_err(|_| format!("Invalid prop color: {color}"))?;
    }
    Ok(())
}

fn primary_path_for_image(image: &MhnEventImage, path: usize) -> Result<usize, String> {
    if image.is_primary_image {
        return Ok(path);
    }

    let mapped = image.path_perm_from_primary.map_inverse(path as i32);
    if mapped <= 0 {
        return Err("error in fixHolds: path permutation inverse".to_string());
    }
    Ok(mapped as usize)
}

fn strip_doctype(xml: &str) -> String {
    xml.lines()
        .filter(|line| !line.trim_start().starts_with("<!DOCTYPE"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_f64_attribute(
    node: Node<'_, '_>,
    name: &str,
    default: Option<f64>,
    element: &str,
) -> Result<f64, String> {
    let Some(source) = node.attribute(name) else {
        return default
            .ok_or_else(|| format!("Invalid JML: <{element}> attribute '{name}' is required"));
    };
    source
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("Invalid JML: invalid {element} attribute '{name}'"))
}

fn parse_usize_attribute(
    node: Node<'_, '_>,
    name: &str,
    default: Option<usize>,
    element: &str,
) -> Result<usize, String> {
    let Some(source) = node.attribute(name) else {
        return default
            .ok_or_else(|| format!("Invalid JML: <{element}> attribute '{name}' is required"));
    };
    source
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("Invalid JML: invalid {element} attribute '{name}'"))
}

fn parse_prop_assignment(
    source: Option<&str>,
    number_of_paths: usize,
    number_of_props: usize,
) -> Result<Vec<usize>, String> {
    let Some(source) = source.filter(|source| !source.trim().is_empty()) else {
        if number_of_paths > 0 && number_of_props == 0 {
            return Err("Invalid JML: setup assigns paths without defining a prop".to_string());
        }
        return Ok(vec![1; number_of_paths]);
    };
    let tokens = source.split(',').collect::<Vec<_>>();
    if tokens.len() != number_of_paths {
        return Err("Invalid JML: wrong number of prop assignments".to_string());
    }
    tokens
        .into_iter()
        .map(|token| {
            let prop = token
                .trim()
                .parse::<usize>()
                .map_err(|_| "Invalid JML: invalid prop assignment format".to_string())?;
            if prop == 0 || prop > number_of_props {
                return Err(format!(
                    "Invalid JML: prop assignment {prop} is out of range"
                ));
            }
            Ok(prop)
        })
        .collect()
}

fn parse_jml_symmetry(
    node: Node<'_, '_>,
    number_of_jugglers: usize,
    number_of_paths: usize,
) -> Result<MhnJmlSymmetry, String> {
    let symmetry_type_source = node
        .attribute("type")
        .ok_or_else(|| "Invalid JML: symmetry type is required".to_string())?;
    let symmetry_type = match symmetry_type_source.to_lowercase().as_str() {
        "delay" => MhnSymmetryType::Delay,
        "switch" => MhnSymmetryType::Switch,
        "switchdelay" => MhnSymmetryType::SwitchDelay,
        other => return Err(format!("Unrecognized symmetry type: {other}")),
    };
    let jperm = node
        .attribute("jperm")
        .map(|value| Permutation::parse(number_of_jugglers, value, true))
        .transpose()?
        .unwrap_or_else(|| Permutation::new(number_of_jugglers, true));
    let pperm = node
        .attribute("pperm")
        .map(|value| Permutation::parse(number_of_paths, value, false))
        .transpose()?
        .unwrap_or_else(|| Permutation::identity(number_of_paths));
    Ok(MhnJmlSymmetry {
        symmetry_type,
        number_of_jugglers,
        number_of_paths,
        juggler_perm: jperm,
        path_perm: pperm,
        delay: parse_f64_attribute(node, "delay", Some(-1.0), "symmetry")?,
    })
}

fn parse_hand(source: &str) -> Result<(usize, usize), String> {
    let (juggler, hand_name) = if let Some((juggler, hand)) = source.split_once(':') {
        let juggler = juggler
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("Invalid JML hand '{source}'"))?;
        (juggler, hand.trim())
    } else {
        (1, source.trim())
    };
    let hand = if hand_name.eq_ignore_ascii_case("left") {
        1
    } else if hand_name.eq_ignore_ascii_case("right") {
        0
    } else {
        return Err(format!("Invalid JML hand '{source}'"));
    };
    Ok((juggler, hand))
}

fn parse_jml_transition(
    node: Node<'_, '_>,
    number_of_paths: usize,
) -> Result<MhnJmlTransition, String> {
    if node.children().any(|child| child.is_element()) {
        return Err("Invalid JML: event transitions cannot contain subtags".to_string());
    }
    let path = parse_usize_attribute(node, "path", None, "event transition")?;
    if path == 0 || path > number_of_paths {
        return Err(format!(
            "Invalid JML: transition path {path} is out of range"
        ));
    }
    let transition_type = if node.has_tag_name("throw") {
        MhnJmlTransitionType::Throw
    } else if node.has_tag_name("catch") {
        match node.attribute("type") {
            Some(value) if value.eq_ignore_ascii_case("soft") => MhnJmlTransitionType::SoftCatch,
            Some(value) if value.eq_ignore_ascii_case("grab") => MhnJmlTransitionType::GrabCatch,
            _ => MhnJmlTransitionType::Catch,
        }
    } else if node.has_tag_name("softcatch") {
        MhnJmlTransitionType::SoftCatch
    } else if node.has_tag_name("holding") {
        MhnJmlTransitionType::Holding
    } else {
        return Err(format!(
            "Invalid JML: '{}' is not an event transition",
            node.tag_name().name()
        ));
    };
    let throw_type = if transition_type == MhnJmlTransitionType::Throw {
        node.attribute("type").map(str::to_string)
    } else {
        None
    };
    let throw_mod = if transition_type == MhnJmlTransitionType::Throw {
        node.attribute("mod").map(str::to_string)
    } else {
        None
    };
    Ok(MhnJmlTransition {
        transition_type,
        path,
        throw_type,
        throw_mod,
    })
}

fn truncated_time(value: f64) -> f64 {
    format!("{value:.4}").parse::<f64>().unwrap_or(value)
}

fn events_are_coincident(left: &MhnJmlEvent, right: &MhnJmlEvent) -> bool {
    left.juggler == right.juggler
        && left.hand == right.hand
        && (truncated_time(left.t) - truncated_time(right.t)).abs() < 1e-9
}

fn compare_events(left: &MhnJmlEvent, right: &MhnJmlEvent) -> std::cmp::Ordering {
    truncated_time(left.t)
        .total_cmp(&truncated_time(right.t))
        .then(left.juggler.cmp(&right.juggler))
        .then(left.hand.cmp(&right.hand))
        .then(left.x.total_cmp(&right.x))
}

fn compare_positions(left: &BodyPosition, right: &BodyPosition) -> std::cmp::Ordering {
    truncated_time(left.t)
        .total_cmp(&truncated_time(right.t))
        .then(left.juggler.cmp(&right.juggler))
        .then(left.x.total_cmp(&right.x))
}

fn position_to_jml(position: &BodyPosition) -> String {
    format!(
        "<position x=\"{}\" y=\"{}\" z=\"{}\" t=\"{}\" angle=\"{}\" juggler=\"{}\"/>\n",
        to_string_rounded(position.x, 4),
        to_string_rounded(position.y, 4),
        to_string_rounded(position.z, 4),
        to_string_rounded(position.t, 4),
        to_string_rounded(position.angle, 4),
        position.juggler
    )
}

fn format_base_pattern_config(config: &str) -> String {
    let mut output = String::new();
    let mut chars = config.chars().peekable();
    while let Some(ch) = chars.next() {
        output.push(ch);
        if ch == ';' {
            while chars.peek().is_some_and(|next| next.is_whitespace()) {
                chars.next();
            }
            output.push('\n');
        }
    }
    output
}

fn hand_name(hand: usize) -> &'static str {
    if hand == 1 { "left" } else { "right" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delay_symmetry(paths: usize, delay: f64) -> MhnJmlSymmetry {
        MhnJmlSymmetry {
            symmetry_type: MhnSymmetryType::Delay,
            number_of_jugglers: 1,
            number_of_paths: paths,
            juggler_perm: Permutation::identity(1),
            path_perm: Permutation::identity(paths),
            delay,
        }
    }

    fn delay_symmetry_with_path_perm(paths: usize, delay: f64, path_perm: &str) -> MhnJmlSymmetry {
        MhnJmlSymmetry {
            symmetry_type: MhnSymmetryType::Delay,
            number_of_jugglers: 1,
            number_of_paths: paths,
            juggler_perm: Permutation::identity(1),
            path_perm: Permutation::parse(paths, path_perm, false).unwrap(),
            delay,
        }
    }

    fn transition(transition_type: MhnJmlTransitionType, path: usize) -> MhnJmlTransition {
        MhnJmlTransition {
            transition_type,
            path,
            throw_type: None,
            throw_mod: None,
        }
    }

    #[test]
    fn scaled_time_updates_every_timed_structure_and_path_cache() {
        let mut pattern = MhnJmlPattern::new(1, 1, 2.0);
        pattern.symmetries.push(delay_symmetry(1, 2.0));
        pattern.positions.push(BodyPosition {
            x: 0.0,
            y: 0.0,
            z: 100.0,
            t: 0.5,
            angle: 0.0,
            juggler: 1,
        });
        pattern.events.push(
            MhnJmlEvent::new(20.0, 0.0, 0.0, 0.75, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Throw, 1)),
        );
        pattern.rebuild_path_events();

        let scaled = pattern.with_scaled_time(1.5);

        assert!((scaled.period_secs - 3.0).abs() < 1e-9);
        assert!((scaled.symmetries[0].delay - 3.0).abs() < 1e-9);
        assert!((scaled.positions[0].t - 0.75).abs() < 1e-9);
        assert!((scaled.events[0].t - 1.125).abs() < 1e-9);
        assert!((scaled.path_events[0][0].t - 1.125).abs() < 1e-9);
    }

    #[test]
    fn inverted_x_axis_matches_swap_hands_and_flip_x_commands() {
        let mut pattern = MhnJmlPattern::new(1, 1, 2.0);
        pattern.symmetries.push(delay_symmetry(1, 2.0));
        pattern
            .events
            .push(MhnJmlEvent::new(18.0, 4.0, 7.0, 0.25, 1, 0));

        let swapped = pattern.with_inverted_x_axis(false);
        assert_eq!(swapped.events[0].hand, 1);
        assert_eq!(swapped.events[0].x, 18.0);
        assert_eq!(swapped.events[0].y, 4.0);
        assert_eq!(swapped.events[0].z, 7.0);

        let flipped = pattern.with_inverted_x_axis(true);
        assert_eq!(flipped.events[0].hand, 1);
        assert_eq!(flipped.events[0].x, -18.0);
    }

    #[test]
    fn inverted_time_reverses_events_positions_and_throw_details() {
        let mut pattern = MhnJmlPattern::new(1, 1, 2.0);
        pattern.symmetries.push(delay_symmetry(1, 2.0));
        pattern.props.push(MhnJmlProp::new("ball", None));
        pattern.prop_assignment = vec![1];
        pattern.positions.push(BodyPosition {
            x: 0.0,
            y: 0.0,
            z: 100.0,
            t: 0.5,
            angle: 0.0,
            juggler: 1,
        });
        pattern.events.push(
            MhnJmlEvent::new(20.0, 0.0, 0.0, 0.25, 1, 0).with_transition(MhnJmlTransition {
                transition_type: MhnJmlTransitionType::Throw,
                path: 1,
                throw_type: Some("bounce".to_string()),
                throw_mod: Some("bounces=2".to_string()),
            }),
        );
        pattern.events.push(
            MhnJmlEvent::new(-20.0, 0.0, 0.0, 1.25, 1, 1)
                .with_transition(transition(MhnJmlTransitionType::SoftCatch, 1)),
        );

        let inverted = pattern.with_inverted_time().unwrap();

        assert!((inverted.positions[0].t - 1.5).abs() < 1e-9);
        let throw_event = inverted
            .events
            .iter()
            .find(|event| (event.t - 0.75).abs() < 1e-9)
            .unwrap();
        let restored_throw = &throw_event.transitions[0];
        assert_eq!(restored_throw.transition_type, MhnJmlTransitionType::Throw);
        assert_eq!(restored_throw.throw_type.as_deref(), Some("bounce"));
        assert_eq!(restored_throw.throw_mod.as_deref(), Some("bounces=2"));

        let catch_event = inverted
            .events
            .iter()
            .find(|event| (event.t - 1.75).abs() < 1e-9)
            .unwrap();
        assert_eq!(
            catch_event.transitions[0].transition_type,
            MhnJmlTransitionType::Catch
        );
        assert!(inverted.assert_valid().is_ok());
    }

    #[test]
    fn inverted_time_inverts_non_switch_path_permutations() {
        let mut pattern = MhnJmlPattern::new(1, 3, 2.0);
        pattern
            .symmetries
            .push(delay_symmetry_with_path_perm(3, 2.0, "2,3,1"));

        let inverted = pattern.with_inverted_time().unwrap();

        assert_eq!(inverted.symmetries[0].path_perm.map(1), 3);
        assert_eq!(inverted.symmetries[0].path_perm.map(2), 1);
        assert_eq!(inverted.symmetries[0].path_perm.map(3), 2);
    }

    #[test]
    fn inserts_calcpos_events_for_long_hand_gaps() {
        let mut pattern = MhnJmlPattern::new(1, 0, 2.0);
        pattern
            .symmetries
            .push(delay_symmetry_with_path_perm(0, 2.0, ""));
        pattern
            .events
            .push(MhnJmlEvent::new(0.0, 0.0, 0.0, 0.0, 1, 0));
        pattern
            .events
            .push(MhnJmlEvent::new(12.0, 0.0, 0.0, 1.2, 1, 0));

        pattern.add_events_for_gaps(0.5);

        assert!(pattern.events.iter().any(|event| event.calcpos));
        let images = pattern.event_images_between(-2.0, 4.0).unwrap();
        for pair in images
            .iter()
            .filter(|image| image.event.juggler == 1 && image.event.hand == 0)
            .collect::<Vec<_>>()
            .windows(2)
        {
            assert!(pair[1].event.t - pair[0].event.t <= 0.5 + 1e-9);
        }
    }

    #[test]
    fn interpolates_incomplete_event_locations() {
        let mut pattern = MhnJmlPattern::new(1, 0, 2.0);
        pattern
            .symmetries
            .push(delay_symmetry_with_path_perm(0, 2.0, ""));
        pattern
            .events
            .push(MhnJmlEvent::new(0.0, 0.0, 0.0, 0.0, 1, 0));
        pattern
            .events
            .push(MhnJmlEvent::new(10.0, 20.0, 30.0, 1.0, 1, 0));
        pattern
            .events
            .push(MhnJmlEvent::new(0.0, 0.0, 0.0, 0.5, 1, 0).with_calcpos(true));

        pattern.add_locations_for_incomplete_events(25.0).unwrap();

        let event = pattern
            .events
            .iter()
            .find(|event| (event.t - 0.5).abs() < 1e-9)
            .unwrap();
        assert!(!event.calcpos);
        assert!((event.x - 5.0).abs() < 1e-9);
        assert!((event.y - 10.0).abs() < 1e-9);
        assert!((event.z - 15.0).abs() < 1e-9);
    }

    #[test]
    fn incomplete_events_without_anchor_use_resting_position() {
        let mut pattern = MhnJmlPattern::new(1, 0, 2.0);
        pattern
            .events
            .push(MhnJmlEvent::new(0.0, 0.0, 0.0, 0.5, 1, 1).with_calcpos(true));

        pattern.add_locations_for_incomplete_events(25.0).unwrap();

        assert_eq!(pattern.events[0].x, -25.0);
        assert!(!pattern.events[0].calcpos);
    }

    #[test]
    fn event_images_apply_delay_symmetry() {
        let mut pattern = MhnJmlPattern::new(1, 1, 1.0);
        pattern.symmetries.push(delay_symmetry(1, 1.0));
        pattern.events.push(
            MhnJmlEvent::new(10.0, 0.0, 0.0, 0.2, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Throw, 1)),
        );

        let images = pattern.event_images_between(-1.0, 1.5).unwrap();

        assert!(
            images
                .iter()
                .any(|image| (image.event.t + 0.8).abs() < 1e-9)
        );
        assert!(
            images
                .iter()
                .any(|image| (image.event.t - 0.2).abs() < 1e-9)
        );
        assert!(
            images
                .iter()
                .any(|image| (image.event.t - 1.2).abs() < 1e-9)
        );
    }

    #[test]
    fn select_primary_events_promotes_loop_image() {
        let mut pattern = MhnJmlPattern::new(1, 1, 1.0);
        pattern.symmetries.push(delay_symmetry(1, 1.0));
        pattern.events.push(
            MhnJmlEvent::new(10.0, 0.0, 0.0, 1.2, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Throw, 1)),
        );

        pattern.select_primary_events().unwrap();

        assert!((pattern.events[0].t - 0.2).abs() < 1e-9);
    }

    #[test]
    fn fix_holds_adds_missing_holding_transition() {
        let mut pattern = MhnJmlPattern::new(1, 1, 2.0);
        pattern.symmetries.push(delay_symmetry(1, 2.0));
        pattern.events.push(
            MhnJmlEvent::new(15.0, 0.0, 0.0, 0.0, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Catch, 1)),
        );
        pattern
            .events
            .push(MhnJmlEvent::new(20.0, 0.0, 0.0, 0.5, 1, 0));
        pattern.events.push(
            MhnJmlEvent::new(25.0, 0.0, 0.0, 1.0, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Throw, 1)),
        );

        pattern.fix_holds().unwrap();

        let holding_event = pattern
            .events
            .iter()
            .find(|event| (event.t - 0.5).abs() < 1e-9)
            .unwrap();
        assert!(holding_event.transitions.iter().any(|transition| {
            transition.transition_type == MhnJmlTransitionType::Holding && transition.path == 1
        }));
    }

    #[test]
    fn fix_holds_removes_wrong_holding_transition() {
        let mut pattern = MhnJmlPattern::new(1, 1, 2.0);
        pattern.symmetries.push(delay_symmetry(1, 2.0));
        pattern.events.push(
            MhnJmlEvent::new(15.0, 0.0, 0.0, 0.0, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Catch, 1)),
        );
        pattern.events.push(
            MhnJmlEvent::new(-15.0, 0.0, 0.0, 0.5, 1, 1)
                .with_transition(transition(MhnJmlTransitionType::Holding, 1)),
        );
        pattern.events.push(
            MhnJmlEvent::new(25.0, 0.0, 0.0, 1.0, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Throw, 1)),
        );

        pattern.fix_holds().unwrap();

        let left_event = pattern
            .events
            .iter()
            .find(|event| (event.t - 0.5).abs() < 1e-9)
            .unwrap();
        assert!(left_event.transitions.is_empty());
    }

    #[test]
    fn applies_mixed_prop_colors() {
        let mut pattern = MhnJmlPattern::new(1, 3, 1.0);
        pattern.symmetries.push(delay_symmetry(3, 1.0));
        pattern
            .props
            .push(MhnJmlProp::new("ball", Some("diam=12.0".to_string())));
        pattern.prop_assignment = vec![1, 1, 1];

        pattern.apply_prop_colors("mixed").unwrap();

        assert_eq!(pattern.props.len(), 3);
        assert_eq!(pattern.prop_assignment, vec![1, 2, 3]);
        assert_eq!(
            pattern.props[0].modifier.as_deref(),
            Some("diam=12.0;color=red")
        );
        assert_eq!(
            pattern.props[1].modifier.as_deref(),
            Some("diam=12.0;color=green")
        );
        assert_eq!(
            pattern.props[2].modifier.as_deref(),
            Some("diam=12.0;color=blue")
        );
    }

    #[test]
    fn applies_orbit_prop_colors_from_delay_permutation() {
        let mut pattern = MhnJmlPattern::new(1, 3, 1.0);
        pattern
            .symmetries
            .push(delay_symmetry_with_path_perm(3, 1.0, "(1,2)"));
        pattern.props.push(MhnJmlProp::new("ball", None));
        pattern.prop_assignment = vec![1, 1, 1];

        pattern.apply_prop_colors("orbits").unwrap();

        assert_eq!(pattern.props.len(), 2);
        assert_eq!(pattern.prop_assignment, vec![1, 1, 2]);
        assert_eq!(pattern.props[0].modifier.as_deref(), Some("color=red"));
        assert_eq!(pattern.props[1].modifier.as_deref(), Some("color=green"));
    }

    #[test]
    fn applies_repeated_named_and_rgb_prop_colors() {
        let mut pattern = MhnJmlPattern::new(1, 4, 1.0);
        pattern.symmetries.push(delay_symmetry(4, 1.0));
        pattern.props.push(MhnJmlProp::new("square", None));
        pattern.prop_assignment = vec![1; 4];

        pattern.apply_prop_colors("({red}{10,20,30})^2").unwrap();

        assert_eq!(pattern.prop_assignment, vec![1, 2, 1, 2]);
        assert_eq!(pattern.props[0].modifier.as_deref(), Some("color=red"));
        assert_eq!(
            pattern.props[1].modifier.as_deref(),
            Some("color={10,20,30}")
        );
    }

    #[test]
    fn rejects_invalid_prop_color_values() {
        let mut pattern = MhnJmlPattern::new(1, 1, 1.0);
        pattern.symmetries.push(delay_symmetry(1, 1.0));
        pattern.props.push(MhnJmlProp::new("ball", None));
        pattern.prop_assignment = vec![1];

        assert_eq!(
            pattern.apply_prop_colors("chartreuse").unwrap_err(),
            "Invalid prop color: chartreuse"
        );
        assert_eq!(
            pattern.apply_prop_colors("{256,0,0}").unwrap_err(),
            "Invalid prop color: 256,0,0"
        );
    }

    #[test]
    fn writes_jml_pattern_xml() {
        let mut pattern = MhnJmlPattern::new(1, 1, 1.0);
        pattern.title = Some("Cascade".to_string());
        pattern.base_pattern_notation = Some("siteswap".to_string());
        pattern.base_pattern_config = Some("pattern=3;bps=3".to_string());
        pattern.symmetries.push(delay_symmetry(1, 1.0));
        pattern.props.push(MhnJmlProp::new("ball", None));
        pattern.prop_assignment = vec![1];
        pattern.events.push(
            MhnJmlEvent::new(20.0, 0.0, 0.0, 0.0, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Throw, 1)),
        );
        pattern.events.push(
            MhnJmlEvent::new(30.0, 0.0, 0.0, 0.5, 1, 1)
                .with_transition(transition(MhnJmlTransitionType::Catch, 1)),
        );

        let xml = pattern.write_jml(true, true);

        assert!(xml.contains("<jml version=\"3\">"));
        assert!(xml.contains("<title>Cascade</title>"));
        assert!(xml.contains("<basepattern notation=\"siteswap\">"));
        assert!(xml.contains("pattern=3;\nbps=3"));
        assert!(xml.contains("<setup jugglers=\"1\" paths=\"1\" props=\"1\"/>"));
        assert!(xml.contains("hand=\"1:right\""));
        assert!(xml.contains("hand=\"1:left\""));
    }

    #[test]
    fn image_prop_source_replacement_preserves_other_parameters() {
        let mut prop = MhnJmlProp::new(
            "image",
            Some("image=https://example.com/original.png;width=17".to_string()),
        );
        assert_eq!(
            prop.image_source().unwrap().as_deref(),
            Some("https://example.com/original.png")
        );

        prop.set_image_source("data:image/png;base64,AAAA").unwrap();
        assert_eq!(
            prop.modifier.as_deref(),
            Some("image=data:image/png%3Bbase64,AAAA;width=17")
        );
        assert_eq!(
            prop.image_source().unwrap().as_deref(),
            Some("data:image/png;base64,AAAA")
        );
    }

    #[test]
    fn writes_soft_and_grab_catches() {
        assert_eq!(
            transition(MhnJmlTransitionType::SoftCatch, 2).to_jml(),
            "<catch path=\"2\" type=\"soft\"/>\n"
        );
        assert_eq!(
            transition(MhnJmlTransitionType::GrabCatch, 3).to_jml(),
            "<catch path=\"3\" type=\"grab\"/>\n"
        );
    }

    #[test]
    fn parses_jml_xml_into_mhn_model() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <title>Soft catch test</title>
        <prop type="ball" mod="color=red"/>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <event x="-20" y="0" z="0" t="0" hand="1:left">
          <throw path="1" type="toss"/>
        </event>
        <event x="-20" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1" type="soft"/>
        </event>
        </pattern>
        </jml>
        "#;

        let model = MhnJmlPattern::from_jml_xml(xml).unwrap();

        assert_eq!(model.title.as_deref(), Some("Soft catch test"));
        assert_eq!(model.number_of_paths, 1);
        assert_eq!(model.props.len(), 1);
        assert!(model.assert_valid().is_ok());
        assert!(model.events.iter().any(|event| {
            event
                .transitions
                .iter()
                .any(|transition| transition.transition_type == MhnJmlTransitionType::SoftCatch)
        }));
    }

    #[test]
    fn jml_metadata_round_trips_through_the_structured_model() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <title>Metadata pattern</title>
        <info tags="passing,technical">A &amp; B</info>
        <prop type="ball"/>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <event t="0" hand="right"><throw path="1"/></event>
        <event t="0.5" hand="left"><catch path="1"/></event>
        </pattern>
        </jml>
        "#;

        let model = MhnJmlPattern::from_jml_xml(xml).unwrap();
        assert_eq!(model.info.as_deref(), Some("A & B"));
        assert_eq!(model.tags, vec!["passing", "technical"]);

        let written = model.write_jml(true, true);
        assert!(written.contains("<info tags=\"passing,technical\">A &amp; B</info>"));
        let reparsed = MhnJmlPattern::from_jml_xml(&written).unwrap();
        assert_eq!(reparsed.info, model.info);
        assert_eq!(reparsed.tags, model.tags);
        assert!(!model.write_jml(true, false).contains("<info"));
    }

    #[test]
    fn jml_version_one_swaps_event_y_and_z_coordinates() {
        let xml = r#"
        <jml version="1.0">
        <pattern>
        <prop type="ball"/>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <event x="1" y="2" z="3" t="0" hand="right"><throw path="1"/></event>
        <event t="0.5" hand="left"><catch path="1"/></event>
        </pattern>
        </jml>
        "#;

        let model = MhnJmlPattern::from_jml_xml(xml).unwrap();
        assert_eq!(model.events[0].y, 3.0);
        assert_eq!(model.events[0].z, 2.0);
    }

    #[test]
    fn strict_jml_parser_rejects_invalid_attributes_and_assignments() {
        let valid = r#"
        <jml version="3"><pattern>
        <prop type="ball"/>
        <setup jugglers="1" paths="1" props="1"/>
        <symmetry type="delay" pperm="1" delay="1"/>
        <event x="20" t="0" hand="right"><throw path="1"/></event>
        <event x="-20" t="0.5" hand="left"><catch path="1"/></event>
        </pattern></jml>
        "#;
        assert!(MhnJmlPattern::from_jml_xml(valid).is_ok());

        assert!(
            MhnJmlPattern::from_jml_xml(&valid.replace("x=\"20\"", "x=\"nope\""))
                .unwrap_err()
                .contains("invalid event attribute 'x'")
        );
        assert!(
            MhnJmlPattern::from_jml_xml(&valid.replace(" hand=\"right\"", ""))
                .unwrap_err()
                .contains("event hand is required")
        );
        assert!(
            MhnJmlPattern::from_jml_xml(&valid.replace("props=\"1\"", "props=\"1,1\""))
                .unwrap_err()
                .contains("wrong number of prop assignments")
        );
        assert!(
            MhnJmlPattern::from_jml_xml(&valid.replace("path=\"1\"", "path=\"2\""))
                .unwrap_err()
                .contains("transition path 2 is out of range")
        );
    }

    #[test]
    fn validates_basic_jml_pattern() {
        let mut pattern = MhnJmlPattern::new(1, 1, 1.0);
        pattern.symmetries.push(delay_symmetry(1, 1.0));
        pattern.props.push(MhnJmlProp::new("ball", None));
        pattern.prop_assignment = vec![1];
        pattern.events.push(
            MhnJmlEvent::new(20.0, 0.0, 0.0, 0.0, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Throw, 1)),
        );
        pattern.events.push(
            MhnJmlEvent::new(30.0, 0.0, 0.0, 0.5, 1, 1)
                .with_transition(transition(MhnJmlTransitionType::Catch, 1)),
        );

        assert!(pattern.assert_valid().is_ok());
    }

    #[test]
    fn validation_rejects_path_without_events() {
        let mut pattern = MhnJmlPattern::new(1, 2, 1.0);
        pattern.symmetries.push(delay_symmetry(2, 1.0));
        pattern.props.push(MhnJmlProp::new("ball", None));
        pattern.prop_assignment = vec![1, 1];
        pattern.events.push(
            MhnJmlEvent::new(20.0, 0.0, 0.0, 0.0, 1, 0)
                .with_transition(transition(MhnJmlTransitionType::Throw, 1)),
        );
        pattern.events.push(
            MhnJmlEvent::new(30.0, 0.0, 0.0, 0.5, 1, 1)
                .with_transition(transition(MhnJmlTransitionType::Catch, 1)),
        );

        assert!(pattern.assert_valid().unwrap_err().contains("path 2"));
    }
}
