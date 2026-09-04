use std::{
    collections::{BTreeMap, VecDeque},
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    DiscoveredPlugin, Error, PluginProgressEvent, PluginRequest, PluginResponse, Result,
    PLUGIN_API_VERSION,
};

const MAX_PENDING_FRAMES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginProcessOptions {
    pub request_timeout: Duration,
    pub shutdown_grace: Duration,
    pub stderr_capacity: usize,
}

impl Default for PluginProcessOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
            shutdown_grace: Duration::from_secs(2),
            stderr_capacity: 200,
        }
    }
}

impl PluginProcessOptions {
    pub fn validate(&self) -> Result<()> {
        if self.request_timeout.is_zero() {
            return Err(Error::InvalidContract(
                "plugin request_timeout must be greater than zero".to_owned(),
            ));
        }
        if self.shutdown_grace.is_zero() {
            return Err(Error::InvalidContract(
                "plugin shutdown_grace must be greater than zero".to_owned(),
            ));
        }
        if self.stderr_capacity == 0 {
            return Err(Error::InvalidContract(
                "plugin stderr_capacity must be greater than zero".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginCallResult {
    pub request_id: String,
    pub response: PluginResponse,
    pub progress: Vec<PluginProgressEvent>,
}

#[derive(Clone)]
pub struct PluginProcessControl {
    plugin_id: String,
    stdin: Arc<Mutex<ChildStdin>>,
}

impl PluginProcessControl {
    pub fn send(&self, request: &PluginRequest) -> Result<()> {
        request.validate_v1()?;
        let payload = serde_json::to_string(request)?;
        let mut stdin = self.stdin.lock().map_err(|_| Error::PluginRuntimeIo {
            plugin: self.plugin_id.clone(),
            message: "stdin lock was poisoned".to_owned(),
        })?;
        stdin
            .write_all(payload.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|error| Error::PluginRuntimeIo {
                plugin: self.plugin_id.clone(),
                message: error.to_string(),
            })
    }

    pub fn request_cancel(&self, target_request_id: &str) -> Result<String> {
        if target_request_id.trim().is_empty() {
            return Err(Error::InvalidContract(
                "cancel target request_id must not be empty".to_owned(),
            ));
        }
        let request =
            new_plugin_request("plugin.cancel", json!({ "request_id": target_request_id }));
        let request_id = request.request_id.clone();
        self.send(&request)?;
        Ok(request_id)
    }
}

pub struct PluginProcess {
    plugin_id: String,
    child: Mutex<Child>,
    control: PluginProcessControl,
    inbox: Mutex<PluginInbox>,
    stderr: Arc<Mutex<VecDeque<String>>>,
    options: PluginProcessOptions,
}

impl PluginProcess {
    pub fn spawn(plugin: &DiscoveredPlugin, options: PluginProcessOptions) -> Result<Self> {
        plugin.manifest.validate_v1()?;
        options.validate()?;

        let mut command = Command::new(&plugin.manifest.entrypoint.command);
        command
            .args(&plugin.manifest.entrypoint.args)
            .current_dir(&plugin.directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| Error::PluginSpawn {
            plugin: plugin.manifest.id.clone(),
            message: error.to_string(),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| Error::PluginSpawn {
            plugin: plugin.manifest.id.clone(),
            message: "failed to capture plugin stdin".to_owned(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| Error::PluginSpawn {
            plugin: plugin.manifest.id.clone(),
            message: "failed to capture plugin stdout".to_owned(),
        })?;
        let stderr_pipe = child.stderr.take().ok_or_else(|| Error::PluginSpawn {
            plugin: plugin.manifest.id.clone(),
            message: "failed to capture plugin stderr".to_owned(),
        })?;

        let (sender, receiver) = mpsc::channel();
        let stdout_plugin_id = plugin.manifest.id.clone();
        thread::Builder::new()
            .name(format!("plugin-{}-stdout", plugin.manifest.id))
            .spawn(move || {
                read_plugin_stdout(&stdout_plugin_id, stdout, sender);
            })
            .map_err(|error| Error::PluginSpawn {
                plugin: plugin.manifest.id.clone(),
                message: format!("failed to start stdout reader: {error}"),
            })?;

        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        let stderr_buffer = Arc::clone(&stderr);
        let stderr_capacity = options.stderr_capacity;
        thread::Builder::new()
            .name(format!("plugin-{}-stderr", plugin.manifest.id))
            .spawn(move || {
                let reader = BufReader::new(stderr_pipe);
                for line in reader.lines() {
                    let Ok(line) = line else {
                        break;
                    };
                    let Ok(mut buffer) = stderr_buffer.lock() else {
                        break;
                    };
                    while buffer.len() >= stderr_capacity {
                        buffer.pop_front();
                    }
                    buffer.push_back(line);
                }
            })
            .map_err(|error| Error::PluginSpawn {
                plugin: plugin.manifest.id.clone(),
                message: format!("failed to start stderr reader: {error}"),
            })?;

        let stdin = Arc::new(Mutex::new(stdin));
        Ok(Self {
            plugin_id: plugin.manifest.id.clone(),
            child: Mutex::new(child),
            control: PluginProcessControl {
                plugin_id: plugin.manifest.id.clone(),
                stdin,
            },
            inbox: Mutex::new(PluginInbox::new(receiver)),
            stderr,
            options,
        })
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    pub fn control(&self) -> PluginProcessControl {
        self.control.clone()
    }

    pub fn is_running(&self) -> Result<bool> {
        let mut child = self.child.lock().map_err(|_| Error::PluginRuntimeIo {
            plugin: self.plugin_id.clone(),
            message: "child lock was poisoned".to_owned(),
        })?;
        Ok(child
            .try_wait()
            .map_err(|error| Error::PluginRuntimeIo {
                plugin: self.plugin_id.clone(),
                message: error.to_string(),
            })?
            .is_none())
    }

    pub fn stderr_tail(&self) -> Result<Vec<String>> {
        let stderr = self.stderr.lock().map_err(|_| Error::PluginRuntimeIo {
            plugin: self.plugin_id.clone(),
            message: "stderr diagnostics lock was poisoned".to_owned(),
        })?;
        Ok(stderr.iter().cloned().collect())
    }

    pub fn call(&self, method: &str, params: Value) -> Result<PluginCallResult> {
        self.call_with_timeout(method, params, self.options.request_timeout)
    }

    pub fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<PluginCallResult> {
        if timeout.is_zero() {
            return Err(Error::InvalidContract(
                "plugin call timeout must be greater than zero".to_owned(),
            ));
        }
        let request = new_plugin_request(method, params);
        self.call_request_with_timeout(request, timeout)
    }

    pub fn call_request(&self, request: PluginRequest) -> Result<PluginCallResult> {
        self.call_request_with_timeout(request, self.options.request_timeout)
    }

    pub fn call_request_with_timeout(
        &self,
        request: PluginRequest,
        timeout: Duration,
    ) -> Result<PluginCallResult> {
        request.validate_v1()?;
        let request_id = request.request_id.clone();
        self.control.send(&request)?;
        self.wait_for_response(&request_id, timeout)
    }

    pub fn initialize(&self, params: Value) -> Result<PluginCallResult> {
        self.call("plugin.initialize", params)
    }

    pub fn health(&self) -> Result<PluginCallResult> {
        self.call("plugin.health", Value::Null)
    }

    pub fn capabilities(&self) -> Result<PluginCallResult> {
        self.call("plugin.capabilities", Value::Null)
    }

    pub fn execute(&self, operation: &str, payload: Value) -> Result<PluginCallResult> {
        if operation.trim().is_empty() {
            return Err(Error::InvalidContract(
                "plugin execute operation must not be empty".to_owned(),
            ));
        }
        self.call(
            "plugin.execute",
            json!({
                "operation": operation,
                "payload": payload,
            }),
        )
    }

    pub fn cancel(&self, target_request_id: &str) -> Result<PluginCallResult> {
        let request_id = self.control.request_cancel(target_request_id)?;
        self.wait_for_response(&request_id, self.options.request_timeout)
    }

    pub fn shutdown(&self) -> Result<()> {
        if !self.is_running()? {
            return Ok(());
        }

        let shutdown_result = self.call("plugin.shutdown", Value::Null);
        match shutdown_result {
            Ok(_) => {}
            Err(Error::PluginProcessExited {
                status: Some(0), ..
            }) => return Ok(()),
            Err(error) => {
                self.force_terminate()?;
                return Err(error);
            }
        }

        if self.wait_for_exit(self.options.shutdown_grace)?.is_none() {
            self.force_terminate()?;
        }
        Ok(())
    }

    pub fn force_terminate(&self) -> Result<()> {
        let mut child = self.child.lock().map_err(|_| Error::PluginRuntimeIo {
            plugin: self.plugin_id.clone(),
            message: "child lock was poisoned".to_owned(),
        })?;

        match child.try_wait().map_err(|error| Error::PluginRuntimeIo {
            plugin: self.plugin_id.clone(),
            message: error.to_string(),
        })? {
            Some(_) => return Ok(()),
            None => {}
        }

        child.kill().map_err(|error| Error::PluginRuntimeIo {
            plugin: self.plugin_id.clone(),
            message: format!("failed to kill plugin process: {error}"),
        })?;
        child.wait().map_err(|error| Error::PluginRuntimeIo {
            plugin: self.plugin_id.clone(),
            message: format!("failed to reap plugin process: {error}"),
        })?;
        Ok(())
    }

    fn wait_for_response(&self, request_id: &str, timeout: Duration) -> Result<PluginCallResult> {
        let deadline = Instant::now() + timeout;
        let mut progress = Vec::new();
        let mut inbox = self.inbox.lock().map_err(|_| Error::PluginProtocol {
            plugin: self.plugin_id.clone(),
            message: "stdout inbox lock was poisoned".to_owned(),
        })?;

        loop {
            while let Some(message) = inbox.take_pending(request_id) {
                if let Some(response) = collect_message(message, &mut progress) {
                    return Ok(PluginCallResult {
                        request_id: request_id.to_owned(),
                        response,
                        progress,
                    });
                }
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(Error::PluginTimeout {
                    plugin: self.plugin_id.clone(),
                    request_id: request_id.to_owned(),
                });
            }

            match inbox
                .receiver
                .recv_timeout(deadline.saturating_duration_since(now))
            {
                Ok(ReaderFrame::Message(message)) => {
                    let message_request_id = wire_request_id(&message).to_owned();
                    if message_request_id == request_id {
                        if let Some(response) = collect_message(message, &mut progress) {
                            return Ok(PluginCallResult {
                                request_id: request_id.to_owned(),
                                response,
                                progress,
                            });
                        }
                    } else {
                        inbox
                            .stash(message)
                            .map_err(|message| Error::PluginProtocol {
                                plugin: self.plugin_id.clone(),
                                message,
                            })?;
                    }
                }
                Ok(ReaderFrame::Malformed(message)) => {
                    return Err(Error::PluginProtocol {
                        plugin: self.plugin_id.clone(),
                        message,
                    });
                }
                Ok(ReaderFrame::Eof) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(self.process_exited_error()?);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(Error::PluginTimeout {
                        plugin: self.plugin_id.clone(),
                        request_id: request_id.to_owned(),
                    });
                }
            }
        }
    }

    fn process_exited_error(&self) -> Result<Error> {
        let mut child = self.child.lock().map_err(|_| Error::PluginRuntimeIo {
            plugin: self.plugin_id.clone(),
            message: "child lock was poisoned".to_owned(),
        })?;
        let status = match child.try_wait() {
            Ok(Some(status)) => status.code(),
            Ok(None) => None,
            Err(error) => {
                return Err(Error::PluginRuntimeIo {
                    plugin: self.plugin_id.clone(),
                    message: error.to_string(),
                });
            }
        };
        Ok(Error::PluginProcessExited {
            plugin: self.plugin_id.clone(),
            status,
        })
    }

    fn wait_for_exit(&self, timeout: Duration) -> Result<Option<i32>> {
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut child = self.child.lock().map_err(|_| Error::PluginRuntimeIo {
                    plugin: self.plugin_id.clone(),
                    message: "child lock was poisoned".to_owned(),
                })?;
                if let Some(status) = child.try_wait().map_err(|error| Error::PluginRuntimeIo {
                    plugin: self.plugin_id.clone(),
                    message: error.to_string(),
                })? {
                    return Ok(status.code());
                }
            }

            if Instant::now() >= deadline {
                return Ok(None);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        let _ = self.force_terminate();
    }
}

fn new_plugin_request(method: &str, params: Value) -> PluginRequest {
    PluginRequest {
        api_version: PLUGIN_API_VERSION,
        request_id: format!("req_{}", Uuid::new_v4().simple()),
        method: method.to_owned(),
        params,
    }
}

enum ReaderFrame {
    Message(PluginWireMessage),
    Malformed(String),
    Eof,
}

enum PluginWireMessage {
    Response(PluginResponse),
    Progress(PluginProgressEvent),
}

fn wire_request_id(message: &PluginWireMessage) -> &str {
    match message {
        PluginWireMessage::Response(PluginResponse::Success { request_id, .. })
        | PluginWireMessage::Response(PluginResponse::Failure { request_id, .. }) => request_id,
        PluginWireMessage::Progress(event) => &event.request_id,
    }
}

fn collect_message(
    message: PluginWireMessage,
    progress: &mut Vec<PluginProgressEvent>,
) -> Option<PluginResponse> {
    match message {
        PluginWireMessage::Progress(event) => {
            progress.push(event);
            None
        }
        PluginWireMessage::Response(response) => Some(response),
    }
}

struct PluginInbox {
    receiver: mpsc::Receiver<ReaderFrame>,
    pending: BTreeMap<String, VecDeque<PluginWireMessage>>,
    pending_count: usize,
}

impl PluginInbox {
    fn new(receiver: mpsc::Receiver<ReaderFrame>) -> Self {
        Self {
            receiver,
            pending: BTreeMap::new(),
            pending_count: 0,
        }
    }

    fn take_pending(&mut self, request_id: &str) -> Option<PluginWireMessage> {
        let queue = self.pending.get_mut(request_id)?;
        let message = queue.pop_front();
        if message.is_some() {
            self.pending_count = self.pending_count.saturating_sub(1);
        }
        if queue.is_empty() {
            self.pending.remove(request_id);
        }
        message
    }

    fn stash(&mut self, message: PluginWireMessage) -> std::result::Result<(), String> {
        if self.pending_count >= MAX_PENDING_FRAMES {
            return Err(format!(
                "plugin produced more than {MAX_PENDING_FRAMES} unmatched frames"
            ));
        }
        let request_id = wire_request_id(&message).to_owned();
        self.pending
            .entry(request_id)
            .or_default()
            .push_back(message);
        self.pending_count += 1;
        Ok(())
    }
}

fn read_plugin_stdout(
    plugin_id: &str,
    stdout: impl std::io::Read,
    sender: mpsc::Sender<ReaderFrame>,
) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let frame = match line {
            Ok(line) => match parse_wire_message(&line) {
                Ok(message) => ReaderFrame::Message(message),
                Err(message) => ReaderFrame::Malformed(format!(
                    "invalid stdout frame from plugin {plugin_id}: {message}"
                )),
            },
            Err(error) => ReaderFrame::Malformed(format!(
                "failed reading stdout from plugin {plugin_id}: {error}"
            )),
        };
        if sender.send(frame).is_err() {
            return;
        }
    }
    let _ = sender.send(ReaderFrame::Eof);
}

fn parse_wire_message(raw: &str) -> std::result::Result<PluginWireMessage, String> {
    if raw.trim().is_empty() {
        return Err("stdout line is empty".to_owned());
    }
    let value: Value =
        serde_json::from_str(raw).map_err(|error| format!("invalid JSON: {error}"))?;

    if value.get("event").is_some() {
        let event: PluginProgressEvent = serde_json::from_value(value)
            .map_err(|error| format!("invalid progress event: {error}"))?;
        event
            .validate_v1()
            .map_err(|error| format!("invalid progress event: {error}"))?;
        return Ok(PluginWireMessage::Progress(event));
    }

    let response: PluginResponse = serde_json::from_value(value)
        .map_err(|error| format!("invalid response envelope: {error}"))?;
    response
        .validate_v1()
        .map_err(|error| format!("invalid response envelope: {error}"))?;
    Ok(PluginWireMessage::Response(response))
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command, thread};

    use tempfile::tempdir;

    use crate::{
        PluginEntrypoint, PluginManifest, PluginPermissions, PLUGIN_MANIFEST_SCHEMA,
        PLUGIN_MANIFEST_SCHEMA_VERSION,
    };

    use super::*;

    fn python_available() -> bool {
        Command::new("python3").arg("--version").output().is_ok()
    }

    fn fixture_plugin() -> Option<(tempfile::TempDir, DiscoveredPlugin)> {
        if !python_available() {
            return None;
        }

        let temp = tempdir().unwrap();
        let script_path = temp.path().join("fixture.py");
        fs::write(
            &script_path,
            r#"import json
import sys
import time

for raw in sys.stdin:
    request = json.loads(raw)
    request_id = request["request_id"]
    method = request["method"]

    if method == "fixture.malformed":
        print("{broken-json", flush=True)
        continue

    if method == "fixture.exit":
        sys.exit(7)

    if method == "fixture.sleep":
        time.sleep(0.2)

    if method == "plugin.execute":
        print(json.dumps({
            "api_version": 1,
            "event": "progress",
            "request_id": request_id,
            "progress": {"percent": 42, "message": "fixture progress"}
        }), flush=True)

    if method == "plugin.health":
        print("health diagnostic", file=sys.stderr, flush=True)

    result = {"method": method, "params": request.get("params")}
    print(json.dumps({
        "api_version": 1,
        "request_id": request_id,
        "result": result
    }), flush=True)

    if method == "plugin.shutdown":
        break
"#,
        )
        .unwrap();

        let manifest = PluginManifest {
            schema: PLUGIN_MANIFEST_SCHEMA.to_owned(),
            schema_version: PLUGIN_MANIFEST_SCHEMA_VERSION,
            id: "fixture".to_owned(),
            name: "Fixture Plugin".to_owned(),
            version: "1.0.0".to_owned(),
            api_version: PLUGIN_API_VERSION,
            types: vec!["quality".to_owned()],
            entrypoint: PluginEntrypoint {
                command: "python3".to_owned(),
                args: vec!["fixture.py".to_owned()],
            },
            capabilities: vec!["fixture".to_owned()],
            scene_types: Vec::new(),
            permissions: PluginPermissions::default(),
            settings: None,
            resources: None,
        };

        let plugin = DiscoveredPlugin {
            directory: temp.path().to_path_buf(),
            manifest_path: temp.path().join("plugin.json"),
            manifest,
        };
        Some((temp, plugin))
    }

    fn process_options() -> PluginProcessOptions {
        PluginProcessOptions {
            request_timeout: Duration::from_secs(2),
            shutdown_grace: Duration::from_secs(1),
            stderr_capacity: 8,
        }
    }

    #[test]
    fn process_correlates_response_and_progress() {
        let Some((_temp, plugin)) = fixture_plugin() else {
            return;
        };
        let process = PluginProcess::spawn(&plugin, process_options()).unwrap();

        let result = process
            .execute("visual.resolve", json!({"scene_id": "SC01"}))
            .unwrap();

        assert_eq!(result.progress.len(), 1);
        assert_eq!(result.progress[0].progress.percent, 42);
        match result.response {
            PluginResponse::Success { result, .. } => {
                assert_eq!(result["method"], "plugin.execute");
                assert_eq!(result["params"]["operation"], "visual.resolve");
            }
            PluginResponse::Failure { .. } => panic!("fixture returned failure"),
        }

        process.shutdown().unwrap();
        assert!(!process.is_running().unwrap());
    }

    #[test]
    fn timeout_does_not_break_later_request_correlation() {
        let Some((_temp, plugin)) = fixture_plugin() else {
            return;
        };
        let process = PluginProcess::spawn(&plugin, process_options()).unwrap();

        let timeout = process
            .call_with_timeout("fixture.sleep", Value::Null, Duration::from_millis(20))
            .unwrap_err();
        assert!(matches!(timeout, Error::PluginTimeout { .. }));

        let health = process.health().unwrap();
        match health.response {
            PluginResponse::Success { result, .. } => {
                assert_eq!(result["method"], "plugin.health");
            }
            PluginResponse::Failure { .. } => panic!("fixture returned failure"),
        }

        process.shutdown().unwrap();
    }

    #[test]
    fn malformed_stdout_is_a_protocol_error() {
        let Some((_temp, plugin)) = fixture_plugin() else {
            return;
        };
        let process = PluginProcess::spawn(&plugin, process_options()).unwrap();

        let error = process.call("fixture.malformed", Value::Null).unwrap_err();

        assert!(matches!(error, Error::PluginProtocol { .. }));
        process.force_terminate().unwrap();
    }

    #[test]
    fn stderr_is_captured_as_bounded_diagnostics() {
        let Some((_temp, plugin)) = fixture_plugin() else {
            return;
        };
        let process = PluginProcess::spawn(&plugin, process_options()).unwrap();

        process.health().unwrap();
        thread::sleep(Duration::from_millis(30));
        let diagnostics = process.stderr_tail().unwrap();

        assert!(diagnostics
            .iter()
            .any(|line| line.contains("health diagnostic")));
        process.shutdown().unwrap();
    }

    #[test]
    fn unexpected_process_exit_is_reported() {
        let Some((_temp, plugin)) = fixture_plugin() else {
            return;
        };
        let process = PluginProcess::spawn(&plugin, process_options()).unwrap();

        let error = process.call("fixture.exit", Value::Null).unwrap_err();

        assert!(matches!(
            error,
            Error::PluginProcessExited {
                status: Some(7),
                ..
            }
        ));
    }

    #[test]
    fn control_can_send_cancel_without_waiting_for_the_main_inbox() {
        let Some((_temp, plugin)) = fixture_plugin() else {
            return;
        };
        let process = PluginProcess::spawn(&plugin, process_options()).unwrap();
        let control = process.control();

        let cancel_request_id = control.request_cancel("req_target").unwrap();
        assert!(cancel_request_id.starts_with("req_"));

        let response = process
            .wait_for_response(&cancel_request_id, Duration::from_secs(1))
            .unwrap();
        match response.response {
            PluginResponse::Success { result, .. } => {
                assert_eq!(result["method"], "plugin.cancel");
                assert_eq!(result["params"]["request_id"], "req_target");
            }
            PluginResponse::Failure { .. } => panic!("fixture returned failure"),
        }

        process.shutdown().unwrap();
    }
}
