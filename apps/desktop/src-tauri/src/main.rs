#[allow(dead_code)]
mod application {
    // Keep the established desktop implementation in one application module so
    // Phase 17 can extend the command surface without duplicating UI/workflow truth.
    include!("application.rs");
    include!("phase17.rs");
}

fn main() {
    application::run_phase17();
}
