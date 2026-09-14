use crate::{
    ApplicationControlService, ApplicationRuntimeInspectorV1, ControlOperationV1,
    ControlResponseV1, ControlResultV1, LlmRuntimeControlSnapshotV1,
};

impl ApplicationControlService<'_> {
    pub fn llm_runtime_inspection_v1(
        &self,
        inspector: &impl ApplicationRuntimeInspectorV1,
    ) -> ControlResultV1<ControlResponseV1<LlmRuntimeControlSnapshotV1>> {
        Ok(ControlResponseV1::new(
            ControlOperationV1::LlmRuntimeInspection,
            inspector.llm_runtime_snapshot_v1()?,
        ))
    }
}
