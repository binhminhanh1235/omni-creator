use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};
use omnicreator_core::{
    ComputeDeviceV1, ComputeProvider, ComputeProviderCapabilitiesV1, ComputeProviderConnectV1,
    ComputeProviderConnectionState, ComputeProviderHeartbeatV1, ComputeProviderLivenessPolicyV1,
    ComputeProviderMetadataV1, ComputeProviderRuntime, Error, Result,
};

#[derive(Debug)]
struct FakeProvider {
    metadata: ComputeProviderMetadataV1,
    connect_sessions: VecDeque<String>,
    capabilities: ComputeProviderCapabilitiesV1,
    heartbeat_health: VecDeque<bool>,
    disconnect_calls: usize,
    discover_calls: usize,
}

impl FakeProvider {
    fn new(connect_sessions: &[&str], capabilities: ComputeProviderCapabilitiesV1) -> Self {
        Self {
            metadata: ComputeProviderMetadataV1 {
                provider_id: capabilities.provider_id.clone(),
                display_name: Some("Offline test provider".to_owned()),
                implementation_version: Some("1.0.0".to_owned()),
            },
            connect_sessions: connect_sessions
                .iter()
                .map(|session| (*session).to_owned())
                .collect(),
            capabilities,
            heartbeat_health: VecDeque::new(),
            disconnect_calls: 0,
            discover_calls: 0,
        }
    }

    fn with_heartbeats(mut self, health: &[bool]) -> Self {
        self.heartbeat_health = health.iter().copied().collect();
        self
    }
}

impl ComputeProvider for FakeProvider {
    fn metadata(&self) -> ComputeProviderMetadataV1 {
        self.metadata.clone()
    }

    fn connect(&mut self) -> Result<ComputeProviderConnectV1> {
        let session_id = self.connect_sessions.pop_front().ok_or_else(|| {
            Error::InvalidContract("offline fake provider has no next session".to_owned())
        })?;
        Ok(ComputeProviderConnectV1 { session_id })
    }

    fn disconnect(&mut self, _session_id: &str) -> Result<()> {
        self.disconnect_calls += 1;
        Ok(())
    }

    fn heartbeat(&mut self, session_id: &str) -> Result<ComputeProviderHeartbeatV1> {
        Ok(ComputeProviderHeartbeatV1 {
            session_id: session_id.to_owned(),
            healthy: self.heartbeat_health.pop_front().unwrap_or(true),
        })
    }

    fn discover_capabilities(
        &mut self,
        _session_id: &str,
    ) -> Result<ComputeProviderCapabilitiesV1> {
        self.discover_calls += 1;
        Ok(self.capabilities.clone())
    }
}

fn fixture_capabilities() -> ComputeProviderCapabilitiesV1 {
    ComputeProviderCapabilitiesV1::from_json_v1(include_str!(
        "fixtures/contracts/v1/compute-capabilities.json"
    ))
    .unwrap()
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-09-04T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn liveness_policy() -> ComputeProviderLivenessPolicyV1 {
    ComputeProviderLivenessPolicyV1 {
        stale_after_seconds: 10,
        lost_after_seconds: 30,
    }
}

#[test]
fn t4_x2_fixture_discovers_two_independent_devices() {
    let capabilities = fixture_capabilities();

    assert_eq!(capabilities.devices.len(), 2);
    assert_eq!(capabilities.max_parallel_jobs, Some(2));
    assert_ne!(capabilities.devices[0].id, capabilities.devices[1].id);
    assert_eq!(capabilities.devices[0].memory_mb, Some(15_360));
    assert_eq!(capabilities.devices[1].memory_mb, Some(15_360));
    assert!(
        capabilities
            .devices
            .iter()
            .all(|device| device.memory_mb != Some(30_720)),
        "device discovery must not pool two T4 memories into a fake 30 GB device"
    );
}

#[test]
fn capability_validation_rejects_invalid_ids_duplicates_and_zero_parallelism() {
    let valid = fixture_capabilities();

    let mut empty_provider = valid.clone();
    empty_provider.provider_id = "   ".to_owned();
    assert!(empty_provider.validate_v1().is_err());

    let mut empty_device = valid.clone();
    empty_device.devices[0].id.clear();
    assert!(empty_device.validate_v1().is_err());

    let mut duplicate_devices = valid.clone();
    duplicate_devices.devices[1].id = duplicate_devices.devices[0].id.clone();
    assert!(duplicate_devices.validate_v1().is_err());

    let mut zero_parallelism = valid;
    zero_parallelism.max_parallel_jobs = Some(0);
    assert!(zero_parallelism.validate_v1().is_err());
}

#[test]
fn capability_parser_normalizes_model_groups_deterministically() {
    let mut capabilities = fixture_capabilities();
    capabilities.model_groups = vec![
        " omnivoice-v3.2 ".to_owned(),
        "flux-schnell".to_owned(),
        "omnivoice-v3.2".to_owned(),
    ];

    let normalized = capabilities.normalized_v1().unwrap();

    assert_eq!(
        normalized.model_groups,
        vec!["flux-schnell".to_owned(), "omnivoice-v3.2".to_owned()]
    );

    capabilities.model_groups.push("  ".to_owned());
    assert!(capabilities.normalized_v1().is_err());
}

#[test]
fn device_validation_rejects_zero_memory_and_blank_model() {
    let zero_memory = ComputeDeviceV1 {
        id: "gpu0".to_owned(),
        device_type: "gpu".to_owned(),
        model: Some("NVIDIA T4".to_owned()),
        memory_mb: Some(0),
    };
    assert!(zero_memory.validate_v1().is_err());

    let blank_model = ComputeDeviceV1 {
        id: "gpu0".to_owned(),
        device_type: "gpu".to_owned(),
        model: Some(" ".to_owned()),
        memory_mb: Some(15_360),
    };
    assert!(blank_model.validate_v1().is_err());
}

#[test]
fn connect_discovers_capabilities_and_disconnect_is_idempotent() {
    let provider = FakeProvider::new(&["session-1"], fixture_capabilities());
    let mut runtime = ComputeProviderRuntime::new(provider, liveness_policy()).unwrap();

    assert_eq!(
        runtime.state(),
        ComputeProviderConnectionState::Disconnected
    );

    let session = runtime.connect(fixed_time()).unwrap();
    assert_eq!(session.identity.provider_id, "kaggle-session");
    assert_eq!(session.identity.session_id, "session-1");
    assert_eq!(session.capabilities.devices.len(), 2);
    assert_eq!(runtime.state(), ComputeProviderConnectionState::Ready);
    assert_eq!(runtime.provider().discover_calls, 1);

    runtime.disconnect().unwrap();
    runtime.disconnect().unwrap();

    assert_eq!(
        runtime.state(),
        ComputeProviderConnectionState::Disconnected
    );
    assert!(runtime.session().is_none());
    assert_eq!(runtime.provider().disconnect_calls, 1);
}

#[test]
fn stale_lost_and_reconnect_are_deterministic() {
    let provider =
        FakeProvider::new(&["session-1", "session-2"], fixture_capabilities()).with_heartbeats(&[
            true, true,
        ]);
    let mut runtime = ComputeProviderRuntime::new(provider, liveness_policy()).unwrap();
    let start = fixed_time();

    assert_eq!(
        runtime.connect(start).unwrap().identity.session_id,
        "session-1"
    );
    assert_eq!(
        runtime.heartbeat(start + Duration::seconds(5)).unwrap(),
        ComputeProviderConnectionState::Ready
    );
    assert_eq!(
        runtime
            .refresh_liveness(start + Duration::seconds(14))
            .unwrap(),
        ComputeProviderConnectionState::Ready
    );
    assert_eq!(
        runtime
            .refresh_liveness(start + Duration::seconds(15))
            .unwrap(),
        ComputeProviderConnectionState::Stale
    );

    assert_eq!(
        runtime.heartbeat(start + Duration::seconds(16)).unwrap(),
        ComputeProviderConnectionState::Ready,
        "a valid heartbeat may recover a stale session"
    );
    assert_eq!(
        runtime
            .refresh_liveness(start + Duration::seconds(46))
            .unwrap(),
        ComputeProviderConnectionState::Lost
    );

    runtime.disconnect().unwrap();
    assert_eq!(
        runtime.state(),
        ComputeProviderConnectionState::Disconnected
    );
    assert_eq!(
        runtime.provider().disconnect_calls,
        0,
        "a lost worker is cleared locally without pretending remote disconnect succeeded"
    );

    assert_eq!(
        runtime
            .connect(start + Duration::seconds(47))
            .unwrap()
            .identity
            .session_id,
        "session-2"
    );
    assert_eq!(runtime.state(), ComputeProviderConnectionState::Ready);

    runtime.disconnect().unwrap();
    assert_eq!(runtime.provider().disconnect_calls, 1);
}

#[test]
fn unhealthy_heartbeat_updates_liveness_without_claiming_ready() {
    let provider =
        FakeProvider::new(&["session-1"], fixture_capabilities()).with_heartbeats(&[false]);
    let mut runtime = ComputeProviderRuntime::new(provider, liveness_policy()).unwrap();
    let start = fixed_time();

    runtime.connect(start).unwrap();
    assert_eq!(
        runtime.heartbeat(start + Duration::seconds(9)).unwrap(),
        ComputeProviderConnectionState::Unhealthy
    );
    assert_eq!(
        runtime
            .refresh_liveness(start + Duration::seconds(18))
            .unwrap(),
        ComputeProviderConnectionState::Unhealthy
    );
    assert_eq!(
        runtime
            .refresh_liveness(start + Duration::seconds(19))
            .unwrap(),
        ComputeProviderConnectionState::Stale
    );
}

#[test]
fn invalid_discovered_capabilities_fail_connect_without_leaking_session_state() {
    let mut capabilities = fixture_capabilities();
    capabilities.devices[1].id = capabilities.devices[0].id.clone();
    let provider = FakeProvider::new(&["session-1"], capabilities);
    let mut runtime = ComputeProviderRuntime::new(provider, liveness_policy()).unwrap();

    assert!(runtime.connect(fixed_time()).is_err());
    assert_eq!(
        runtime.state(),
        ComputeProviderConnectionState::Disconnected
    );
    assert!(runtime.session().is_none());
}

#[test]
fn provider_id_mismatch_is_rejected_during_discovery() {
    let capabilities = fixture_capabilities();
    let mut provider = FakeProvider::new(&["session-1"], capabilities);
    provider.metadata.provider_id = "another-provider".to_owned();
    let mut runtime = ComputeProviderRuntime::new(provider, liveness_policy()).unwrap();

    assert!(runtime.connect(fixed_time()).is_err());
    assert_eq!(
        runtime.state(),
        ComputeProviderConnectionState::Disconnected
    );
}

#[test]
fn liveness_policy_rejects_non_deterministic_thresholds() {
    assert!(
        ComputeProviderLivenessPolicyV1 {
            stale_after_seconds: 0,
            lost_after_seconds: 30,
        }
        .validate_v1()
        .is_err()
    );
    assert!(
        ComputeProviderLivenessPolicyV1 {
            stale_after_seconds: 30,
            lost_after_seconds: 30,
        }
        .validate_v1()
        .is_err()
    );
}
