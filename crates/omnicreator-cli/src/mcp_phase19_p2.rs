include!("mcp_p5.rs");

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AgentWorkPrepareParamsV1 {
    project_id: String,
    work_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AgentWorkCommitParamsV1 {
    payload: Value,
}

#[derive(Clone)]
struct OmniCreatorPhase19P2ToolsV1 {
    inner: OmniCreatorMcpServerV1,
}

impl OmniCreatorPhase19P2ToolsV1 {
    fn new(config: McpServerConfigV1) -> Self {
        Self {
            inner: OmniCreatorMcpServerV1::new(config),
        }
    }

    fn with_read_only_service_v1(
        &self,
        operation: impl for<'service> FnOnce(
            &mut ApplicationControlService<'service>,
        ) -> ControlResultV1<Value>,
    ) -> ControlResultV1<Value> {
        let workspace =
            Workspace::inspect(&self.inner.config.data_root).map_err(ControlErrorV1::from)?;
        let mut service = ApplicationControlService::for_read_only(&workspace)?;
        operation(&mut service)
    }
}

#[tool_router(server_handler)]
impl OmniCreatorPhase19P2ToolsV1 {
    #[tool(
        description = "Inspect the canonical Phase 19 agent work graph without acquiring the OmniCreator writer lease. Returns deterministic readiness, dependencies, selected artifacts, and external visual/voice descriptors."
    )]
    async fn agent_work_graph(
        &self,
        Parameters(params): Parameters<ProjectIdParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.inner
            .render_v1(self.with_read_only_service_v1(|service| {
                to_value_v1(service.agent_work_graph_v1(&ProjectIdRequestV1 {
                    project_id: params.project_id,
                })?)
            }))
    }

    #[tool(
        description = "Inspect canonical Phase 19 visual/voice fan-in QA and recovery state without acquiring the writer lease. Combines work readiness with verified/missing/invalid/unselected artifact health."
    )]
    async fn agent_fan_in_qa(
        &self,
        Parameters(params): Parameters<ProjectIdParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.inner
            .render_v1(self.with_read_only_service_v1(|service| {
                to_value_v1(service.agent_fan_in_qa_v1(&ProjectIdRequestV1 {
                    project_id: params.project_id,
                })?)
            }))
    }

    #[tool(
        description = "Prepare one externally executable Phase 19 work item by canonical work_id. This is read-only and never claims or mutates the work item."
    )]
    async fn agent_work_prepare(
        &self,
        Parameters(params): Parameters<AgentWorkPrepareParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        self.inner
            .render_v1(self.with_read_only_service_v1(|service| {
                let graph = service.agent_work_graph_v1(&ProjectIdRequestV1 {
                    project_id: params.project_id,
                })?;
                let item = graph
                    .items
                    .into_iter()
                    .find(|item| item.work_id == params.work_id)
                    .ok_or_else(|| {
                        ControlErrorV1::new(
                            ControlErrorCodeV1::NotFound,
                            format!("agent work item was not found: {}", params.work_id),
                        )
                    })?;
                if item.external.is_none() {
                    return Err(ControlErrorV1::new(
                        ControlErrorCodeV1::Blocked,
                        format!(
                            "agent work item {} has no external work descriptor in current canonical state",
                            params.work_id
                        ),
                    ));
                }
                to_value_v1(item)
            }))
    }

    #[tool(
        description = "Commit a bounded batch of completed external visual/voice results through the single canonical OmniCreator writer. Per-item stale, duplicate/reconnect, replacement, and failure semantics are preserved."
    )]
    async fn agent_work_commit(
        &self,
        Parameters(params): Parameters<AgentWorkCommitParamsV1>,
    ) -> Result<CallToolResult, McpError> {
        let request: omnicreator_application::ExternalResultBatchRequestV1 =
            match parse_payload_v1(params.payload) {
                Ok(request) => request,
                Err(error) => return self.inner.render_v1::<Value>(Err(error)),
            };
        self.inner
            .render_v1(self.inner.with_service_v1(|service| {
                to_value_v1(service.provide_external_result_batch_v1(&request)?)
            }))
    }
}

#[derive(Clone)]
struct OmniCreatorMcpPhase19P2ServerV1 {
    legacy: OmniCreatorMcpTaskServerV1,
    phase19: OmniCreatorPhase19P2ToolsV1,
}

impl OmniCreatorMcpPhase19P2ServerV1 {
    fn new(config: McpServerConfigV1) -> Self {
        Self {
            legacy: OmniCreatorMcpTaskServerV1::new(config.clone()),
            phase19: OmniCreatorPhase19P2ToolsV1::new(config),
        }
    }

    fn is_phase19_tool_v1(name: &str) -> bool {
        matches!(
            name,
            "agent_work_graph"
                | "agent_fan_in_qa"
                | "agent_work_prepare"
                | "agent_work_commit"
        )
    }
}

impl rmcp::ServerHandler for OmniCreatorMcpPhase19P2ServerV1 {
    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParams,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, McpError> {
        if Self::is_phase19_tool_v1(request.name.as_ref()) {
            <OmniCreatorPhase19P2ToolsV1 as rmcp::ServerHandler>::call_tool(
                &self.phase19,
                request,
                context,
            )
            .await
        } else {
            <OmniCreatorMcpTaskServerV1 as rmcp::ServerHandler>::call_tool(
                &self.legacy,
                request,
                context,
            )
            .await
        }
    }

    async fn list_tools(
        &self,
        request: Option<rmcp::model::PaginatedRequestParams>,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, McpError> {
        let mut result = <OmniCreatorMcpTaskServerV1 as rmcp::ServerHandler>::list_tools(
            &self.legacy,
            request,
            context,
        )
        .await?;
        for name in [
            "agent_work_graph",
            "agent_fan_in_qa",
            "agent_work_prepare",
            "agent_work_commit",
        ] {
            if let Some(tool) =
                <OmniCreatorPhase19P2ToolsV1 as rmcp::ServerHandler>::get_tool(&self.phase19, name)
            {
                result.tools.push(tool);
            }
        }
        result
            .tools
            .sort_by(|left, right| left.name.cmp(&right.name));
        Ok(result)
    }

    fn get_tool(&self, name: &str) -> Option<rmcp::model::Tool> {
        if Self::is_phase19_tool_v1(name) {
            <OmniCreatorPhase19P2ToolsV1 as rmcp::ServerHandler>::get_tool(&self.phase19, name)
        } else {
            <OmniCreatorMcpTaskServerV1 as rmcp::ServerHandler>::get_tool(&self.legacy, name)
        }
    }

    async fn get_task(
        &self,
        request: rmcp::model::GetTaskParams,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<rmcp::model::GetTaskResult, McpError> {
        <OmniCreatorMcpTaskServerV1 as rmcp::ServerHandler>::get_task(
            &self.legacy,
            request,
            context,
        )
        .await
    }

    async fn update_task(
        &self,
        request: rmcp::model::UpdateTaskParams,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<(), McpError> {
        <OmniCreatorMcpTaskServerV1 as rmcp::ServerHandler>::update_task(
            &self.legacy,
            request,
            context,
        )
        .await
    }

    async fn cancel_task(
        &self,
        request: rmcp::model::CancelTaskParams,
        context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<(), McpError> {
        <OmniCreatorMcpTaskServerV1 as rmcp::ServerHandler>::cancel_task(
            &self.legacy,
            request,
            context,
        )
        .await
    }

    fn get_info(&self) -> rmcp::model::ServerInfo {
        <OmniCreatorMcpTaskServerV1 as rmcp::ServerHandler>::get_info(&self.legacy)
    }
}

pub async fn serve_mcp_stdio_phase19_p2_from_args_v1(args: Vec<String>) -> Result<(), String> {
    let _phase18_p5_entrypoint_v1 = serve_mcp_stdio_p5_from_args_v1;
    let config = parse_mcp_config_v1(args).map_err(|error| error.to_string())?;
    OmniCreatorMcpTaskServerV1::reconcile_interrupted_control_tasks_v1(&config)?;
    let service = OmniCreatorMcpPhase19P2ServerV1::new(config)
        .serve(stdio())
        .await
        .map_err(|error| format!("unable to start MCP stdio service: {error}"))?;
    service
        .waiting()
        .await
        .map_err(|error| format!("MCP stdio service stopped with an error: {error}"))?;
    Ok(())
}
