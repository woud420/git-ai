use super::codec as admission_codec;
use super::*;

mod budgets;
mod codec_cases;
mod faults;
mod plans;
mod reads;
mod support;
mod transactions;
mod wire;
use support::*;
use wire::*;

#[allow(dead_code)]
mod vectors {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/integration/fixtures/jj-admission/vectors.rs"
    ));
}
#[allow(dead_code)]
mod boundary {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/integration/fixtures/jj-admission/boundary.rs"
    ));
}
#[allow(dead_code)]
mod native_wire {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/integration/jj_operation_support.rs"
    ));
}
