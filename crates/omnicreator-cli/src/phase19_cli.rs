use std::{
    collections::BTreeMap,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

use omnicreator_application::{
    ApplicationControlService, ControlErrorCodeV1, ControlErrorV1, ExternalResultBatchRequestV1,
    ProjectIdRequestV1,
};
use omnicreator_core::{Workspace, WorkspaceSession};
use serde::Serialize;
use serde_json::Value;

const CLI_RESPONSE_SCHEMA_V1: &str = "omnicreator.cli-response";
const CLI_RESPONSE_VERSION_V1: u32 = 1;

#[derive(Debug, Serialize)]
struct Phase19CliEnvelopeV1 {
    schema: &'static str,
    version: u32,
    ok: bool,
    operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ControlErrorV1>,
}

#[derive(Debug, Clone)]
struct Phase19CliOptionsV1 {
    data_root: PathBuf,
    read_only: bool,
    json: bool,
    device_id: String,
    verb: String,
    command_args: Vec<String>,
}

#[derive(Debug, Clone)]
struct Phase19CommandArgsV1 {
    items: Vec<String>,
}

impl Phase19CommandArgsV1 {
    fn new(items: Vec<String>) -> Self {
        Self { items }
    }

    fn take_flag(&mut self, flag: &str) -> bool {
        if let Some(index) = self.items.iter().position(|item| item == flag) {
            self.items.remove(index);
            true
        } else {
            false
        }
    }

    fn take_value(&mut self, flag: &str) -> Result<Option<String>, ControlErrorV1> {
        let Some(index) = self.items.iter().position(|item| item == flag) else {
            return Ok(None);
        };
        if index + 1 >= self.items.len() {
            return Err(invalid_input_v1(format!("{flag} requires a value")));
        }
        self.items.remove(index);
        Ok(Some(self.items.remove(index)))
    }

    fn require_value(&mut self, flag: &str) -> Result<String, ControlErrorV1> {
        self.take_value(flag)?
            .ok_or_else(|| invalid_input_v1(format!("missing required {flag}")))
    }

    fn finish(&self) -> Result<(), ControlErrorV1> {
        if self.items.is_empty() {
            Ok(())
        } else {
            Err(invalid_input_v1(format!(
                "unexpected arguments: {}",
                self.items.join(" ")
            )))
        }
    }
}

pub fn maybe_run_phase19_work_cli_v1(
    args: &[String],
    stdin: &mut dyn Read,
) -> Option<omnicreator_cli::CliRunResultV1> {
    let parsed = match parse_phase19_invocation_v1(args) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return None,
        Err((operation, error, json, data_root)) => {
            return Some(render_v1(
                operation,
                Err(sanitize_error_v1(error, data_root.as_deref())),
                json,
            ));
        }
    };
    let operation = format!("work.{}", parsed.verb);
    let result = execute_phase19_work_v1(&parsed, stdin)
        .map_err(|error| sanitize_error_v1(error, Some(&parsed.data_root)));
    Some(render_v1(operation, result, parsed.json))
}

fn parse_phase19_invocation_v1(
    args: &[String],
) -> Result<Option<Phase19CliOptionsV1>, (String, ControlErrorV1, bool, Option<PathBuf>)> {
    let mut data_root = None::<PathBuf>;
    let mut read_only = false;
    let mut json = false;
    let mut device_id = None::<String>;
    let mut command = Vec::<String>::new();
    let mut index = 0usize;

    while index < args.len() {
        match args[index].as_str() {
            "--data-root" => {
                let Some(value) = args.get(index + 1) else {
                    return Err((
                        "work.parse".to_owned(),
                        invalid_input_v1("--data-root requires a value"),
                        json,
                        data_root,
                    ));
                };
                data_root = Some(PathBuf::from(value));
                index += 2;
            }
            "--read-only" => {
                read_only = true;
                index += 1;
            }
            "--json" => {
                json = true;
                index += 1;
            }
            "--device-id" => {
                let Some(value) = args.get(index + 1) else {
                    return Err((
                        "work.parse".to_owned(),
                        invalid_input_v1("--device-id requires a value"),
                        json,
                        data_root,
                    ));
                };
                device_id = Some(value.clone());
                index += 2;
            }
            "--llm-provider-config" | "--llmgateway-config" | "--studio-pack-catalog" => {
                if args.get(index + 1).is_none() {
                    return Err((
                        "work.parse".to_owned(),
                        invalid_input_v1(format!("{} requires a value", args[index])),
                        json,
                        data_root,
                    ));
                }
                index += 2;
            }
            _ => {
                command.push(args[index].clone());
                index += 1;
            }
        }
    }

    if command.first().map(String::as_str) != Some("work") {
        return Ok(None);
    }
    if command.len() < 2 {
        return Err((
            "work.parse".to_owned(),
            invalid_input_v1("expected work <graph|prepare|commit>"),
            json,
            data_root,
        ));
    }
    let data_root = data_root.ok_or_else(|| {
        (
            "work.parse".to_owned(),
            invalid_input_v1("--data-root is required"),
            json,
            None,
        )
    })?;
    Ok(Some(Phase19CliOptionsV1 {
        data_root,
        read_only,
        json,
        device_id: device_id.unwrap_or_else(default_device_id_v1),
        verb: command[1].clone(),
        command_args: command[2..].to_vec(),
    }))
}

fn execute_phase19_work_v1(
    options: &Phase19CliOptionsV1,
    stdin: &mut dyn Read,
) -> Result<Value, ControlErrorV1> {
    match options.verb.as_str() {
        "graph" => {
            let mut args = Phase19CommandArgsV1::new(options.command_args.clone());
            let project_id = args.require_value("--project")?;
            args.finish()?;
            with_read_only_service_v1(&options.data_root, |service| {
                value_v1(service.agent_work_graph_v1(&ProjectIdRequestV1 { project_id })?)
            })
        }
        "prepare" => {
            let mut args = Phase19CommandArgsV1::new(options.command_args.clone());
            let project_id = args.require_value("--project")?;
            let work_id = args.require_value("--work")?;
            args.finish()?;
            with_read_only_service_v1(&options.data_root, |service| {
                let graph = service.agent_work_graph_v1(&ProjectIdRequestV1 { project_id })?;
                let item = graph
                    .items
                    .into_iter()
                    .find(|item| item.work_id == work_id)
                    .ok_or_else(|| {
                        ControlErrorV1::new(
                            ControlErrorCodeV1::NotFound,
                            format!("agent work item was not found: {work_id}"),
                        )
                    })?;
                if item.external.is_none() {
                    return Err(ControlErrorV1::new(
                        ControlErrorCodeV1::Blocked,
                        format!(
                            "agent work item {work_id} has no external work descriptor in current canonical state"
                        ),
                    ));
                }
                value_v1(item)
            })
        }
        "commit" => {
            let mut args = Phase19CommandArgsV1::new(options.command_args.clone());
            let request: ExternalResultBatchRequestV1 = read_payload_v1(&mut args, stdin)?;
            if options.read_only {
                with_read_only_service_v1(&options.data_root, |service| {
                    value_v1(service.provide_external_result_batch_v1(&request)?)
                })
            } else {
                let workspace =
                    Workspace::open(&options.data_root).map_err(ControlErrorV1::from)?;
                let session = WorkspaceSession::acquire(workspace, &options.device_id)
                    .map_err(ControlErrorV1::from)?;
                let mut service = ApplicationControlService::for_writer(&session)?;
                value_v1(service.provide_external_result_batch_v1(&request)?)
            }
        }
        other => Err(invalid_input_v1(format!(
            "unsupported work command: {other}; expected graph, prepare, or commit"
        ))),
    }
}

fn with_read_only_service_v1<T>(
    data_root: &Path,
    operation: impl for<'service> FnOnce(
        &mut ApplicationControlService<'service>,
    ) -> Result<T, ControlErrorV1>,
) -> Result<T, ControlErrorV1> {
    let workspace = Workspace::inspect(data_root).map_err(ControlErrorV1::from)?;
    let mut service = ApplicationControlService::for_read_only(&workspace)?;
    operation(&mut service)
}

fn read_payload_v1<T: serde::de::DeserializeOwned>(
    args: &mut Phase19CommandArgsV1,
    stdin: &mut dyn Read,
) -> Result<T, ControlErrorV1> {
    let use_stdin = args.take_flag("--stdin");
    let input_file = args.take_value("--input-file")?;
    if use_stdin == input_file.is_some() {
        return Err(invalid_input_v1(
            "use exactly one of --stdin or --input-file <path> for a structured payload",
        ));
    }
    let raw = if use_stdin {
        let mut raw = String::new();
        stdin.read_to_string(&mut raw).map_err(|error| {
            ControlErrorV1::new(ControlErrorCodeV1::InvalidInput, error.to_string())
        })?;
        raw
    } else {
        let path = input_file.expect("input file presence checked above");
        fs::read_to_string(&path).map_err(|error| {
            ControlErrorV1::new(
                ControlErrorCodeV1::InvalidInput,
                format!("unable to read input file: {error}"),
            )
        })?
    };
    args.finish()?;
    serde_json::from_str(&raw).map_err(|error| {
        ControlErrorV1::new(
            ControlErrorCodeV1::InvalidInput,
            format!("invalid structured JSON payload: {error}"),
        )
    })
}

fn render_v1(
    operation: String,
    result: Result<Value, ControlErrorV1>,
    json: bool,
) -> omnicreator_cli::CliRunResultV1 {
    let (ok, data, error, exit_code) = match result {
        Ok(data) => (true, Some(data), None, 0),
        Err(error) => {
            let code = exit_code_v1(error.code);
            (false, None, Some(error), code)
        }
    };
    let envelope = Phase19CliEnvelopeV1 {
        schema: CLI_RESPONSE_SCHEMA_V1,
        version: CLI_RESPONSE_VERSION_V1,
        ok,
        operation,
        data,
        error,
    };
    let output = if json {
        serde_json::to_string(&envelope)
    } else {
        serde_json::to_string_pretty(&envelope)
    }
    .unwrap_or_else(|error| {
        format!(
            "{{\"schema\":\"{CLI_RESPONSE_SCHEMA_V1}\",\"version\":{CLI_RESPONSE_VERSION_V1},\"ok\":false,\"operation\":\"work.serialize\",\"error\":{{\"code\":\"internal\",\"message\":\"serialization failed: {error}\"}}}}"
        )
    });
    omnicreator_cli::CliRunResultV1 { exit_code, output }
}

fn value_v1<T: Serialize>(value: T) -> Result<Value, ControlErrorV1> {
    serde_json::to_value(value).map_err(|error| {
        ControlErrorV1::new(
            ControlErrorCodeV1::Internal,
            format!("unable to serialize CLI result: {error}"),
        )
    })
}

fn invalid_input_v1(message: impl Into<String>) -> ControlErrorV1 {
    ControlErrorV1::new(ControlErrorCodeV1::InvalidInput, message)
}

fn sanitize_error_v1(mut error: ControlErrorV1, data_root: Option<&Path>) -> ControlErrorV1 {
    let mut replacements = BTreeMap::<String, &str>::new();
    if let Some(data_root) = data_root {
        replacements.insert(data_root.to_string_lossy().to_string(), "<data-root>");
        if let Ok(canonical) = fs::canonicalize(data_root) {
            replacements.insert(canonical.to_string_lossy().to_string(), "<data-root>");
        }
    }
    for (needle, replacement) in replacements {
        if !needle.is_empty() {
            error.message = error.message.replace(&needle, replacement);
        }
    }
    error.message = error.message.replace("Bearer ", "Bearer <redacted>");
    error
}

fn exit_code_v1(code: ControlErrorCodeV1) -> i32 {
    match code {
        ControlErrorCodeV1::InvalidInput => 2,
        ControlErrorCodeV1::NotFound => 3,
        ControlErrorCodeV1::ReadOnly => 4,
        ControlErrorCodeV1::WriterConflict => 5,
        ControlErrorCodeV1::InvalidTransition => 6,
        ControlErrorCodeV1::Blocked => 7,
        ControlErrorCodeV1::CapabilityUnavailable => 8,
        ControlErrorCodeV1::ProviderUnavailable => 9,
        ControlErrorCodeV1::ArtifactInvalid => 10,
        ControlErrorCodeV1::StaleInput => 11,
        ControlErrorCodeV1::Internal => 70,
    }
}

fn default_device_id_v1() -> String {
    for key in ["OMNICREATOR_DEVICE_ID", "HOSTNAME", "COMPUTERNAME"] {
        if let Ok(value) = env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                return format!("cli-{value}");
            }
        }
    }
    "omnicreator-cli-local".to_owned()
}
