//! Read-only arrays exported to Python through the buffer protocol.
//!
//! # Choice of bridge
//!
//! The `numpy` crate needs the full C API of `NumPy` and `CPython` and is not usable with
//! the stable ABI. The buffer protocol (`bf_getbuffer`, PEP 3118) is part of the limited
//! API since Python 3.11, so an `abi3-py311` wheel can export it, and `numpy.asarray`
//! turns such a buffer into an array without copying: the array's `base` is a
//! `memoryview` whose `obj` is the [`Buffer`], which holds an `Arc` to the tree data.
//! The data stays alive as long as any array or memoryview of it, whatever happens to the
//! `Tree` object. The buffer is read-only, so `NumPy` marks the arrays non-writable;
//! `numpy.array(a)` or `a.copy()` gives a writable copy.
//!
//! The same objects work without `NumPy`: `memoryview(buffer)` and Blender's
//! `foreach_set` accept them directly.

use std::ffi::c_int;
use std::sync::Arc;

use pyo3::exceptions::PyBufferError;
use pyo3::ffi;
use pyo3::prelude::*;

use crate::data::{Field, TreeData};

/// Read-only buffer over one array of a tree.
///
/// Implements the buffer protocol: C-contiguous, shape `(rows,)` or `(rows, columns)`,
/// native byte order. Get one from the `buffer` method of `MeshPart`, `Skeleton` or
/// `Organs`; the array properties of those classes are `numpy.asarray` of it.
#[pyclass(frozen, module = "arbrume._arbrume")]
#[derive(Debug)]
pub struct Buffer {
    owner: Arc<TreeData>,
    field: Field,
    ndim: c_int,
    shape: [ffi::Py_ssize_t; 2],
    strides: [ffi::Py_ssize_t; 2],
}

impl Buffer {
    /// A buffer over `field` of `owner`.
    pub fn new(owner: Arc<TreeData>, field: Field) -> Self {
        let view = owner.view(field);
        let item = view.element.size().cast_signed();
        let rows = view.rows().cast_signed();
        let columns = view.columns.cast_signed();
        let (ndim, shape, strides) = if view.columns == 1 {
            (1, [rows, 0], [item, 0])
        } else {
            (2, [rows, columns], [item * columns, item])
        };
        Self {
            owner,
            field,
            ndim,
            shape,
            strides,
        }
    }

    /// Whether the layout is also Fortran-contiguous (at most one non-trivial axis).
    const fn is_fortran_contiguous(&self) -> bool {
        self.ndim == 1 || self.shape[0] <= 1 || self.shape[1] <= 1
    }
}

/// `true` when every bit of `request` is set in `flags`.
const fn wants(flags: c_int, request: c_int) -> bool {
    flags & request == request
}

#[pymethods]
impl Buffer {
    /// Fills `view` for a consumer; refuses writable and Fortran-order requests that
    /// the layout cannot satisfy.
    ///
    /// # Safety
    ///
    /// `view` is the `Py_buffer` passed by the interpreter to `bf_getbuffer`. Every
    /// pointer stored in it (data, format, shape, strides) stays valid as long as
    /// `view.obj`, a new reference to this buffer, is alive: the data and the shape
    /// arrays live in the buffer object or in the `Arc` it holds, and are never mutated.
    unsafe fn __getbuffer__(
        slf: &Bound<'_, Self>,
        view: *mut ffi::Py_buffer,
        flags: c_int,
    ) -> PyResult<()> {
        if view.is_null() {
            return Err(PyBufferError::new_err("null Py_buffer"));
        }
        let this = slf.get();
        if wants(flags, ffi::PyBUF_WRITABLE) {
            return Err(PyBufferError::new_err("arbrume arrays are read-only"));
        }
        if wants(flags, ffi::PyBUF_F_CONTIGUOUS) && !this.is_fortran_contiguous() {
            return Err(PyBufferError::new_err("arbrume arrays are C-contiguous"));
        }
        let data = this.owner.view(this.field);
        let format = if wants(flags, ffi::PyBUF_FORMAT) {
            data.element.format().as_ptr().cast_mut()
        } else {
            std::ptr::null_mut()
        };
        let shape = if wants(flags, ffi::PyBUF_ND) {
            this.shape.as_ptr().cast_mut()
        } else {
            std::ptr::null_mut()
        };
        let strides = if wants(flags, ffi::PyBUF_STRIDES) {
            this.strides.as_ptr().cast_mut()
        } else {
            std::ptr::null_mut()
        };
        // SAFETY: `view` is non-null and points to a `Py_buffer` owned by the caller;
        // see the safety section above for the lifetime of the stored pointers.
        unsafe {
            (*view).buf = data.bytes.as_ptr().cast_mut().cast();
            (*view).len = data.bytes.len().cast_signed();
            (*view).itemsize = data.element.size().cast_signed();
            (*view).readonly = 1;
            (*view).ndim = this.ndim;
            (*view).format = format;
            (*view).shape = shape;
            (*view).strides = strides;
            (*view).suboffsets = std::ptr::null_mut();
            (*view).internal = std::ptr::null_mut();
            (*view).obj = slf.clone().into_any().into_ptr();
        }
        Ok(())
    }

    /// Shape of the array: `(rows,)` or `(rows, columns)`.
    #[getter]
    fn shape(&self) -> Vec<isize> {
        self.shape[..self.ndim as usize].to_vec()
    }

    /// Struct-module format character of the elements.
    #[getter]
    fn format(&self) -> String {
        let view = self.owner.view(self.field);
        view.element.format().to_string_lossy().into_owned()
    }

    fn __len__(&self) -> usize {
        self.shape[0] as usize
    }

    fn __repr__(&self) -> String {
        format!("<arbrume.Buffer {:?} shape={:?}>", self.field, self.shape())
    }
}
