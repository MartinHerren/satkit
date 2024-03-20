use nalgebra as na;

use crate::orbitprop;
use crate::orbitprop::PropSettings;
use crate::AstroTime;
use crate::SKResult;

type PVCovType = na::SMatrix<f64, 6, 6>;

#[derive(Clone, Debug)]
pub enum StateCov {
    None,
    PVCov(PVCovType),
}

#[derive(Clone, Debug)]
pub struct SatState {
    pub time: AstroTime,
    pub pv: na::Vector6<f64>,
    pub cov: StateCov,
}

impl SatState {
    pub fn from_pv(time: &AstroTime, pos: &na::Vector3<f64>, vel: &na::Vector3<f64>) -> SatState {
        SatState {
            time: time.clone(),
            pv: na::vector![pos[0], pos[1], pos[2], vel[0], vel[1], vel[2]],
            cov: StateCov::None,
        }
    }

    pub fn pos(&self) -> na::Vector3<f64> {
        self.pv.fixed_view::<3, 1>(0, 0).into()
    }

    pub fn vel(&self) -> na::Vector3<f64> {
        self.pv.fixed_view::<3, 1>(3, 0).into()
    }

    /// set covariance
    ///
    /// # Arguments
    ///
    /// * `cov` -  Covariance matrix.  6x6 or larger if including terms like drag.
    ///            Upper-left 6x6 is covariance for position & velocity, in units of
    ///            meters and meters / second
    ///
    pub fn set_cov(&mut self, cov: StateCov) {
        self.cov = cov;
    }

    /// Return Quaternion to go from gcrf (Geocentric Celestial Reference Frame)
    /// to lvlh (Local-Vertical, Local-Horizontal) frame
    ///
    /// Note: lvlh:
    ///       z axis = -r (nadir)
    ///       y axis = -h (h = p cross v)
    ///       x axis such that x cross y = z
    pub fn qgcrf2lvlh(&self) -> na::UnitQuaternion<f64> {
        type Quat = na::UnitQuaternion<f64>;

        let p = self.pos();
        let v = self.vel();
        let h = p.cross(&v);
        let q1 = Quat::rotation_between(&(-1.0 * p), &na::Vector3::z_axis()).unwrap();
        let q2 = Quat::rotation_between(&(-1.0 * (q1 * h)), &na::Vector3::y_axis()).unwrap();
        q2 * q1
    }

    /// Set position uncertainty (1-sigma, meters) in the
    /// lvlh (local-vertical, local-horizontal) frame
    ///
    /// # Arguments
    ///
    /// * `sigma_lvlh` - 3-vector with 1-sigma position uncertainty in LVLH frame
    pub fn set_lvlh_pos_uncertainty(&mut self, sigma_lvlh: &na::Vector3<f64>) {
        let dcm = self.qgcrf2lvlh().to_rotation_matrix();

        let mut pcov = na::Matrix3::<f64>::zeros();
        pcov.set_diagonal(&sigma_lvlh.map(|x| x * x));

        let mut m = na::Matrix6::<f64>::zeros();
        m.fixed_view_mut::<3, 3>(0, 0)
            .copy_from(&(dcm.transpose() * pcov * dcm));
        self.cov = StateCov::PVCov(m);
    }

    /// Set position uncertainty (1-sigma, meters) in the
    /// gcrf (Geocentric Celestial Reference Frame)
    ///
    /// # Arguments
    ///
    /// * `sigma_gcrf` - 3-vector with 1-sigma position uncertainty in GCRF frame    
    ///
    pub fn set_gcrf_pos_uncertainty(&mut self, sigma_cart: &na::Vector3<f64>) {
        self.cov = StateCov::PVCov({
            let mut m = PVCovType::zeros();
            let mut diag = na::Vector6::<f64>::zeros();
            diag[0] = sigma_cart[0] * sigma_cart[0];
            diag[1] = sigma_cart[1] * sigma_cart[1];
            diag[2] = sigma_cart[2] * sigma_cart[2];
            m.set_diagonal(&diag);
            m
        })
    }

    ///
    /// Propagate state to a new time
    ///
    /// # Arguments:
    ///
    /// * `time` - Time for which to compute new state
    /// * `settings` - Settings for the propagator
    ///
    pub fn propagate(
        &self,
        time: &AstroTime,
        option_settings: Option<&PropSettings>,
    ) -> SKResult<SatState> {
        let default = orbitprop::PropSettings::default();
        let settings = option_settings.unwrap_or(&default);
        match self.cov {
            // Simple case: do not compute state transition matrix, since covariance is not set
            StateCov::None => {
                let res = orbitprop::propagate(&self.pv, &self.time, time, None, settings, None)?;
                Ok(SatState {
                    time: time.clone(),
                    pv: res.state[0],
                    cov: StateCov::None,
                })
            }
            // Compute state transition matrix & propagate covariance as well
            StateCov::PVCov(cov) => {
                let mut state = na::SMatrix::<f64, 6, 7>::zeros();

                // First row of state is 6-element position & velocity
                state.fixed_view_mut::<6, 1>(0, 0).copy_from(&self.pv);

                // See equation 7.42 of Montenbruck & Gill
                // State transition matrix initializes to identity matrix
                // State transition matrix is columns 1-7 of state (0-based)
                state
                    .fixed_view_mut::<6, 6>(0, 1)
                    .copy_from(&na::Matrix6::<f64>::identity());

                // Propagate
                let res = orbitprop::propagate(&state, &self.time, time, None, settings, None)?;

                Ok(SatState {
                    time: time.clone(),
                    pv: res.state[0].fixed_view::<6, 1>(0, 0).into(),
                    cov: {
                        // Extract state transition matrix from the propagated state
                        let phi = res.state[0].fixed_view::<6, 6>(0, 1);
                        // Evolve the covariance
                        StateCov::PVCov(phi * cov * phi.transpose())
                    },
                })
            }
        }
    }

    pub fn to_string(&self) -> String {
        let mut s1 = format!(
            r#"Satellite State
                Time: {}
            Position: [{:+8.0}, {:+8.0}, {:+8.0}] m,
            Velocity: [{:+8.3}, {:+8.3}, {:+8.3}] m/s"#,
            self.time, self.pv[0], self.pv[1], self.pv[2], self.pv[3], self.pv[4], self.pv[5],
        );
        match self.cov {
            StateCov::None => s1,
            StateCov::PVCov(cov) => {
                s1.push_str(
                    format!(
                        r#"
            Covariance: {cov:+8.2e}"#
                    )
                    .as_str(),
                );
                s1
            }
        }
    }
}

impl std::fmt::Display for SatState {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::consts;
    use approx::assert_relative_eq;

    #[test]
    fn test_qgcrf2lvlh() -> SKResult<()> {
        let satstate = SatState::from_pv(
            &AstroTime::from_datetime(2015, 3, 20, 0, 0, 0.0),
            &na::vector![consts::GEO_R, 0.0, 0.0],
            &na::vector![0.0, (consts::MU_EARTH / consts::GEO_R).sqrt(), 0.0],
        );

        let state2 = satstate.propagate(&(satstate.time + crate::Duration::Hours(3.56)), None)?;

        let rz = -1.0 / state2.pos().norm() * (state2.qgcrf2lvlh() * state2.pos());
        let h = state2.pos().cross(&state2.vel());
        let ry = -1.0 / h.norm() * (state2.qgcrf2lvlh() * h);
        let rx = 1.0 / state2.vel().norm() * (state2.qgcrf2lvlh() * state2.vel());

        assert_relative_eq!(rz, na::Vector3::z_axis(), epsilon = 1.0e-6);
        assert_relative_eq!(ry, na::Vector3::y_axis(), epsilon = 1.0e-6);
        assert_relative_eq!(rx, na::Vector3::x_axis(), epsilon = 1.0e-4);

        Ok(())
    }

    #[test]
    fn test_satstate() -> SKResult<()> {
        let satstate = SatState::from_pv(
            &AstroTime::from_datetime(2015, 3, 20, 0, 0, 0.0),
            &na::vector![consts::GEO_R, 0.0, 0.0],
            &na::vector![0.0, (consts::MU_EARTH / consts::GEO_R).sqrt(), 0.0],
        );
        println!("state orig = {:?}", satstate);

        let state2 = satstate.propagate(&(satstate.time + 1.0), None)?;

        println!("state 2 = {:?}", state2);

        let state0 = state2.propagate(&satstate.time, None);
        println!("state 0 = {:?}", state0);
        Ok(())
    }

    #[test]
    fn test_satcov() -> SKResult<()> {
        let mut satstate = SatState::from_pv(
            &AstroTime::from_datetime(2015, 3, 20, 0, 0, 0.0),
            &na::vector![consts::GEO_R, 0.0, 0.0],
            &na::vector![0.0, (consts::MU_EARTH / consts::GEO_R).sqrt(), 0.0],
        );
        satstate.set_lvlh_pos_uncertainty(&na::vector![1.0, 1.0, 1.0]);
        println!("state orig = {:?}", satstate.cov);

        let state2 = satstate.propagate(&(satstate.time + 1.0), None)?;

        println!("state 2 = {:?}", state2.cov);

        Ok(())
    }
}
