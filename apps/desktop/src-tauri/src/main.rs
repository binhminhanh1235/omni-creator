#[allow(dead_code)]
mod application {
    // Keep the established desktop implementation in one application module so
    // later phases can extend the command surface without duplicating UI/workflow truth.
    include!("application.rs");
    include!("plugin_credentials.rs");
    include!("phase17.rs");
    include!("phase19_p5a.rs");
}

fn main() {
    application::run_phase19_p5a();
}
