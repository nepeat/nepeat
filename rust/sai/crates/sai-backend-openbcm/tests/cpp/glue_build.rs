//! The C++ side of the glue differential (`tests/glue_differential.rs`) as a build of
//! sai-openbcm's `tests/frontend_driver.cpp` (sai-driver's `src/cpp_build.rs`, included as
//! `crate::cpp_build` by both users of this file: the test and `build.rs`, which prebuilds it for
//! a target without a compiler). No C++ file is changed: the driver's recorder wraps the REAL
//! backend (`-Dmake_fake_backend=glue_diff_backend`, defined by `test_port_glue.cpp`), whose
//! factory is renamed so that the driver's own `make_openbcm_backend` stays the library's
//! (`-Dmake_openbcm_backend=glue_diff_openbcm_backend`); the glue is `glue_stub.c`.
#![allow(
    dead_code,
    reason = "shared by build.rs and tests/glue_differential.rs; each uses a part"
)]

use std::path::Path;

use crate::cpp_build::{Prebuild, Variant};

/// The file name of the C++ glue driver.
pub(crate) const GLUE_DRIVER: &str = "sai-cpp-glue-driver";

/// The prebuild's names: its switch, its data directory under `target-tests`, its allow-list.
pub(crate) const PREBUILD: Prebuild<'static> = Prebuild {
    switch: "SAI_GLUE_DIFF_PREBUILD",
    data: "sai-backend-openbcm",
    allow: "glue-differential-allow.txt",
};

/// The build: `own` is this crate's `tests/cpp`, `src` sai-openbcm's `src`.
pub(crate) fn variant(own: &Path, src: &Path) -> Variant {
    Variant {
        exe: GLUE_DRIVER.to_owned(),
        driver_defines: vec!["-Dmake_fake_backend=glue_diff_backend".to_owned()],
        cpp: vec![
            (
                "openbcm_backend".to_owned(),
                src.join("openbcm_backend.cpp"),
                vec!["-Dmake_openbcm_backend=glue_diff_openbcm_backend".to_owned()],
            ),
            ("test_port_glue".to_owned(), own.join("test_port_glue.cpp"), Vec::new()),
        ],
        c: vec![("glue_stub".to_owned(), own.join("glue_stub.c"))],
        test_port_cpp: false,
    }
}
