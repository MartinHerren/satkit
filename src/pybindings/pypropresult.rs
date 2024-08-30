use pyo3::prelude::*;

use super::pyastrotime::PyAstroTime;
use super::pyutils::*;

use numpy::PyArrayMethods;
use numpy::{self as np, ToPyArray};

use crate::orbitprop::PropagationResult;

pub enum PyPropResultType {
    R1(PropagationResult<1>),
    R7(PropagationResult<7>),
}

#[pyclass(name = "propstats", module = "satkit")]
pub struct PyPropStats {
    #[pyo3(get)]
    num_eval: u32,
    #[pyo3(get)]
    num_accept: u32,
    #[pyo3(get)]
    num_reject: u32,
}

#[pymethods]
impl PyPropStats {
    fn __str__(&self) -> String {
        format!("Propagation Statistics:\n  Function Evals: {}\n  Accepted Steps: {}\n  Rejected Steps: {}",
    self.num_eval, self.num_accept, self.num_reject)
    }
}

#[pyclass(name = "propresult", module = "satkit")]
pub struct PyPropResult {
    pub inner: PyPropResultType,
}

fn to_string<const T: usize>(r: &PropagationResult<T>) -> String {
    let mut s = format!("Propagation Results\n");
    s.push_str(format!("  Time: {}\n", r.time_end).as_str());
    s.push_str(
        format!(
            "   Pos: [{:.3}, {:.3}, {:.3}] km\n",
            r.state_end[0] * 1.0e-3,
            r.state_end[1] * 1.0e-3,
            r.state_end[2] * 1.0e-3
        )
        .as_str(),
    );
    s.push_str(
        format!(
            "   Vel: [{:.3}, {:.3}, {:.3}] m/s\n",
            r.state_end[3], r.state_end[4], r.state_end[5]
        )
        .as_str(),
    );
    s.push_str("  Stats:\n");
    s.push_str(format!("       Function Evaluations: {}\n", r.num_eval).as_str());
    s.push_str(format!("             Accepted Steps: {}\n", r.accepted_steps).as_str());
    s.push_str(format!("             Rejected Steps: {}\n", r.rejected_steps).as_str());
    s.push_str(format!("   Can Interp: {}\n", r.odesol.is_some()).as_str());
    if r.odesol.is_some() {
        s.push_str(format!("        Start Time: {}", r.time_start).as_str());
    }
    s
}

#[pymethods]
impl PyPropResult {
    // Get start time
    #[getter]
    fn time_start(&self) -> PyAstroTime {
        PyAstroTime {
            inner: match &self.inner {
                PyPropResultType::R1(r) => r.time_start,
                PyPropResultType::R7(r) => r.time_start,
            },
        }
    }

    /// Get the stop time
    #[getter]
    fn time(&self) -> PyAstroTime {
        PyAstroTime {
            inner: match &self.inner {
                PyPropResultType::R1(r) => r.time_end,
                PyPropResultType::R7(r) => r.time_end,
            },
        }
    }

    #[getter]
    fn stats(&self) -> PyPropStats {
        match &self.inner {
            PyPropResultType::R1(r) => PyPropStats {
                num_eval: r.num_eval,
                num_accept: r.accepted_steps,
                num_reject: r.rejected_steps,
            },
            PyPropResultType::R7(r) => PyPropStats {
                num_eval: r.num_eval,
                num_accept: r.accepted_steps,
                num_reject: r.rejected_steps,
            },
        }
    }

    #[getter]
    fn pos(&self) -> PyObject {
        pyo3::Python::with_gil(|py| -> PyObject {
            match &self.inner {
                PyPropResultType::R1(r) => np::ndarray::arr1(&r.state_end.as_slice()[0..3])
                    .to_pyarray_bound(py)
                    .to_object(py),
                PyPropResultType::R7(r) => np::ndarray::arr1(&r.state_end.as_slice()[0..3])
                    .to_pyarray_bound(py)
                    .to_object(py),
            }
        })
    }

    #[getter]
    fn vel(&self) -> PyObject {
        pyo3::Python::with_gil(|py| -> PyObject {
            match &self.inner {
                PyPropResultType::R1(r) => np::ndarray::arr1(&r.state_end.as_slice()[3..6])
                    .to_pyarray_bound(py)
                    .to_object(py),
                PyPropResultType::R7(r) => {
                    np::ndarray::arr1(&r.state_end.column(0).as_slice()[3..6])
                        .to_pyarray_bound(py)
                        .to_object(py)
                }
            }
        })
    }

    #[getter]
    fn state(&self) -> PyObject {
        pyo3::Python::with_gil(|py| -> PyObject {
            match &self.inner {
                PyPropResultType::R1(r) => np::ndarray::arr1(r.state_end.as_slice())
                    .to_pyarray_bound(py)
                    .to_object(py),
                PyPropResultType::R7(r) => np::ndarray::arr1(&r.state_end.as_slice()[0..6])
                    .to_pyarray_bound(py)
                    .to_object(py),
            }
        })
    }

    #[getter]
    fn phi(&self) -> PyObject {
        pyo3::Python::with_gil(|py| -> PyObject {
            match &self.inner {
                PyPropResultType::R1(_r) => py.None(),
                PyPropResultType::R7(r) => {
                    let phi = unsafe { np::PyArray2::<f64>::new_bound(py, [6, 6], false) };
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            r.state_end.as_ptr().offset(6),
                            phi.as_raw_array_mut().as_mut_ptr(),
                            36,
                        );
                    }
                    phi.to_object(py)
                }
            }
        })
    }

    fn __str__(&self) -> String {
        match &self.inner {
            PyPropResultType::R1(r) => to_string::<1>(r),
            PyPropResultType::R7(r) => to_string::<7>(r),
        }
    }

    #[getter]
    fn can_interp(&self) -> bool {
        match &self.inner {
            PyPropResultType::R1(r) => r.odesol.is_some(),
            PyPropResultType::R7(r) => r.odesol.is_some(),
        }
    }

    #[pyo3(signature=(time, output_phi=false))]
    fn interp(&self, time: PyAstroTime, output_phi: bool) -> PyResult<PyObject> {
        match &self.inner {
            PyPropResultType::R1(r) => match r.interp(&time.inner) {
                Ok(res) => {
                    pyo3::Python::with_gil(|py| -> PyResult<PyObject> { Ok(vec2py(py, &res)) })
                }
                Err(e) => Err(pyo3::exceptions::PyValueError::new_err(e.to_string())),
            },
            PyPropResultType::R7(r) => match r.interp(&time.inner) {
                Ok(res) => {
                    if output_phi == false {
                        pyo3::Python::with_gil(|py| -> PyResult<PyObject> {
                            Ok(slice2py1d(py, &res.as_slice()[0..6]))
                        })
                    } else {
                        pyo3::Python::with_gil(|py| -> PyResult<PyObject> {
                            Ok((
                                slice2py1d(py, &res.as_slice()[0..6]),
                                slice2py2d(py, &res.as_slice()[6..42], 6, 6)?,
                            )
                                .to_object(py))
                        })
                    }
                }
                Err(e) => Err(pyo3::exceptions::PyValueError::new_err(e.to_string())),
            },
        }
    }
}