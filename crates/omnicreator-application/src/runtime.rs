use crate::{
    ComputeRuntimeControlSnapshotV1, ControlResultV1, PluginRuntimeControlSnapshotV1,
};

/// Small transport-neutral bridge for machine-local runtime inspection.
///
/// The application layer intentionally does not own PluginRegistry or ComputeProvider runtimes.
/// Desktop/CLI/MCP adapters may implement this trait from their existing machine-local runtime
/// context and return only sanitized capability/readiness data.
pub trait ApplicationRuntimeInspectorV1 {
    fn plugin_runtime_snapshot_v1(&self) -> ControlResultV1<PluginRuntimeControlSnapshotV1>;
    fn compute_runtime_snapshot_v1(&self) -> ControlResultV1<ComputeRuntimeControlSnapshotV1>;
}
