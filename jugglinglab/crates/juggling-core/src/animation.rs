use crate::jml::PatternRecord;
use crate::layout::LaidoutPattern;
use crate::mhn_body::BodyPosition;
use crate::mhn_jml::MhnJmlPattern;
use crate::mhn_matrix::MhnMatrix;
use crate::mhn_symmetry::MhnSymmetryType;
use crate::permutation::Permutation;
use crate::prop::PropSpec;
use crate::siteswap::{self, SiteswapSpec};
use roxmltree::{Document, Node};

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationSpec {
    pub title: String,
    pub source_label: String,
    pub ball_count: usize,
    pub period_secs: f64,
    pub kind: AnimationKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnimationKind {
    Jml(JmlAnimation),
    Unavailable(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct JmlAnimation {
    pub title: String,
    pub base_config: Option<String>,
    pub paths: usize,
    pub jugglers: usize,
    pub period_secs: f64,
    pub props: Vec<PropSpec>,
    pub prop_assignment: Vec<usize>,
    pub path_permutation: Permutation,
    pub symmetries: Vec<MhnSymmetryType>,
    pub positions: Vec<BodyPosition>,
    pub events: Vec<JmlEvent>,
    pub loop_event_images: Vec<JmlEventImage>,
    pub all_event_images: Vec<JmlEventImage>,
    pub path_events: Vec<Vec<PathEvent>>,
    pub layout: Option<LaidoutPattern>,
}

impl JmlAnimation {
    pub fn prop_for_path(&self, path: usize) -> Option<&PropSpec> {
        let prop_index = self
            .prop_assignment
            .get(path.checked_sub(1)?)?
            .checked_sub(1)?;
        self.props.get(prop_index)
    }

    pub fn prop_assignment_at_time(&self, time: f64) -> Vec<usize> {
        let period = self.period_secs.max(0.1);
        let cycles = (time / period).floor() as i32;
        (1..=self.paths)
            .map(|path| {
                let source_path = self.path_permutation.map_power(path as i32, cycles);
                self.prop_assignment
                    .get(source_path.max(1) as usize - 1)
                    .copied()
                    .unwrap_or(1)
            })
            .collect()
    }

    pub fn prop_for_path_at_time(&self, path: usize, time: f64) -> Option<&PropSpec> {
        let prop_index = self
            .prop_assignment_at_time(time)
            .get(path.checked_sub(1)?)?
            .checked_sub(1)?;
        self.props.get(prop_index)
    }

    pub fn period_with_props(&self) -> usize {
        let size = self.path_permutation.size();
        let mut period = 1usize;
        let mut done = vec![false; size];

        for index in 0..size {
            if done[index] {
                continue;
            }
            let mut cycle = self.path_permutation.cycle_of(index as i32 + 1);
            for path in &mut cycle {
                let path_index = (*path).max(1) as usize - 1;
                done[path_index] = true;
                *path = self.prop_assignment.get(path_index).copied().unwrap_or(1) as i32;
            }
            for cycle_period in 1..=cycle.len() {
                if cycle.len() % cycle_period != 0 {
                    continue;
                }
                if cycle
                    .iter()
                    .enumerate()
                    .all(|(offset, prop)| *prop == cycle[(offset + cycle_period) % cycle.len()])
                {
                    period = crate::permutation::lcm(period, cycle_period);
                    break;
                }
            }
        }
        period
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JmlEvent {
    pub t: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub hand: String,
    pub transitions: Vec<JmlTransition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JmlEventImage {
    pub event: JmlEvent,
    pub primary_index: usize,
    pub primary_time: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JmlTransition {
    pub path: usize,
    pub kind: TransitionKind,
    pub throw_type: Option<String>,
    pub throw_mod: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathEvent {
    pub t: f64,
    pub point: Point3,
    pub kind: TransitionKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionKind {
    Throw,
    Catch,
    SoftCatch,
    GrabCatch,
    Holding,
}

impl AnimationSpec {
    pub fn from_record(record: &PatternRecord) -> Result<Self, String> {
        let notation = record.notation.as_deref().unwrap_or("siteswap");

        if notation.eq_ignore_ascii_case("jml") {
            if let Some(raw) = &record.raw_pattern {
                let jml = parse_jml_animation(raw)?;
                let title = if record.display.trim().is_empty() {
                    jml.title.clone()
                } else {
                    record.display.clone()
                };
                return Ok(Self {
                    title,
                    source_label: "JML".to_string(),
                    ball_count: jml.paths.max(1),
                    period_secs: jml.period_secs.max(0.1),
                    kind: if jml.layout.is_some() {
                        AnimationKind::Jml(jml)
                    } else {
                        AnimationKind::Unavailable(
                            "This JML pattern did not produce a physical layout.".to_string(),
                        )
                    },
                });
            }
        }

        let config = record.config.as_deref().ok_or_else(|| {
            "The selected row does not contain a playable configuration".to_string()
        })?;
        let spec = siteswap::parse_config(config)?;
        let title = if record.display.trim().is_empty() {
            siteswap::display_title(&spec)
        } else {
            record.display.clone()
        };
        match siteswap_to_jml_animation(&spec) {
            Ok(mut jml) => {
                jml.title = title.clone();
                Ok(Self {
                    title,
                    source_label: "siteswap/JML".to_string(),
                    ball_count: jml.paths.max(1),
                    period_secs: jml.period_secs.max(0.1),
                    kind: AnimationKind::Jml(jml),
                })
            }
            Err(err) => {
                let period_secs = (spec.beats.len() as f64 / spec.bps).max(0.1);
                Ok(Self {
                    title,
                    source_label: "Not rendered".to_string(),
                    ball_count: spec.balls.max(1),
                    period_secs,
                    kind: AnimationKind::Unavailable(format!(
                        "This pattern is not available in the physical JML renderer: {err}"
                    )),
                })
            }
        }
    }

    pub fn fallback() -> Self {
        let record = PatternRecord::siteswap("3 cascade", "pattern=3");
        Self::from_record(&record).expect("fallback siteswap is valid")
    }
}

pub fn siteswap_to_jml_animation(spec: &SiteswapSpec) -> Result<JmlAnimation, String> {
    let mut matrix = MhnMatrix::from_siteswap(spec)?;
    let model = matrix.to_jml_pattern(spec)?;
    let layout = LaidoutPattern::from_jml_pattern_unchecked(&model)?;
    let mut animation = parse_jml_animation(&model.write_jml(true, true))?;
    animation.layout = Some(layout);
    Ok(animation)
}

pub fn parse_jml_animation(xml: &str) -> Result<JmlAnimation, String> {
    let wrapped = if xml.trim_start().starts_with("<pattern") {
        format!("<jml version=\"3\">{xml}</jml>")
    } else {
        strip_doctype(xml)
    };
    let doc = Document::parse(&wrapped).map_err(|err| format!("Invalid pattern JML: {err}"))?;
    let pattern = doc
        .descendants()
        .find(|node| node.has_tag_name("pattern"))
        .ok_or_else(|| "Missing <pattern> tag".to_string())?;

    let title = child_text(pattern, "title").unwrap_or_else(|| "JML pattern".to_string());
    let base_config = child(pattern, "basepattern")
        .and_then(|node| node.text().map(|value| value.trim().to_string()));

    let setup = child(pattern, "setup");
    let paths = setup
        .and_then(|node| node.attribute("paths"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3)
        .max(1);
    let jugglers = setup
        .and_then(|node| node.attribute("jugglers"))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1);
    let mut props = pattern
        .children()
        .filter(|node| node.has_tag_name("prop"))
        .map(|node| {
            PropSpec::from_jml(
                node.attribute("type").unwrap_or("ball"),
                node.attribute("mod"),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if props.is_empty() {
        props.push(PropSpec::default_for_type("ball"));
    }
    let prop_assignment = setup
        .and_then(|node| node.attribute("props"))
        .map(|value| parse_prop_assignment(value, paths))
        .unwrap_or_else(|| (0..paths).map(|index| index % props.len() + 1).collect());
    let positions = pattern
        .children()
        .filter(|node| node.has_tag_name("position"))
        .map(|node| BodyPosition {
            x: attr_f64(node, "x", 0.0),
            y: attr_f64(node, "y", 0.0),
            z: attr_f64(node, "z", 100.0),
            t: attr_f64(node, "t", 0.0),
            angle: attr_f64(node, "angle", 0.0),
            juggler: attr_usize(node, "juggler", 1).max(1),
        })
        .collect::<Vec<_>>();

    let delay_symmetry = pattern.children().find(|node| {
        node.has_tag_name("symmetry")
            && node
                .attribute("type")
                .is_some_and(|value| value.eq_ignore_ascii_case("delay"))
    });
    let parsed_path_permutation = delay_symmetry
        .and_then(|node| node.attribute("pperm"))
        .and_then(|value| Permutation::parse(paths, value, false).ok())
        .unwrap_or_else(|| Permutation::identity(paths));
    let parsed_symmetries = pattern
        .children()
        .filter(|node| node.has_tag_name("symmetry"))
        .filter_map(
            |node| match node.attribute("type")?.to_ascii_lowercase().as_str() {
                "delay" => Some(crate::mhn_symmetry::MhnSymmetryType::Delay),
                "switch" => Some(crate::mhn_symmetry::MhnSymmetryType::Switch),
                "switchdelay" => Some(crate::mhn_symmetry::MhnSymmetryType::SwitchDelay),
                _ => None,
            },
        )
        .collect::<Vec<_>>();
    let period_secs = delay_symmetry
        .and_then(|node| node.attribute("delay"))
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or_else(|| {
            pattern
                .children()
                .filter(|node| node.has_tag_name("event"))
                .filter_map(|node| node.attribute("t")?.parse::<f64>().ok())
                .fold(0.0, f64::max)
                .max(1.0)
        })
        .max(0.1);

    let mut events = Vec::new();
    for event_node in pattern.children().filter(|node| node.has_tag_name("event")) {
        let t = attr_f64(event_node, "t", 0.0);
        let x = attr_f64(event_node, "x", 0.0);
        let y = attr_f64(event_node, "y", 0.0);
        let z = attr_f64(event_node, "z", 0.0);
        let hand = event_node
            .attribute("hand")
            .unwrap_or("1:right")
            .to_string();
        let mut transitions = Vec::new();

        for transition in event_node.children().filter(|node| node.is_element()) {
            let kind = if transition.has_tag_name("throw") {
                TransitionKind::Throw
            } else if transition.has_tag_name("catch") {
                match transition.attribute("type") {
                    Some(value) if value.eq_ignore_ascii_case("soft") => TransitionKind::SoftCatch,
                    Some(value) if value.eq_ignore_ascii_case("grab") => TransitionKind::GrabCatch,
                    _ => TransitionKind::Catch,
                }
            } else if transition.has_tag_name("softcatch") {
                TransitionKind::SoftCatch
            } else if transition.has_tag_name("holding") {
                TransitionKind::Holding
            } else {
                continue;
            };

            if let Some(path) = transition
                .attribute("path")
                .and_then(|value| value.parse::<usize>().ok())
            {
                transitions.push(JmlTransition {
                    path,
                    kind,
                    throw_type: transition.attribute("type").map(str::to_string),
                    throw_mod: transition.attribute("mod").map(str::to_string),
                });
            }
        }

        events.push(JmlEvent {
            t,
            x,
            y,
            z,
            hand,
            transitions,
        });
    }

    let mut path_events = vec![Vec::new(); paths];
    for event in &events {
        for transition in &event.transitions {
            if transition.path == 0 || transition.path > paths {
                continue;
            }
            path_events[transition.path - 1].push(PathEvent {
                t: event.t,
                point: Point3 {
                    x: event.x,
                    y: event.y,
                    z: event.z,
                },
                kind: transition.kind,
            });
        }
    }
    for path in &mut path_events {
        path.sort_by(|a, b| a.t.total_cmp(&b.t));
    }

    let model = MhnJmlPattern::from_jml_xml(xml).ok();
    let convert_images = |images: Vec<crate::mhn_jml::MhnEventImage>, model: &MhnJmlPattern| {
        images
            .into_iter()
            .filter_map(|image| {
                let primary_time = model.events.get(image.primary_index)?.t;
                Some(JmlEventImage {
                    event: jml_event_from_mhn(&image.event),
                    primary_index: image.primary_index,
                    primary_time,
                })
            })
            .collect::<Vec<_>>()
    };
    let all_event_images = model
        .as_ref()
        .and_then(|model| Some(convert_images(model.all_event_images().ok()?, model)))
        .filter(|images| !images.is_empty())
        .unwrap_or_else(|| {
            events
                .iter()
                .cloned()
                .enumerate()
                .map(|(primary_index, event)| JmlEventImage {
                    primary_time: event.t,
                    event,
                    primary_index,
                })
                .collect()
        });
    let loop_event_images = all_event_images
        .iter()
        .filter(|image| image.event.t >= -1e-9 && image.event.t < period_secs - 1e-9)
        .cloned()
        .collect::<Vec<_>>();
    let path_permutation = model
        .as_ref()
        .and_then(|model| model.path_permutation().ok())
        .cloned()
        .unwrap_or(parsed_path_permutation);
    let symmetries = model
        .as_ref()
        .map(|model| {
            model
                .symmetries
                .iter()
                .map(|symmetry| symmetry.symmetry_type)
                .collect()
        })
        .unwrap_or(parsed_symmetries);
    let layout = model.and_then(|mut model| {
        model.merge_coincident_events();
        LaidoutPattern::from_jml_pattern(&model)
            .or_else(|_| LaidoutPattern::from_jml_pattern_unchecked(&model))
            .ok()
    });

    Ok(JmlAnimation {
        title,
        base_config,
        paths,
        jugglers,
        period_secs,
        props,
        prop_assignment,
        path_permutation,
        symmetries,
        positions,
        events,
        loop_event_images,
        all_event_images,
        path_events,
        layout,
    })
}

fn jml_event_from_mhn(event: &crate::mhn_jml::MhnJmlEvent) -> JmlEvent {
    JmlEvent {
        t: event.t,
        x: event.x,
        y: event.y,
        z: event.z,
        hand: format!(
            "{}:{}",
            event.juggler,
            if event.hand == 1 { "left" } else { "right" }
        ),
        transitions: event
            .transitions
            .iter()
            .map(|transition| JmlTransition {
                path: transition.path,
                kind: match transition.transition_type {
                    crate::mhn_jml::MhnJmlTransitionType::Throw => TransitionKind::Throw,
                    crate::mhn_jml::MhnJmlTransitionType::Catch => TransitionKind::Catch,
                    crate::mhn_jml::MhnJmlTransitionType::SoftCatch => TransitionKind::SoftCatch,
                    crate::mhn_jml::MhnJmlTransitionType::GrabCatch => TransitionKind::GrabCatch,
                    crate::mhn_jml::MhnJmlTransitionType::Holding => TransitionKind::Holding,
                },
                throw_type: transition.throw_type.clone(),
                throw_mod: transition.throw_mod.clone(),
            })
            .collect(),
    }
}

pub fn lerp_point(a: Point3, b: Point3, u: f64) -> Point3 {
    Point3 {
        x: a.x + (b.x - a.x) * u,
        y: a.y + (b.y - a.y) * u,
        z: a.z + (b.z - a.z) * u,
    }
}

fn child<'a>(node: Node<'a, 'a>, tag: &str) -> Option<Node<'a, 'a>> {
    node.children()
        .find(|child| child.is_element() && child.tag_name().name().eq_ignore_ascii_case(tag))
}

fn child_text(node: Node, tag: &str) -> Option<String> {
    child(node, tag).and_then(|node| node.text().map(|value| value.trim().to_string()))
}

fn attr_f64(node: Node, attr: &str, default: f64) -> f64 {
    node.attribute(attr)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(default)
}

fn attr_usize(node: Node, attr: &str, default: usize) -> usize {
    node.attribute(attr)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn parse_prop_assignment(source: &str, paths: usize) -> Vec<usize> {
    let mut result = source
        .split(',')
        .filter_map(|token| token.trim().parse::<usize>().ok())
        .collect::<Vec<_>>();
    if result.is_empty() {
        result = vec![1; paths];
    }
    let original = result.clone();
    while result.len() < paths {
        let next = original[result.len() % original.len()];
        result.push(next);
    }
    result.truncate(paths);
    result
}

fn strip_doctype(xml: &str) -> String {
    xml.lines()
        .filter(|line| !line.trim_start().starts_with("<!DOCTYPE"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn siteswap_records_compile_to_generated_jml_animation() {
        let record = PatternRecord::siteswap("Cascade", "pattern=3;bps=3");
        let spec = AnimationSpec::from_record(&record).unwrap();

        assert_eq!(spec.source_label, "siteswap/JML");
        assert!(matches!(&spec.kind, AnimationKind::Jml(jml) if jml.layout.is_some()));
        assert_eq!(spec.ball_count, 3);
    }

    #[test]
    fn library_examples_compile_to_physical_layout() {
        for config in [
            "pattern=534",
            "pattern=534;bps=3.8;hands=(-25)(2.5).(25)(-2.5).(-25)(0).;title=Mills Mess 534",
            "pattern=633",
            "pattern=633;bps=4.5;hands=(25)(17.5).(0)(25).(0)(17.5).;title=4-ball Box A",
            "pattern=(2,6x)(2x,6)*;colors=orbits",
            "pattern=744",
            "pattern=77722;bps=3",
            "pattern=77722;hss=4;bps=3",
            "pattern=24[76]42",
            "pattern=726;\nhss=3;\ntitle=oss: 726  hss: 3;\ntitle=oss: 5  hss: 123",
        ] {
            let record = PatternRecord::siteswap(config, config);
            let parsed = siteswap::parse_config(record.config.as_deref().unwrap()).unwrap();
            let conversion = siteswap_to_jml_animation(&parsed);
            assert!(
                conversion.is_ok(),
                "{config} should convert to generated JML layout: {:?}",
                conversion.err()
            );
            let spec = AnimationSpec::from_record(&record).unwrap();

            assert!(
                matches!(&spec.kind, AnimationKind::Jml(jml) if jml.layout.is_some()),
                "{config} should use generated JML physical layout"
            );
        }
    }

    #[test]
    fn thousand_and_one_prop_fixture_builds_the_physical_renderer() {
        let library = crate::jml::parse_jml(include_str!(
            "../../../../patterns/Omnikrabundi_FunWithJugglingLab.jml"
        ))
        .unwrap();
        let record = library
            .records
            .iter()
            .find(|record| record.display.contains("1001 balls"))
            .unwrap();
        let spec = AnimationSpec::from_record(record).unwrap();

        match spec.kind {
            AnimationKind::Jml(jml) => {
                assert_eq!(jml.paths, 1001);
                assert!(jml.layout.is_some());
            }
            AnimationKind::Unavailable(error) => {
                panic!("1001-ball fixture did not produce a physical layout: {error}")
            }
        }
    }

    #[test]
    fn unsupported_siteswaps_do_not_use_legacy_renderer() {
        for config in ["pattern=510", "pattern=654", "pattern=664"] {
            let record = PatternRecord::siteswap(config, config);
            match AnimationSpec::from_record(&record) {
                Ok(spec) => assert!(
                    matches!(spec.kind, AnimationKind::Unavailable(_)),
                    "{config} should be unavailable instead of rendered by a legacy fallback"
                ),
                Err(_) => {}
            }
        }
    }

    #[test]
    fn converts_siteswap_to_jml_animation_events() {
        let siteswap = siteswap::parse_config("pattern=3;bps=3").unwrap();
        let jml = siteswap_to_jml_animation(&siteswap).unwrap();

        assert_eq!(jml.paths, 3);
        assert!(jml.layout.is_some());
        assert!(jml.events.iter().any(|event| {
            event
                .transitions
                .iter()
                .any(|transition| transition.kind == TransitionKind::Throw)
        }));
        assert!(jml.events.iter().any(|event| {
            event.transitions.iter().any(|transition| {
                matches!(
                    transition.kind,
                    TransitionKind::Catch | TransitionKind::SoftCatch | TransitionKind::GrabCatch
                )
            })
        }));
    }

    #[test]
    fn parses_soft_and_grab_catch_types() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <setup jugglers="1" paths="2" props="1,1"/>
        <symmetry type="delay" pperm="(1,2)" delay="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/>
        </event>
        <event x="0" y="0" z="0" t="0.5" hand="1:left">
          <catch path="1" type="soft"/>
          <catch path="2" type="grab"/>
        </event>
        </pattern>
        </jml>
        "#;

        let jml = parse_jml_animation(xml).unwrap();
        assert!(jml.events.iter().any(|event| {
            event
                .transitions
                .iter()
                .any(|transition| transition.kind == TransitionKind::SoftCatch)
        }));
        assert!(jml.events.iter().any(|event| {
            event
                .transitions
                .iter()
                .any(|transition| transition.kind == TransitionKind::GrabCatch)
        }));
    }

    #[test]
    fn parses_props_and_path_assignments() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <prop type="square" mod="diam=14;color=green"/>
        <prop type="ring" mod="outside=28;inside=18;color=blue"/>
        <setup jugglers="1" paths="3" props="1,2"/>
        <symmetry type="delay" pperm="(1,2,3)" delay="1"/>
        </pattern>
        </jml>
        "#;

        let jml = parse_jml_animation(xml).unwrap();

        assert_eq!(jml.props.len(), 2);
        assert_eq!(jml.prop_assignment, vec![1, 2, 1]);
        assert_eq!(jml.prop_for_path(1).unwrap().diameter, 14.0);
        assert_eq!(jml.prop_for_path(2).unwrap().inside_diameter, Some(18.0));
    }

    #[test]
    fn prop_assignment_follows_delay_path_permutation_between_loops() {
        let xml = r#"
        <jml version="3"><pattern>
        <setup jugglers="1" paths="3" props="1,2,3"/>
        <prop type="ball" mod="color=red"/>
        <prop type="ring" mod="color=green"/>
        <prop type="square" mod="color=blue"/>
        <symmetry type="delay" pperm="(1,2,3)" delay="1"/>
        <event x="0" y="0" z="0" t="0" hand="1:right">
          <throw path="1" type="toss"/><catch path="2"/><holding path="3"/>
        </event>
        </pattern></jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();

        assert_eq!(jml.prop_assignment_at_time(0.25), vec![1, 2, 3]);
        assert_eq!(jml.prop_assignment_at_time(1.25), vec![2, 3, 1]);
        assert_eq!(jml.prop_assignment_at_time(2.25), vec![3, 1, 2]);
        assert_eq!(jml.period_with_props(), 3);
        assert_eq!(
            jml.prop_for_path_at_time(1, 1.25).unwrap().kind,
            crate::prop::PropKind::Ring
        );
    }

    #[test]
    fn prop_period_uses_the_shortest_repeating_assignment() {
        let xml = r#"
        <jml version="3"><pattern>
        <setup jugglers="1" paths="4" props="1,2,1,2"/>
        <prop type="ball" mod="color=red"/>
        <prop type="ball" mod="color=blue"/>
        <symmetry type="delay" pperm="(1,2,3,4)" delay="1"/>
        </pattern></jml>
        "#;
        let jml = parse_jml_animation(xml).unwrap();

        assert_eq!(jml.path_permutation.order(), 4);
        assert_eq!(jml.period_with_props(), 2);
    }

    #[test]
    fn imported_jml_builds_physical_layout_when_valid() {
        let xml = r#"
        <jml version="3">
        <pattern>
        <prop type="ball"/>
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

        let jml = parse_jml_animation(xml).unwrap();

        assert!(jml.layout.is_some());
    }
}
