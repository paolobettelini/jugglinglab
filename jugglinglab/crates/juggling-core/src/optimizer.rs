use crate::layout::{LaidoutPattern, PathLink, PathLinkKind};
use crate::mhn_jml::{MhnJmlPattern, MhnJmlTransitionType};
use crate::mhn_symmetry::MhnSymmetryType;
use crate::parameter_list::ParameterList;
use crate::prop::PropSpec;
use good_lp::{Expression, ProblemVariables, Solution, SolverModel, Variable, microlp, variable};

const EQUATION_EPSILON: f64 = 0.000_001;
const OPTIMIZER_EPSILON: f64 = 0.000_000_1;

#[derive(Clone, Debug, PartialEq)]
pub struct OptimizationResult {
    pub pattern: MhnJmlPattern,
    pub margin_equations: usize,
    pub stages: usize,
    pub initial_margin: Option<f64>,
    pub final_margin: Option<f64>,
}

#[derive(Clone, Debug)]
struct LinearEquation {
    coefficients: Vec<f64>,
    done: bool,
}

impl LinearEquation {
    fn coefficient(&self, column: usize) -> f64 {
        self.coefficients[column]
    }

    fn constant(&self, variables: usize) -> f64 {
        self.coefficients[variables]
    }
}

#[derive(Clone, Debug)]
struct MarginEquations {
    variable_event_indices: Vec<usize>,
    variable_values: Vec<f64>,
    variable_minimums: Vec<f64>,
    variable_maximums: Vec<f64>,
    equations: Vec<LinearEquation>,
}

impl MarginEquations {
    fn from_pattern(pattern: &MhnJmlPattern) -> Result<Self, String> {
        if pattern.number_of_jugglers > 1 {
            return Err("Optimizer does not support passing patterns".to_string());
        }

        let layout = LaidoutPattern::from_jml_pattern(pattern)?;
        if layout.is_bounce_pattern() {
            return Err("Optimizer does not support bounce patterns".to_string());
        }

        let mut variable_event_indices = Vec::new();
        let mut max_value: f64 = 0.0;
        let mut gravity = 980.0;

        for (event_index, event) in pattern.events.iter().enumerate() {
            let mut is_variable = false;
            for transition in &event.transitions {
                if transition.transition_type.is_throw_or_catch() {
                    is_variable = true;
                    max_value = max_value.max(event.x.abs());

                    if transition.transition_type == MhnJmlTransitionType::Throw {
                        if let Ok(parameters) =
                            ParameterList::parse(transition.throw_mod.as_deref())
                        {
                            if let Some(value) = parameters
                                .get_parameter("g")
                                .and_then(|value| value.parse::<f64>().ok())
                                .filter(|value| value.is_finite())
                            {
                                gravity = value;
                            }
                        }
                    }
                    break;
                }
            }
            if is_variable {
                variable_event_indices.push(event_index);
            }
        }

        let mut variable_values = Vec::with_capacity(variable_event_indices.len());
        let mut variable_minimums = Vec::with_capacity(variable_event_indices.len());
        let mut variable_maximums = Vec::with_capacity(variable_event_indices.len());
        for &event_index in &variable_event_indices {
            let event = &pattern.events[event_index];
            let first_is_throw = event.transitions.first().is_some_and(|transition| {
                transition.transition_type == MhnJmlTransitionType::Throw
            });

            variable_values.push(event.x);
            if event.x > 0.0 {
                variable_minimums.push(0.1 * max_value);
                variable_maximums.push(if first_is_throw {
                    0.9 * max_value
                } else {
                    max_value
                });
            } else {
                variable_minimums.push(if first_is_throw {
                    -0.9 * max_value
                } else {
                    -max_value
                });
                variable_maximums.push(-0.1 * max_value);
            }
        }

        let prop_radius = pattern.props.iter().try_fold(0.0_f64, |radius, prop| {
            let spec = PropSpec::from_jml(&prop.prop_type, prop.modifier.as_deref())?;
            Ok::<_, String>(radius.max(spec.optimizer_radius_cm()))
        })?;

        let primary_links = layout
            .path_links
            .iter()
            .flatten()
            .filter(|link| {
                !matches!(link.kind, PathLinkKind::InHand { .. })
                    && layout.events[link.start_event_index].is_primary
            })
            .collect::<Vec<_>>();

        let mut symmetry_delay = -1.0;
        let mut switch_delay = false;
        for symmetry in &pattern.symmetries {
            match symmetry.symmetry_type {
                MhnSymmetryType::Delay => symmetry_delay = symmetry.delay,
                MhnSymmetryType::SwitchDelay => switch_delay = true,
                MhnSymmetryType::Switch => {
                    return Err("Optimizer does not support switch symmetry".to_string());
                }
            }
        }

        if !symmetry_delay.is_finite() || symmetry_delay <= 0.0 {
            return Err("Optimizer requires a positive delay symmetry".to_string());
        }

        let variables = variable_event_indices.len();
        let mut raw_equations = Vec::<Vec<f64>>::new();
        for first in &primary_links {
            for second in &primary_links {
                Self::add_collision_equations(
                    pattern,
                    &layout,
                    &variable_event_indices,
                    &variable_values,
                    variables,
                    gravity,
                    prop_radius,
                    symmetry_delay,
                    switch_delay,
                    first,
                    second,
                    &mut raw_equations,
                )?;
            }
        }

        let mut equation_index = 1;
        while equation_index < raw_equations.len() {
            let duplicate = raw_equations[..equation_index].iter().any(|other| {
                raw_equations[equation_index]
                    .iter()
                    .zip(other)
                    .all(|(left, right)| (left - right).abs() <= EQUATION_EPSILON)
            });
            if duplicate {
                raw_equations.remove(equation_index);
            } else {
                equation_index += 1;
            }
        }

        let mut equations = raw_equations
            .into_iter()
            .map(|coefficients| LinearEquation {
                coefficients,
                done: false,
            })
            .collect::<Vec<_>>();
        equations.sort_by(|left, right| {
            Self::margin_for(left, &variable_values)
                .total_cmp(&Self::margin_for(right, &variable_values))
        });

        Ok(Self {
            variable_event_indices,
            variable_values,
            variable_minimums,
            variable_maximums,
            equations,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn add_collision_equations(
        pattern: &MhnJmlPattern,
        layout: &LaidoutPattern,
        variable_event_indices: &[usize],
        variable_values: &[f64],
        variables: usize,
        gravity: f64,
        prop_radius: f64,
        symmetry_delay: f64,
        switch_delay: bool,
        first: &PathLink,
        second: &PathLink,
        equations: &mut Vec<Vec<f64>>,
    ) -> Result<(), String> {
        let first_start_event = &layout.events[first.start_event_index];
        let first_end_event = &layout.events[first.end_event_index];
        let second_start_event = &layout.events[second.start_event_index];
        let second_end_event = &layout.events[second.end_event_index];
        let first_start = first_start_event.event.t;
        let first_end = first_end_event.event.t;
        let second_start = second_start_event.event.t;
        let second_end = second_end_event.event.t;
        let mut delay = 0.0;
        let mut invert_second = false;

        loop {
            let mut can_collide = true;
            if delay == 0.0 && first.start_event_index == second.start_event_index {
                can_collide = false;
            }
            if first_start > second_start + delay {
                can_collide = false;
            } else if first_start == second_start + delay {
                if first_end > second_end + delay {
                    can_collide = false;
                } else if first_end == second_end + delay {
                    if first_start_event.event.juggler > second_start_event.event.juggler {
                        can_collide = false;
                    } else if first_start_event.event.juggler == second_start_event.event.juggler
                        && first_start_event.event.hand == 1
                    {
                        can_collide = false;
                    }
                }
            }

            let same_denominator =
                second_start + second_end + 2.0 * delay - first_start - first_end;
            if same_denominator == 0.0 {
                can_collide = false;
            }

            let mut same_time = -1.0;
            if can_collide {
                same_time = ((second_start + delay) * (second_end + delay)
                    - first_start * first_end)
                    / same_denominator;
                if same_time < first_start
                    || same_time > first_end
                    || same_time < second_start + delay
                    || same_time > second_end + delay
                {
                    can_collide = false;
                }
            }

            if can_collide {
                let throw_time_1 = first_start;
                let catch_time_1 = first_end;
                let throw_time_2 = second_start + delay;
                let catch_time_2 = second_end + delay;
                let vertical_velocity_1 = 0.5 * gravity * (catch_time_1 - throw_time_1);
                let vertical_velocity_2 = 0.5 * gravity * (catch_time_2 - throw_time_2);
                let mut denominator = vertical_velocity_1 * (same_time - throw_time_1)
                    + vertical_velocity_2 * (same_time - throw_time_2);

                if denominator > OPTIMIZER_EPSILON {
                    denominator *= std::f64::consts::PI / 180.0;
                    let mut throw_coefficient_1 =
                        (catch_time_1 - same_time) / ((catch_time_1 - throw_time_1) * denominator);
                    let mut catch_coefficient_1 =
                        (same_time - throw_time_1) / ((catch_time_1 - throw_time_1) * denominator);
                    let mut throw_coefficient_2 =
                        -(catch_time_2 - same_time) / ((catch_time_2 - throw_time_2) * denominator);
                    let mut catch_coefficient_2 =
                        -(same_time - throw_time_2) / ((catch_time_2 - throw_time_2) * denominator);
                    let constant = -2.0 * prop_radius / denominator;

                    let throw_variable_1 = Self::variable_for_layout_event(
                        pattern,
                        first_start_event,
                        variable_event_indices,
                        &mut throw_coefficient_1,
                    )?;
                    let catch_variable_1 = Self::variable_for_layout_event(
                        pattern,
                        first_end_event,
                        variable_event_indices,
                        &mut catch_coefficient_1,
                    )?;
                    let throw_variable_2 = Self::variable_for_layout_event(
                        pattern,
                        second_start_event,
                        variable_event_indices,
                        &mut throw_coefficient_2,
                    )?;
                    let catch_variable_2 = Self::variable_for_layout_event(
                        pattern,
                        second_end_event,
                        variable_event_indices,
                        &mut catch_coefficient_2,
                    )?;

                    if invert_second {
                        throw_coefficient_2 = -throw_coefficient_2;
                        catch_coefficient_2 = -catch_coefficient_2;
                    }

                    let mut coefficients = vec![0.0; variables + 1];
                    coefficients[throw_variable_1] += throw_coefficient_1;
                    coefficients[catch_variable_1] += catch_coefficient_1;
                    coefficients[throw_variable_2] += throw_coefficient_2;
                    coefficients[catch_variable_2] += catch_coefficient_2;
                    coefficients[variables] = constant;

                    let distance = coefficients[..variables]
                        .iter()
                        .zip(variable_values)
                        .map(|(coefficient, value)| coefficient * value)
                        .sum::<f64>();
                    if distance < 0.0 {
                        for coefficient in &mut coefficients[..variables] {
                            if *coefficient != 0.0 {
                                *coefficient = -*coefficient;
                            }
                        }
                    }
                    equations.push(coefficients);
                }
            }

            if switch_delay {
                delay += 0.5 * symmetry_delay;
                invert_second = !invert_second;
            } else {
                delay += symmetry_delay;
            }
            if first_end <= second_start + delay {
                break;
            }
        }

        Ok(())
    }

    fn variable_for_layout_event(
        pattern: &MhnJmlPattern,
        event: &crate::layout::LayoutEvent,
        variable_event_indices: &[usize],
        coefficient: &mut f64,
    ) -> Result<usize, String> {
        let primary = pattern
            .events
            .get(event.primary_index)
            .ok_or_else(|| "Could not find primary event in variable events".to_string())?;
        if !event.is_primary && event.event.hand != primary.hand {
            *coefficient = -*coefficient;
        }
        variable_event_indices
            .iter()
            .position(|index| *index == event.primary_index)
            .ok_or_else(|| "Could not find primary event in variable events".to_string())
    }

    fn margin_for(equation: &LinearEquation, values: &[f64]) -> f64 {
        let linear = equation
            .coefficients
            .iter()
            .take(values.len())
            .zip(values)
            .map(|(coefficient, value)| coefficient * value)
            .sum::<f64>();
        linear.abs() + equation.constant(values.len())
    }

    fn minimum_margin(&self) -> Option<f64> {
        self.equations
            .iter()
            .map(|equation| Self::margin_for(equation, &self.variable_values))
            .min_by(f64::total_cmp)
    }
}

struct Optimizer {
    equations: MarginEquations,
    pinned: Vec<bool>,
}

impl Optimizer {
    fn new(pattern: &MhnJmlPattern) -> Result<Self, String> {
        let equations = MarginEquations::from_pattern(pattern)?;
        let pinned = vec![false; equations.variable_values.len()];
        Ok(Self { equations, pinned })
    }

    fn solve_stage(&mut self) -> Result<(), String> {
        let variable_count = self.equations.variable_values.len();
        let mut variables = ProblemVariables::new();
        let x = (0..variable_count)
            .map(|index| {
                (!self.pinned[index]).then(|| {
                    variables.add(
                        variable()
                            .min(self.equations.variable_minimums[index])
                            .max(self.equations.variable_maximums[index]),
                    )
                })
            })
            .collect::<Vec<Option<Variable>>>();
        let z = self
            .equations
            .equations
            .iter()
            .map(|equation| (!equation.done).then(|| variables.add(variable().binary())))
            .collect::<Vec<Option<Variable>>>();
        let error = variables.add(variable().min(f64::NEG_INFINITY).max(f64::INFINITY));
        let mut problem = variables.maximise(error).using(microlp);

        for (equation_index, equation) in self.equations.equations.iter().enumerate() {
            if equation.done {
                continue;
            }

            let mut maximum_ax = 0.0;
            let mut minimum_ax = 0.0;
            for variable_index in 0..variable_count {
                let coefficient = equation.coefficient(variable_index);
                if coefficient > 0.0 {
                    maximum_ax += coefficient * self.equations.variable_maximums[variable_index];
                    minimum_ax += coefficient * self.equations.variable_minimums[variable_index];
                } else {
                    maximum_ax += coefficient * self.equations.variable_minimums[variable_index];
                    minimum_ax += coefficient * self.equations.variable_maximums[variable_index];
                }
            }
            let bound = 2.0 * maximum_ax.abs().max(minimum_ax.abs()) + 1.0;
            let binary = z[equation_index].expect("active equation must have binary variable");

            let mut first_rhs = equation.constant(variable_count);
            let mut first_lhs = Expression::from(error);
            first_lhs.add_mul(-bound, binary);
            for variable_index in 0..variable_count {
                let coefficient = equation.coefficient(variable_index);
                if self.pinned[variable_index] {
                    first_rhs += coefficient * self.equations.variable_values[variable_index];
                } else if coefficient != 0.0 {
                    first_lhs.add_mul(-coefficient, x[variable_index].expect("variable missing"));
                }
            }
            problem.add_constraint(first_lhs.leq(first_rhs));

            let mut second_rhs = equation.constant(variable_count) + bound;
            let mut second_lhs = Expression::from(error);
            second_lhs.add_mul(bound, binary);
            for variable_index in 0..variable_count {
                let coefficient = equation.coefficient(variable_index);
                if self.pinned[variable_index] {
                    second_rhs -= coefficient * self.equations.variable_values[variable_index];
                } else if coefficient != 0.0 {
                    second_lhs.add_mul(coefficient, x[variable_index].expect("variable missing"));
                }
            }
            problem.add_constraint(second_lhs.leq(second_rhs));
        }

        let solution = problem
            .solve()
            .map_err(|_| "Optimizer failed to find solution".to_string())?;
        for (index, variable) in x.into_iter().enumerate() {
            if let Some(variable) = variable {
                self.equations.variable_values[index] = solution.value(variable);
            }
        }
        Ok(())
    }

    fn mark_finished(&mut self) {
        let minimum_margin = self
            .equations
            .equations
            .iter()
            .filter(|equation| !equation.done)
            .map(|equation| MarginEquations::margin_for(equation, &self.equations.variable_values))
            .min_by(f64::total_cmp)
            .unwrap_or(f64::INFINITY);

        for equation in &self.equations.equations {
            if equation.done {
                continue;
            }
            let margin = MarginEquations::margin_for(equation, &self.equations.variable_values);
            if (margin - minimum_margin).abs() > OPTIMIZER_EPSILON {
                continue;
            }
            for (index, pinned) in self.pinned.iter_mut().enumerate() {
                let coefficient = equation.coefficient(index);
                if !*pinned && coefficient.abs() > OPTIMIZER_EPSILON {
                    *pinned = true;
                }
            }
        }

        for equation in &mut self.equations.equations {
            if equation.done {
                continue;
            }
            equation.done = (0..self.pinned.len()).all(|index| {
                self.pinned[index] || equation.coefficient(index).abs() <= OPTIMIZER_EPSILON
            });
        }
    }

    fn optimize(&mut self) -> Result<usize, String> {
        let mut stages = 0;
        while self
            .equations
            .equations
            .iter()
            .any(|equation| !equation.done)
        {
            stages += 1;
            self.solve_stage()?;
            self.mark_finished();
        }
        Ok(stages)
    }

    fn update_pattern(&self, pattern: &mut MhnJmlPattern) {
        for (variable_index, &event_index) in
            self.equations.variable_event_indices.iter().enumerate()
        {
            if self.pinned[variable_index] {
                pattern.events[event_index].x =
                    round_to_hundredth(self.equations.variable_values[variable_index]);
            }
        }
        pattern.rebuild_path_events();
    }
}

pub fn optimize_pattern(pattern: &MhnJmlPattern) -> Result<OptimizationResult, String> {
    let mut optimizer = Optimizer::new(pattern)?;
    let margin_equations = optimizer.equations.equations.len();
    let initial_margin = optimizer.equations.minimum_margin();
    let stages = if margin_equations == 0 {
        0
    } else {
        optimizer.optimize()?
    };
    let final_margin = optimizer.equations.minimum_margin();
    let mut optimized = pattern.clone();
    optimizer.update_pattern(&mut optimized);
    optimized.assert_valid()?;

    Ok(OptimizationResult {
        pattern: optimized,
        margin_equations,
        stages,
        initial_margin,
        final_margin,
    })
}

fn round_to_hundredth(value: f64) -> f64 {
    (value.mul_add(100.0, 0.5)).floor() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mhn_matrix::MhnMatrix;
    use crate::siteswap;

    fn siteswap_pattern(config: &str) -> MhnJmlPattern {
        let spec = siteswap::parse_config(config).unwrap();
        MhnMatrix::from_siteswap(&spec)
            .unwrap()
            .to_jml_pattern(&spec)
            .unwrap()
    }

    #[test]
    fn optimizer_is_a_noop_without_collision_equations() {
        let pattern = siteswap_pattern("pattern=1");
        let result = optimize_pattern(&pattern).unwrap();
        assert_eq!(result.margin_equations, 0);
        assert_eq!(result.stages, 0);
        assert_eq!(result.pattern, pattern);
    }

    #[test]
    fn optimizer_improves_or_preserves_the_minimum_margin() {
        let pattern = siteswap_pattern("pattern=531");
        let result = optimize_pattern(&pattern).unwrap();
        assert!(result.margin_equations > 0);
        assert!(result.stages > 0);
        assert!(result.final_margin.unwrap() + OPTIMIZER_EPSILON >= result.initial_margin.unwrap());
        result.pattern.assert_valid().unwrap();
        LaidoutPattern::from_jml_pattern(&result.pattern).unwrap();
    }

    #[test]
    fn optimizer_rejects_passing_and_bounce_patterns_like_original() {
        let passing = siteswap_pattern("pattern=<3p|3p>");
        assert_eq!(
            optimize_pattern(&passing).unwrap_err(),
            "Optimizer does not support passing patterns"
        );

        let mut bouncing = siteswap_pattern("pattern=3");
        let throw = bouncing
            .events
            .iter_mut()
            .flat_map(|event| &mut event.transitions)
            .find(|transition| transition.transition_type == MhnJmlTransitionType::Throw)
            .unwrap();
        throw.throw_type = Some("bounce".to_string());
        throw.throw_mod = Some("bounces=1".to_string());
        bouncing.rebuild_path_events();
        assert_eq!(
            optimize_pattern(&bouncing).unwrap_err(),
            "Optimizer does not support bounce patterns"
        );
    }

    #[test]
    fn hundredth_rounding_matches_kotlin_ties() {
        assert_eq!(round_to_hundredth(1.234), 1.23);
        assert_eq!(round_to_hundredth(1.235), 1.24);
        assert_eq!(round_to_hundredth(-1.125), -1.12);
    }
}
