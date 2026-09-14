include!("mcp.rs");

const MCP_TASK_POLL_INTERVAL_MS_V1: u64 = 500;
const MCP_TASK_RESULT_SCHEMA_V1: &str = "omnicreator.mcp-task-result";
const MCP_TASK_RESULT_VERSION_V1: u32 = 1;

#[derive(Clone)]
struct OmniCreatorMcpTaskServerV1 {
    inner: OmniCreatorMcpServerV1,
}

impl OmniCreatorMcpTaskServerV1 {
    fn new(config: McpServerConfigV1) -> Self {
        Self {
            inner: OmniCreatorMcpServerV1::new(config),
        }
    }

    fn creator_request_v1(params: CreatorStartParamsV1) -> StartOrResumeCreatorRequestV1 {
        let input = params.input.map(|input| match input.kind {
            CreatorInputKindV1::Topic => CreatorInputV1::topic(input.text),
            CreatorInputKindV1::Script => CreatorInputV1::script(input.text),
        });
        StartOrResumeCreatorRequestV1 {
            project_id: params.project_id,
            input,
        }
    }

    fn creator_task_input_hash_v1(params: &CreatorStartParamsV1) -> String {
        let (kind, text) = match params.input.as_ref() {
            Some(input) => {
                let kind = match input.kind {
                    CreatorInputKindV1::Topic => b"topic".as_slice(),
                    CreatorInputKindV1::Script => b"script".as_slice(),
                };
                (kind, input.text.as_bytes())
            }
            None => (b"resume".as_slice(), b"".as_slice()),
        };
        omnicreator_core::deterministic_input_hash(&[
            b"mcp-task:creator-start-or-resume:v1",
            params.project_id.as_bytes(),
            kind,
            text,
        ])
    }

    fn protocol_error_v1(&self, error: impl Into<ControlErrorV1>) -> McpError {
        let error = sanitize_error_v1(error.into(), &self.inner.config.data_root);
        McpError::invalid_params(
            error.message.clone(),
            Some(json!({
                "schema": MCP_ERROR_ENVELOPE_SCHEMA_V1,
                "version": MCP_ERROR_ENVELOPE_VERSION_V1,
                "error": error
            })),
        )
    }

    fn create_creator_task_v1(
        &self,
        params: &CreatorStartParamsV1,
    ) -> Result<omnicreator_core::ControlTaskSnapshotV1, McpError> {
        let workspace = Workspace::open(&self.inner.config.data_root)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        let session = WorkspaceSession::acquire(workspace, &self.inner.config.device_id)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        let mut store = StateStore::open(session.sqlite_path())
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        store
            .get_project(&params.project_id)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        let input_hash = Self::creator_task_input_hash_v1(params);
        store
            .create_control_task_v1(
                &params.project_id,
                omnicreator_core::CONTROL_TASK_CREATOR_START_OR_RESUME_V1,
                &input_hash,
                Some("mcp:creator_start_or_resume"),
            )
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))
    }

    fn task_from_snapshot_v1(
        snapshot: &omnicreator_core::ControlTaskSnapshotV1,
    ) -> Result<rmcp::model::Task, McpError> {
        let first = snapshot.attempts.first().ok_or_else(|| {
            McpError::internal_error("canonical control task has no attempt", None)
        })?;
        let last = snapshot.attempts.last().unwrap_or(first);
        let updated_at = snapshot
            .selected_artifact
            .as_ref()
            .map(|artifact| artifact.created_at)
            .or(last.finished_at)
            .unwrap_or(last.started_at);
        let status = match snapshot.job.status {
            omnicreator_core::StepStatus::Succeeded => rmcp::model::TaskStatus::Completed,
            omnicreator_core::StepStatus::Cancelled => rmcp::model::TaskStatus::Cancelled,
            omnicreator_core::StepStatus::Failed
            | omnicreator_core::StepStatus::Retryable
            | omnicreator_core::StepStatus::Fatal
            | omnicreator_core::StepStatus::Stale
            | omnicreator_core::StepStatus::Skipped
            | omnicreator_core::StepStatus::NotReady => rmcp::model::TaskStatus::Failed,
            omnicreator_core::StepStatus::Ready
            | omnicreator_core::StepStatus::Queued
            | omnicreator_core::StepStatus::Running => rmcp::model::TaskStatus::Working,
        };
        Ok(rmcp::model::Task::new(
            snapshot.job.job_id.clone(),
            status,
            first.started_at.to_rfc3339(),
            updated_at.to_rfc3339(),
        )
        .with_status_message(format!(
            "canonical OmniCreator job {}",
            snapshot.job.status.as_str()
        ))
        .with_poll_interval_ms(MCP_TASK_POLL_INTERVAL_MS_V1))
    }

    fn load_task_snapshot_v1(
        &self,
        task_id: &str,
    ) -> Result<omnicreator_core::ControlTaskSnapshotV1, McpError> {
        let workspace = Workspace::inspect(&self.inner.config.data_root)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        let store = StateStore::open_read_only(workspace.sqlite_path())
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        store
            .get_control_task_v1(task_id)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))
    }

    fn completed_task_payload_v1(
        &self,
        snapshot: &omnicreator_core::ControlTaskSnapshotV1,
    ) -> Result<rmcp::model::TaskPayload, McpError> {
        let artifact = snapshot.selected_artifact.as_ref().ok_or_else(|| {
            McpError::internal_error("completed control task has no selected result artifact", None)
        })?;
        let artifact_store = ArtifactStore::new(&self.inner.config.data_root)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        if !artifact_store
            .verify_artifact(artifact)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?
        {
            return Err(McpError::internal_error(
                "completed control task result artifact is missing",
                None,
            ));
        }
        let path = artifact_store
            .resolve_artifact_path(artifact)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        let raw = fs::read(path)
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        let value: Value = serde_json::from_slice(&raw)
            .map_err(|error| McpError::internal_error(error.to_string(), None))?;
        let Value::Object(result) = value else {
            return Err(McpError::internal_error(
                "completed control task result artifact is not a JSON object",
                None,
            ));
        };
        Ok(rmcp::model::TaskPayload::Completed { result })
    }

    fn detailed_task_v1(
        &self,
        snapshot: omnicreator_core::ControlTaskSnapshotV1,
    ) -> Result<rmcp::model::DetailedTask, McpError> {
        let task = Self::task_from_snapshot_v1(&snapshot)?;
        let payload = match snapshot.job.status {
            omnicreator_core::StepStatus::Succeeded => self.completed_task_payload_v1(&snapshot)?,
            omnicreator_core::StepStatus::Cancelled => rmcp::model::TaskPayload::Cancelled,
            omnicreator_core::StepStatus::Failed
            | omnicreator_core::StepStatus::Retryable
            | omnicreator_core::StepStatus::Fatal
            | omnicreator_core::StepStatus::Stale
            | omnicreator_core::StepStatus::Skipped
            | omnicreator_core::StepStatus::NotReady => {
                let mut error = rmcp::model::JsonObject::new();
                error.insert(
                    "code".to_owned(),
                    Value::String("canonical_task_interrupted".to_owned()),
                );
                error.insert(
                    "message".to_owned(),
                    Value::String(format!(
                        "canonical OmniCreator task ended in {}",
                        snapshot.job.status.as_str()
                    )),
                );
                rmcp::model::TaskPayload::Failed { error }
            }
            omnicreator_core::StepStatus::Ready
            | omnicreator_core::StepStatus::Queued
            | omnicreator_core::StepStatus::Running => rmcp::model::TaskPayload::Working,
        };
        Ok(rmcp::model::DetailedTask::new(task, payload))
    }

    fn persist_task_result_v1(
        config: &McpServerConfigV1,
        task_id: &str,
        result: &CallToolResult,
    ) -> Result<(), String> {
        let workspace = Workspace::open(&config.data_root).map_err(|error| error.to_string())?;
        let session = WorkspaceSession::acquire(workspace, &config.device_id)
            .map_err(|error| error.to_string())?;
        let mut store = StateStore::open(session.sqlite_path()).map_err(|error| error.to_string())?;
        let snapshot = store
            .get_control_task_v1(task_id)
            .map_err(|error| error.to_string())?;
        if snapshot.job.status == omnicreator_core::StepStatus::Cancelled {
            return Ok(());
        }
        if snapshot.job.status == omnicreator_core::StepStatus::Succeeded {
            return Ok(());
        }
        if snapshot.job.status != omnicreator_core::StepStatus::Running {
            return Err(format!(
                "control task {task_id} cannot persist result from {}",
                snapshot.job.status.as_str()
            ));
        }
        let attempt_id = snapshot.job.selected_attempt.clone().ok_or_else(|| {
            format!("control task {task_id} has no selected running attempt")
        })?;
        let staging_dir = config.data_root.join(".omnicreator/task-staging");
        fs::create_dir_all(&staging_dir).map_err(|error| error.to_string())?;
        let source = staging_dir.join(format!("{task_id}.json"));
        let raw = serde_json::to_vec_pretty(result).map_err(|error| error.to_string())?;
        fs::write(&source, raw).map_err(|error| error.to_string())?;

        let artifact_store = ArtifactStore::new(&config.data_root).map_err(|error| error.to_string())?;
        let promotion = artifact_store.promote_attempt_outputs(
            &mut store,
            omnicreator_core::AttemptPromotionRequest {
                attempt_id,
                job_id: task_id.to_owned(),
                outputs: vec![omnicreator_core::AttemptOutputPromotion {
                    source: source.clone(),
                    target_uri: omnicreator_core::LogicalUri::Project(format!(
                        "control/tasks/{task_id}.json"
                    )),
                    artifact_type: omnicreator_core::CONTROL_TASK_RESULT_ARTIFACT_TYPE_V1.to_owned(),
                    metadata: json!({
                        "schema": MCP_TASK_RESULT_SCHEMA_V1,
                        "version": MCP_TASK_RESULT_VERSION_V1,
                        "origin": "mcp",
                        "operation": "creator_start_or_resume"
                    }),
                    expected_sha256: None,
                }],
                selected_output_index: 0,
            },
        );
        let _ = fs::remove_file(&source);
        promotion.map(|_| ()).map_err(|error| error.to_string())
    }

    fn mark_task_internal_failure_v1(config: &McpServerConfigV1, task_id: &str, code: &str) {
        let Ok(workspace) = Workspace::open(&config.data_root) else {
            return;
        };
        let Ok(session) = WorkspaceSession::acquire(workspace, &config.device_id) else {
            return;
        };
        let Ok(mut store) = StateStore::open(session.sqlite_path()) else {
            return;
        };
        let Ok(snapshot) = store.get_control_task_v1(task_id) else {
            return;
        };
        if snapshot.job.status != omnicreator_core::StepStatus::Running {
            return;
        }
        if let Some(attempt_id) = snapshot.job.selected_attempt.as_deref() {
            let _ = store.finish_attempt_failure(attempt_id, code);
        }
    }

    fn spawn_creator_task_v1(&self, task_id: String, params: CreatorStartParamsV1) {
        let config = self.inner.config.clone();
        tokio::spawn(async move {
            let runner = OmniCreatorMcpServerV1::new(config.clone());
            let request = OmniCreatorMcpTaskServerV1::creator_request_v1(params);
            let call_result = match runner.render_v1(runner.start_or_resume_v1(request)) {
                Ok(result) => result,
                Err(_) => {
                    OmniCreatorMcpTaskServerV1::mark_task_internal_failure_v1(
                        &config,
                        &task_id,
                        "MCP_TASK_INTERNAL_SERIALIZATION",
                    );
                    return;
                }
            };
            if OmniCreatorMcpTaskServerV1::persist_task_result_v1(
                &config,
                &task_id,
                &call_result,
            )
            .is_err()
            {
                OmniCreatorMcpTaskServerV1::mark_task_internal_failure_v1(
                    &config,
                    &task_id,
                    "MCP_TASK_RESULT_PERSISTENCE",
                );
            }
        });
    }

    fn reconcile_interrupted_control_tasks_v1(config: &McpServerConfigV1) -> Result<(), String> {
        if config.read_only {
            return Ok(());
        }
        let workspace = Workspace::open(&config.data_root).map_err(|error| error.to_string())?;
        let session = WorkspaceSession::acquire(workspace, &config.device_id)
            .map_err(|error| error.to_string())?;
        let mut store = StateStore::open(session.sqlite_path()).map_err(|error| error.to_string())?;
        store
            .reconcile_interrupted_jobs()
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

impl rmcp::ServerHandler for OmniCreatorMcpTaskServerV1 {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        let client_supports_tasks = context
            .client_capabilities()
            .is_some_and(|caps| caps.supports_tasks());

        if request.name == "creator_start_or_resume"
            && client_supports_tasks
            && !self.inner.config.read_only
        {
            let params: CreatorStartParamsV1 = serde_json::from_value(Value::Object(
                request.arguments.clone().unwrap_or_default(),
            ))
            .map_err(|error| McpError::invalid_params(error.to_string(), None))?;
            let snapshot = self.create_creator_task_v1(&params)?;
            let task = Self::task_from_snapshot_v1(&snapshot)?;
            let task_id = snapshot.job.job_id.clone();
            self.spawn_creator_task_v1(task_id, params);
            return Ok(rmcp::model::CallToolResponse::Task(
                rmcp::model::CreateTaskResult::new(task),
            ));
        }

        <OmniCreatorMcpServerV1 as rmcp::ServerHandler>::call_tool(
            &self.inner,
            request,
            context,
        )
        .await
    }

    async fn list_tools(
        &self,
        request: Option<rmcp::model::PaginatedRequestParams>,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, McpError> {
        <OmniCreatorMcpServerV1 as rmcp::ServerHandler>::list_tools(
            &self.inner,
            request,
            context,
        )
        .await
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        <OmniCreatorMcpServerV1 as rmcp::ServerHandler>::get_tool(&self.inner, name)
    }

    async fn get_task(
        &self,
        request: rmcp::model::GetTaskParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::GetTaskResult, McpError> {
        let snapshot = self.load_task_snapshot_v1(&request.task_id)?;
        Ok(rmcp::model::GetTaskResult::new(
            self.detailed_task_v1(snapshot)?,
        ))
    }

    async fn update_task(
        &self,
        _request: rmcp::model::UpdateTaskParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<(), McpError> {
        Err(McpError::invalid_params(
            "OmniCreator control tasks do not expose input_required requests; use canonical manual/external tools and creator resume instead",
            None,
        ))
    }

    async fn cancel_task(
        &self,
        request: rmcp::model::CancelTaskParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<(), McpError> {
        if self.inner.config.read_only {
            return Err(McpError::invalid_request(
                "read-only MCP session cannot cancel tasks",
                None,
            ));
        }
        let workspace = Workspace::open(&self.inner.config.data_root)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        let session = WorkspaceSession::acquire(workspace, &self.inner.config.device_id)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        let mut store = StateStore::open(session.sqlite_path())
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        store
            .cancel_control_task_v1(&request.task_id)
            .map_err(|error| self.protocol_error_v1(ControlErrorV1::from(error)))?;
        Ok(())
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        let mut info = <OmniCreatorMcpServerV1 as rmcp::ServerHandler>::get_info(&self.inner);
        info.capabilities = rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .enable_tasks()
            .build();
        info
    }
}

pub async fn serve_mcp_stdio_p5_from_args_v1(args: Vec<String>) -> Result<(), String> {
    let config = parse_mcp_config_v1(args).map_err(|error| error.to_string())?;
    OmniCreatorMcpTaskServerV1::reconcile_interrupted_control_tasks_v1(&config)?;
    let service = OmniCreatorMcpTaskServerV1::new(config)
        .serve(stdio())
        .await
        .map_err(|error| format!("unable to start MCP stdio service: {error}"))?;
    service
        .waiting()
        .await
        .map_err(|error| format!("MCP stdio service stopped with an error: {error}"))?;
    Ok(())
}
