use crate::mhn_hands::Coordinate;

#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    Line(LineCurve),
    Spline(SplineCurve),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineCurve {
    data: CurveData,
    a: Vec<[f64; 3]>,
    b: Vec<[f64; 3]>,
    durations: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SplineCurve {
    data: CurveData,
    a: Vec<[f64; 3]>,
    b: Vec<[f64; 3]>,
    c: Vec<[f64; 3]>,
    d: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq)]
struct CurveData {
    times: Vec<f64>,
    positions: Vec<Coordinate>,
    velocities: Vec<Option<Coordinate>>,
}

impl Curve {
    pub fn line(
        times: Vec<f64>,
        positions: Vec<Coordinate>,
        velocities: Vec<Option<Coordinate>>,
    ) -> Result<Self, String> {
        Ok(Self::Line(LineCurve::new(times, positions, velocities)?))
    }

    pub fn spline(
        times: Vec<f64>,
        positions: Vec<Coordinate>,
        velocities: Vec<Option<Coordinate>>,
    ) -> Result<Self, String> {
        Ok(Self::Spline(SplineCurve::new(
            times, positions, velocities,
        )?))
    }

    pub fn start_time(&self) -> f64 {
        match self {
            Self::Line(curve) => curve.start_time(),
            Self::Spline(curve) => curve.start_time(),
        }
    }

    pub fn end_time(&self) -> f64 {
        match self {
            Self::Line(curve) => curve.end_time(),
            Self::Spline(curve) => curve.end_time(),
        }
    }

    pub fn coordinate_at(&self, time: f64) -> Option<Coordinate> {
        match self {
            Self::Line(curve) => curve.coordinate_at(time),
            Self::Spline(curve) => curve.coordinate_at(time),
        }
    }

    pub fn max_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        match self {
            Self::Line(curve) => curve.max_between(time1, time2),
            Self::Spline(curve) => curve.max_between(time1, time2),
        }
    }

    pub fn min_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        match self {
            Self::Line(curve) => curve.min_between(time1, time2),
            Self::Spline(curve) => curve.min_between(time1, time2),
        }
    }
}

impl LineCurve {
    pub fn new(
        times: Vec<f64>,
        positions: Vec<Coordinate>,
        velocities: Vec<Option<Coordinate>>,
    ) -> Result<Self, String> {
        let data = CurveData::new(times, positions, velocities)?;
        let n = data.segment_count()?;
        let mut a = vec![[0.0; 3]; n];
        let mut b = vec![[0.0; 3]; n];
        let mut durations = vec![0.0; n];

        for i in 0..n {
            durations[i] = data.times[i + 1] - data.times[i];
            if durations[i] <= 0.0 {
                return Err("LineCurve error 2".to_string());
            }
        }

        for axis in 0..3 {
            for j in 0..n {
                let x0 = axis_value(data.positions[j], axis);
                let x1 = axis_value(data.positions[j + 1], axis);
                a[j][axis] = x0;
                b[j][axis] = (x1 - x0) / durations[j];
            }
        }

        Ok(Self {
            data,
            a,
            b,
            durations,
        })
    }

    pub fn start_time(&self) -> f64 {
        self.data.start_time()
    }

    pub fn end_time(&self) -> f64 {
        self.data.end_time()
    }

    pub fn coordinate_at(&self, time: f64) -> Option<Coordinate> {
        if time < self.start_time() || time > self.end_time() {
            return None;
        }
        let segment = self.data.segment_at(time);
        let t = time - self.data.times[segment];
        Some(Coordinate {
            x: self.a[segment][0] + t * self.b[segment][0],
            y: self.a[segment][1] + t * self.b[segment][1],
            z: self.a[segment][2] + t * self.b[segment][2],
        })
    }

    pub fn max_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        self.extreme_between(time1, time2, true)
    }

    pub fn min_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        self.extreme_between(time1, time2, false)
    }

    fn extreme_between(&self, time1: f64, time2: f64, find_max: bool) -> Option<Coordinate> {
        if time2 < self.start_time() || time1 > self.end_time() {
            return None;
        }
        let tlow = self.start_time().max(time1);
        let thigh = self.end_time().min(time2);
        let mut result = self.check(None, tlow, find_max);
        result = self.check(result, thigh, find_max);

        for i in 0..=self.data.segment_count().ok()? {
            if self.data.times[i] >= tlow && self.data.times[i] <= thigh {
                result = self.check(result, self.data.times[i], find_max);
            }
            if i != self.data.segment_count().ok()? {
                let tlow_temp = tlow.max(self.data.times[i]);
                let thigh_temp = thigh.min(self.data.times[i + 1]);
                if tlow_temp < thigh_temp {
                    result = self.check(result, tlow_temp, find_max);
                    result = self.check(result, thigh_temp, find_max);
                }
            }
        }
        result
    }

    fn check(&self, result: Option<Coordinate>, time: f64, find_max: bool) -> Option<Coordinate> {
        let loc = self.coordinate_at(time)?;
        Some(match result {
            None => loc,
            Some(result) if find_max => coordinate_max(result, loc),
            Some(result) => coordinate_min(result, loc),
        })
    }
}

impl SplineCurve {
    const MINIMIZE_RMSACCEL: i32 = 0;
    const CONTINUOUS_ACCEL: i32 = 1;
    const MINIMIZE_RMSVEL: i32 = 2;
    const SPLINE_LAYOUT_METHOD: i32 = Self::MINIMIZE_RMSACCEL;

    pub fn new(
        times: Vec<f64>,
        positions: Vec<Coordinate>,
        velocities: Vec<Option<Coordinate>>,
    ) -> Result<Self, String> {
        let data = CurveData::new(times, positions, velocities)?;
        let n = data.segment_count()?;
        let mut durations = vec![0.0; n];
        for i in 0..n {
            durations[i] = data.times[i + 1] - data.times[i];
            if durations[i] <= 0.0 {
                return Err("SplineCurve error 2".to_string());
            }
        }

        let mut velocities = data.velocities.clone();
        if velocities[0].is_some() && velocities[n].is_some() {
            Self::find_vels_edges_known(n, &durations, &data.positions, &mut velocities)?;
        } else {
            Self::find_vels_edges_unknown(n, &durations, &data.positions, &mut velocities)?;
        }

        let mut a = vec![[0.0; 3]; n];
        let mut b = vec![[0.0; 3]; n];
        let mut c = vec![[0.0; 3]; n];
        let mut d = vec![[0.0; 3]; n];

        for i in 0..n {
            let t = durations[i];
            for axis in 0..3 {
                let xi0 = axis_value(data.positions[i], axis);
                let xi1 = axis_value(data.positions[i + 1], axis);
                let vi0 = axis_value(
                    velocities[i].ok_or_else(|| "SplineCurve missing velocity".to_string())?,
                    axis,
                );
                let vi1 = axis_value(
                    velocities[i + 1].ok_or_else(|| "SplineCurve missing velocity".to_string())?,
                    axis,
                );

                a[i][axis] = xi0;
                b[i][axis] = vi0;
                c[i][axis] = (3.0 * (xi1 - xi0) - (vi1 + 2.0 * vi0) * t) / (t * t);
                d[i][axis] = (-2.0 * (xi1 - xi0) + (vi1 + vi0) * t) / (t * t * t);
            }
        }

        Ok(Self { data, a, b, c, d })
    }

    pub fn start_time(&self) -> f64 {
        self.data.start_time()
    }

    pub fn end_time(&self) -> f64 {
        self.data.end_time()
    }

    pub fn coordinate_at(&self, time: f64) -> Option<Coordinate> {
        if time < self.start_time() || time > self.end_time() {
            return None;
        }
        let segment = self.data.segment_at(time);
        let t = time - self.data.times[segment];
        Some(Coordinate {
            x: self.a[segment][0]
                + t * (self.b[segment][0] + t * (self.c[segment][0] + t * self.d[segment][0])),
            y: self.a[segment][1]
                + t * (self.b[segment][1] + t * (self.c[segment][1] + t * self.d[segment][1])),
            z: self.a[segment][2]
                + t * (self.b[segment][2] + t * (self.c[segment][2] + t * self.d[segment][2])),
        })
    }

    pub fn max_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        self.extreme_between(time1, time2, true)
    }

    pub fn min_between(&self, time1: f64, time2: f64) -> Option<Coordinate> {
        self.extreme_between(time1, time2, false)
    }

    fn extreme_between(&self, time1: f64, time2: f64, find_max: bool) -> Option<Coordinate> {
        if time2 < self.start_time() || time1 > self.end_time() {
            return None;
        }
        let tlow = self.start_time().max(time1);
        let thigh = self.end_time().min(time2);
        let n = self.data.segment_count().ok()?;
        let mut result = self.check(None, tlow, find_max);
        result = self.check(result, thigh, find_max);

        for i in 0..=n {
            if self.data.times[i] >= tlow && self.data.times[i] <= thigh {
                result = self.check(result, self.data.times[i], find_max);
            }
            if i == n {
                continue;
            }

            let tlow_temp = tlow.max(self.data.times[i]);
            let thigh_temp = thigh.min(self.data.times[i + 1]);
            if tlow_temp >= thigh_temp {
                continue;
            }

            result = self.check(result, tlow_temp, find_max);
            result = self.check(result, thigh_temp, find_max);
            for axis in 0..3 {
                if self.d[i][axis].abs() > 1.0e-6 {
                    let k =
                        self.c[i][axis] * self.c[i][axis] - 3.0 * self.b[i][axis] * self.d[i][axis];
                    if k > 0.0 {
                        let root_sign = if find_max { -1.0 } else { 1.0 };
                        let te = self.data.times[i]
                            + (-self.c[i][axis] + root_sign * k.sqrt()) / (3.0 * self.d[i][axis]);
                        if te >= tlow_temp && te <= thigh_temp {
                            result = self.check(result, te, find_max);
                        }
                    }
                } else if (find_max && self.c[i][axis] < 0.0)
                    || (!find_max && self.c[i][axis] > 0.0)
                {
                    let te = self.data.times[i] - self.b[i][axis] / (2.0 * self.c[i][axis]);
                    if te >= tlow_temp && te <= thigh_temp {
                        result = self.check(result, te, find_max);
                    }
                }
            }
        }

        result
    }

    fn check(&self, result: Option<Coordinate>, time: f64, find_max: bool) -> Option<Coordinate> {
        let loc = self.coordinate_at(time)?;
        Some(match result {
            None => loc,
            Some(result) if find_max => coordinate_max(result, loc),
            Some(result) => coordinate_min(result, loc),
        })
    }

    fn find_vels_edges_known(
        n: usize,
        t: &[f64],
        x: &[Coordinate],
        v: &mut [Option<Coordinate>],
    ) -> Result<(), String> {
        if n < 2 {
            return Ok(());
        }

        let num_catches = v[1..n].iter().filter(|velocity| velocity.is_some()).count();
        let interior = n - 1;
        let dim = 3 * interior + 2 * num_catches;
        let mut m = vec![vec![0.0; dim]; dim];
        let mut b = vec![0.0; dim];

        for axis in 0..3 {
            let v0 = axis_value(
                v[0].ok_or_else(|| "SplineCurve missing v0".to_string())?,
                axis,
            );
            let vn = axis_value(
                v[n].ok_or_else(|| "SplineCurve missing vn".to_string())?,
                axis,
            );

            for i in 0..interior {
                let xi0 = axis_value(x[i], axis);
                let xi1 = axis_value(x[i + 1], axis);
                let xi2 = axis_value(x[i + 2], axis);
                let index = i + axis * interior;

                match Self::SPLINE_LAYOUT_METHOD {
                    Self::MINIMIZE_RMSACCEL | Self::CONTINUOUS_ACCEL => {
                        m[index][index] = 2.0 / t[i] + 2.0 / t[i + 1];
                        let offdiag = if i == n - 2 { 0.0 } else { 1.0 / t[i + 1] };
                        if index < 3 * interior - 1 {
                            m[index][index + 1] = offdiag;
                            m[index + 1][index] = offdiag;
                        }

                        b[index] = 3.0 * (xi2 - xi1) / (t[i + 1] * t[i + 1])
                            + 3.0 * (xi1 - xi0) / (t[i] * t[i]);
                        if i == 0 {
                            b[index] -= v0 / t[0];
                        }
                        if i == n - 2 {
                            b[index] -= vn / t[n - 1];
                        }
                    }
                    Self::MINIMIZE_RMSVEL => {
                        m[index][index] = 4.0 * (t[i] + t[i + 1]);
                        let offdiag = if i == n - 2 { 0.0 } else { -t[i + 1] };
                        if index < 3 * interior - 1 {
                            m[index][index + 1] = offdiag;
                            m[index + 1][index] = offdiag;
                        }

                        b[index] = 3.0 * (xi2 - xi0);
                        if i == 0 {
                            b[index] += v0 * t[0];
                        }
                        if i == n - 2 {
                            b[index] += vn * t[n - 1];
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }

        let mut catch_num = 0;
        for i in 0..interior {
            let Some(catch_velocity) = v[i + 1] else {
                continue;
            };
            let index = 3 * interior + 2 * catch_num;
            let ci0 = catch_velocity.x;
            let ci1 = catch_velocity.y;
            let ci2 = catch_velocity.z;
            let large_axis = if ci1.abs() >= ci0.abs().max(ci2.abs()) {
                1
            } else if ci2.abs() >= ci0.abs().max(ci1.abs()) {
                2
            } else {
                0
            };

            match large_axis {
                0 => {
                    m[index][i] = ci2;
                    m[i][index] = ci2;
                    m[index + 1][i] = ci1;
                    m[i][index + 1] = ci1;
                    m[index + 1][i + interior] = -ci0;
                    m[i + interior][index + 1] = -ci0;
                    m[index][i + 2 * interior] = -ci0;
                    m[i + 2 * interior][index] = -ci0;
                }
                1 => {
                    m[index + 1][i] = ci1;
                    m[i][index + 1] = ci1;
                    m[index][i + interior] = ci2;
                    m[i + interior][index] = ci2;
                    m[index + 1][i + interior] = -ci0;
                    m[i + interior][index + 1] = -ci0;
                    m[index][i + 2 * interior] = -ci1;
                    m[i + 2 * interior][index] = -ci1;
                }
                2 => {
                    m[index + 1][i] = ci2;
                    m[i][index + 1] = ci2;
                    m[index][i + interior] = ci2;
                    m[i + interior][index] = ci2;
                    m[index][i + 2 * interior] = -ci1;
                    m[i + 2 * interior][index] = -ci1;
                    m[index + 1][i + 2 * interior] = -ci0;
                    m[i + 2 * interior][index + 1] = -ci0;
                }
                _ => unreachable!(),
            }
            catch_num += 1;
        }

        let solution = solve_linear_system(&m, &b)?;
        for i in 0..interior {
            v[i + 1] = Some(Coordinate {
                x: solution[i],
                y: solution[i + interior],
                z: solution[i + 2 * interior],
            });
        }
        Ok(())
    }

    fn find_vels_edges_unknown(
        n: usize,
        t: &[f64],
        x: &[Coordinate],
        v: &mut [Option<Coordinate>],
    ) -> Result<(), String> {
        if n < 1 {
            return Ok(());
        }

        let mut adiag = vec![0.0; n];
        let mut aoffd = vec![0.0; n];
        let mut acorner;
        let mut rhs = vec![0.0; n];

        for velocity in v.iter_mut().take(n) {
            *velocity = Some(Coordinate::default());
        }

        for axis in 0..3 {
            acorner = 0.0;
            let xn0 = axis_value(x[n], axis);
            let xnm1 = axis_value(x[n - 1], axis);

            for i in 0..n {
                let xi0 = axis_value(x[i], axis);
                let xi1 = axis_value(x[i + 1], axis);
                let xim1 = if i == 0 {
                    0.0
                } else {
                    axis_value(x[i - 1], axis)
                };

                match Self::SPLINE_LAYOUT_METHOD {
                    Self::MINIMIZE_RMSACCEL | Self::CONTINUOUS_ACCEL => {
                        if i == 0 {
                            adiag[i] = 2.0 / t[n - 1] + 2.0 / t[0];
                            acorner = 1.0 / t[n - 1];
                            rhs[i] = 3.0 * (xi1 - xi0) / (t[0] * t[0])
                                + 3.0 * (xn0 - xnm1) / (t[n - 1] * t[n - 1]);
                        } else {
                            adiag[i] = 2.0 / t[i - 1] + 2.0 / t[i];
                            rhs[i] = 3.0 * (xi1 - xi0) / (t[i] * t[i])
                                + 3.0 * (xi0 - xim1) / (t[i - 1] * t[i - 1]);
                        }
                        aoffd[i] = 1.0 / t[i];
                    }
                    Self::MINIMIZE_RMSVEL => {
                        if i == 0 {
                            adiag[i] = 4.0 * (t[n - 1] + t[0]);
                            acorner = -t[n - 1];
                            rhs[i] = 3.0 * (xn0 - xnm1 + xi1 - xi0);
                        } else {
                            adiag[i] = 4.0 * (t[i - 1] + t[i]);
                            rhs[i] = 3.0 * (xi1 - xim1);
                        }
                        aoffd[i] = -t[i];
                    }
                    _ => unreachable!(),
                }
            }

            let mut vel = (0..n)
                .map(|index| axis_value(v[index].unwrap_or_default(), axis))
                .collect::<Vec<_>>();

            tridag(&aoffd, &adiag, &aoffd, &rhs, &mut vel, n)?;

            if n > 2 {
                let mut z1 = vec![0.0; n];
                rhs[0] = acorner;
                for value in rhs.iter_mut().take(n).skip(1) {
                    *value = 0.0;
                }
                tridag(&aoffd, &adiag, &aoffd, &rhs, &mut z1, n)?;

                let mut z2 = vec![0.0; n];
                rhs[n - 1] = acorner;
                for value in rhs.iter_mut().take(n - 1) {
                    *value = 0.0;
                }
                tridag(&aoffd, &adiag, &aoffd, &rhs, &mut z2, n)?;

                let mut h00 = 1.0 + z2[0];
                let mut h01 = -z2[n - 1];
                let mut h10 = -z1[0];
                let mut h11 = 1.0 + z1[n - 1];
                let det = h00 * h11 - h01 * h10;
                h00 /= det;
                h01 /= det;
                h10 /= det;
                h11 /= det;

                let m0 = h00 * vel[n - 1] + h01 * vel[0];
                let m1 = h10 * vel[n - 1] + h11 * vel[0];
                for i in 0..n {
                    vel[i] -= z1[i] * m0 + z2[i] * m1;
                }
            }

            for i in 0..n {
                let mut coord = v[i].unwrap_or_default();
                set_axis_value(&mut coord, axis, vel[i]);
                v[i] = Some(coord);
            }
        }

        v[n] = v[0];
        Ok(())
    }
}

impl CurveData {
    fn new(
        times: Vec<f64>,
        positions: Vec<Coordinate>,
        velocities: Vec<Option<Coordinate>>,
    ) -> Result<Self, String> {
        if times.len() != positions.len() || times.len() != velocities.len() {
            return Err("Curve error 1".to_string());
        }
        if times.len() < 2 {
            return Err("Curve error 2".to_string());
        }
        Ok(Self {
            times,
            positions,
            velocities,
        })
    }

    fn segment_count(&self) -> Result<usize, String> {
        self.times
            .len()
            .checked_sub(1)
            .filter(|count| *count >= 1)
            .ok_or_else(|| "Curve segment error".to_string())
    }

    fn start_time(&self) -> f64 {
        self.times[0]
    }

    fn end_time(&self) -> f64 {
        self.times[self.times.len() - 1]
    }

    fn segment_at(&self, time: f64) -> usize {
        let n = self.times.len() - 1;
        let mut segment = 0;
        while segment < n {
            if time <= self.times[segment + 1] {
                break;
            }
            segment += 1;
        }
        segment.min(n - 1)
    }
}

impl Default for Coordinate {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }
}

fn axis_value(coordinate: Coordinate, axis: usize) -> f64 {
    match axis {
        0 => coordinate.x,
        1 => coordinate.y,
        2 => coordinate.z,
        _ => unreachable!("coordinate axis"),
    }
}

fn set_axis_value(coordinate: &mut Coordinate, axis: usize, value: f64) {
    match axis {
        0 => coordinate.x = value,
        1 => coordinate.y = value,
        2 => coordinate.z = value,
        _ => unreachable!("coordinate axis"),
    }
}

fn coordinate_max(left: Coordinate, right: Coordinate) -> Coordinate {
    Coordinate {
        x: left.x.max(right.x),
        y: left.y.max(right.y),
        z: left.z.max(right.z),
    }
}

fn coordinate_min(left: Coordinate, right: Coordinate) -> Coordinate {
    Coordinate {
        x: left.x.min(right.x),
        y: left.y.min(right.y),
        z: left.z.min(right.z),
    }
}

fn tridag(
    a: &[f64],
    b: &[f64],
    c: &[f64],
    r: &[f64],
    u: &mut [f64],
    n: usize,
) -> Result<(), String> {
    if b[0] == 0.0 {
        return Err("Error 1 in tridag()".to_string());
    }

    let mut bet = b[0];
    let mut gam = vec![0.0; n];
    u[0] = r[0] / bet;
    for j in 1..n {
        gam[j] = c[j - 1] / bet;
        bet = b[j] - a[j - 1] * gam[j];
        if bet == 0.0 {
            return Err("Error 2 in tridag()".to_string());
        }
        u[j] = (r[j] - a[j - 1] * u[j - 1]) / bet;
    }
    for j in (1..n).rev() {
        u[j - 1] -= gam[j] * u[j];
    }
    Ok(())
}

fn solve_linear_system(a: &[Vec<f64>], b: &[f64]) -> Result<Vec<f64>, String> {
    let n = b.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    if a.len() != n || a.iter().any(|row| row.len() != n) {
        return Err("Dimension mismatch in solveLinearSystem".to_string());
    }

    let mut m = a.to_vec();
    let mut x = b.to_vec();

    for i in 0..n {
        let mut pivot_row = i;
        for j in (i + 1)..n {
            if m[j][i].abs() > m[pivot_row][i].abs() {
                pivot_row = j;
            }
        }

        m.swap(i, pivot_row);
        x.swap(i, pivot_row);

        if m[i][i].abs() < 1e-12 {
            return Err("Singular matrix in solveLinearSystem".to_string());
        }

        for j in (i + 1)..n {
            let factor = m[j][i] / m[i][i];
            x[j] -= factor * x[i];
            for k in i..n {
                m[j][k] -= factor * m[i][k];
            }
        }
    }

    for i in (0..n).rev() {
        let mut sum = 0.0;
        for j in (i + 1)..n {
            sum += m[i][j] * x[j];
        }
        x[i] = (x[i] - sum) / m[i][i];
    }
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coord(x: f64, y: f64, z: f64) -> Coordinate {
        Coordinate { x, y, z }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "actual={actual}, expected={expected}"
        );
    }

    #[test]
    fn line_curve_interpolates_points() {
        let curve = LineCurve::new(
            vec![0.0, 1.0, 3.0],
            vec![
                coord(0.0, 0.0, 0.0),
                coord(2.0, 4.0, 6.0),
                coord(6.0, 4.0, 2.0),
            ],
            vec![None, None, None],
        )
        .unwrap();

        assert_eq!(curve.coordinate_at(0.0).unwrap(), coord(0.0, 0.0, 0.0));
        assert_eq!(curve.coordinate_at(1.0).unwrap(), coord(2.0, 4.0, 6.0));
        assert_eq!(curve.coordinate_at(2.0).unwrap(), coord(4.0, 4.0, 4.0));
    }

    #[test]
    fn spline_curve_respects_known_endpoint_velocities() {
        let curve = SplineCurve::new(
            vec![0.0, 1.0],
            vec![coord(0.0, 2.0, 4.0), coord(1.0, 3.0, 5.0)],
            vec![Some(coord(1.0, 1.0, 1.0)), Some(coord(1.0, 1.0, 1.0))],
        )
        .unwrap();

        let mid = curve.coordinate_at(0.5).unwrap();
        assert_close(mid.x, 0.5);
        assert_close(mid.y, 2.5);
        assert_close(mid.z, 4.5);
    }

    #[test]
    fn cyclic_spline_unknown_edges_returns_to_start() {
        let curve = SplineCurve::new(
            vec![0.0, 1.0, 2.0],
            vec![
                coord(0.0, 0.0, 0.0),
                coord(10.0, 0.0, 0.0),
                coord(0.0, 0.0, 0.0),
            ],
            vec![None, None, None],
        )
        .unwrap();

        assert_eq!(curve.coordinate_at(0.0).unwrap(), coord(0.0, 0.0, 0.0));
        assert_eq!(curve.coordinate_at(2.0).unwrap(), coord(0.0, 0.0, 0.0));
        assert!(curve.coordinate_at(1.0).unwrap().x > 9.999);
    }
}
