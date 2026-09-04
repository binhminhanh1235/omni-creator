use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{ComputeProviderCapabilitiesV1, Error, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ComputeProviderConnectionState {
    Disconnected,
    Connecting,
    Ready,
    Unhealthy,
    Stale,
    Lost,
}

impl ComputeProviderConnectionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "DISCONNECTED",
            Self::Connecting => "CONNECTING",
            Self::Ready => "READY",
            Self::Unhealthy => "UNHEALTHY",
            Self::Stale => "STALE",
            Self::Lost => "LOST",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeProviderMetadataV1 {
    pub provider_id: String,
    pub display_name: Option<String>,
    pub implementation_version: Option<String>,
}

impl ComputeProviderMetadataV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_identifier("compute provider_id", &self.provider_id)?;
        validate_optional_non_empty(
            "compute provider display_name",
            self.display_name.as_deref(),
        )?;
        validate_optional_non_empty(
            "compute provider implementation_version",
            self.implementation_version.as_deref(),
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeProviderSessionIdentityV1 {
    pub provider_id: String,
    pub session_id: String,
}

impl ComputeProviderSessionIdentityV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_identifier("compute session provider_id", &self.provider_id)?;
        validate_identifier("compute session_id", &self.session_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeProviderSessionV1 {
    pub identity: ComputeProviderSessionIdentityV1,
    pub connected_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub last_healthy_heartbeat_at: Option<DateTime<Utc>>,
    pub capabilities: ComputeProviderCapabilitiesV1,
}

impl ComputeProviderSessionV1 {
    pub fn validate_v1(&self) -> Result<()> {
        self.identity.validate_v1()?;
        self.capabilities.validate_v1()?;
        if self.capabilities.provider_id != self.identity.provider_id {
            return Err(Error::InvalidContract(
                "compute session capabilities provider_id does not match session provider_id"
                    .to_owned(),
            ));
        }
        if self.last_heartbeat_at < self.connected_at {
            return Err(Error::InvalidContract(
                "compute session last_heartbeat_at must not precede connected_at".to_owned(),
            ));
        }
        if self
            .last_healthy_heartbeat_at
            .is_some_and(|timestamp| timestamp < self.connected_at)
        {
            return Err(Error::InvalidContract(
                "compute session last_healthy_heartbeat_at must not precede connected_at"
                    .to_owned(),
            ));
        }
        if self
            .last_healthy_heartbeat_at
            .is_some_and(|timestamp| timestamp > self.last_heartbeat_at)
        {
            return Err(Error::InvalidContract(
                "compute session last_healthy_heartbeat_at must not exceed last_heartbeat_at"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeProviderLivenessPolicyV1 {
    pub stale_after_seconds: u64,
    pub lost_after_seconds: u64,
}

impl ComputeProviderLivenessPolicyV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.stale_after_seconds == 0 {
            return Err(Error::InvalidContract(
                "compute provider stale_after_seconds must be positive".to_owned(),
            ));
        }
        if self.lost_after_seconds <= self.stale_after_seconds {
            return Err(Error::InvalidContract(
                "compute provider lost_after_seconds must be greater than stale_after_seconds"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeProviderConnectV1 {
    pub session_id: String,
}

impl ComputeProviderConnectV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_identifier("compute connect session_id", &self.session_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputeProviderHeartbeatV1 {
    pub session_id: String,
    pub healthy: bool,
}

impl ComputeProviderHeartbeatV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_identifier("compute heartbeat session_id", &self.session_id)
    }
}

pub trait ComputeProvider {
    fn metadata(&self) -> ComputeProviderMetadataV1;

    fn connect(&mut self) -> Result<ComputeProviderConnectV1>;

    fn disconnect(&mut self, session_id: &str) -> Result<()>;

    fn heartbeat(&mut self, session_id: &str) -> Result<ComputeProviderHeartbeatV1>;

    fn discover_capabilities(&mut self, session_id: &str) -> Result<ComputeProviderCapabilitiesV1>;
}

pub struct ComputeProviderRuntime<P> {
    provider: P,
    metadata: ComputeProviderMetadataV1,
    policy: ComputeProviderLivenessPolicyV1,
    state: ComputeProviderConnectionState,
    session: Option<ComputeProviderSessionV1>,
}

impl<P> ComputeProviderRuntime<P>
where
    P: ComputeProvider,
{
    pub fn new(provider: P, policy: ComputeProviderLivenessPolicyV1) -> Result<Self> {
        let metadata = provider.metadata();
        metadata.validate_v1()?;
        policy.validate_v1()?;

        Ok(Self {
            provider,
            metadata,
            policy,
            state: ComputeProviderConnectionState::Disconnected,
            session: None,
        })
    }

    pub fn metadata(&self) -> &ComputeProviderMetadataV1 {
        &self.metadata
    }

    pub fn state(&self) -> ComputeProviderConnectionState {
        self.state
    }

    pub fn session(&self) -> Option<&ComputeProviderSessionV1> {
        self.session.as_ref()
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }

    pub fn into_provider(self) -> P {
        self.provider
    }

    pub fn connect(&mut self, now: DateTime<Utc>) -> Result<&ComputeProviderSessionV1> {
        if self.state != ComputeProviderConnectionState::Disconnected {
            return Err(Error::InvalidContract(format!(
                "compute provider connect requires DISCONNECTED state; found {}",
                self.state.as_str()
            )));
        }

        self.state = ComputeProviderConnectionState::Connecting;

        let connection = match self.provider.connect() {
            Ok(connection) => connection,
            Err(error) => {
                self.state = ComputeProviderConnectionState::Disconnected;
                return Err(error);
            }
        };

        let session = match self.finish_connect(connection, now) {
            Ok(session) => session,
            Err(error) => {
                self.state = ComputeProviderConnectionState::Disconnected;
                self.session = None;
                return Err(error);
            }
        };

        self.session = Some(session);
        self.state = ComputeProviderConnectionState::Ready;
        Ok(self.session.as_ref().expect("session was just initialized"))
    }

    pub fn disconnect(&mut self) -> Result<()> {
        match self.state {
            ComputeProviderConnectionState::Disconnected => return Ok(()),
            ComputeProviderConnectionState::Connecting => {
                return Err(Error::InvalidContract(
                    "cannot disconnect while compute provider connect is in progress".to_owned(),
                ));
            }
            ComputeProviderConnectionState::Lost => {
                self.session = None;
                self.state = ComputeProviderConnectionState::Disconnected;
                return Ok(());
            }
            ComputeProviderConnectionState::Ready
            | ComputeProviderConnectionState::Unhealthy
            | ComputeProviderConnectionState::Stale => {}
        }

        let session_id = self
            .session
            .as_ref()
            .ok_or_else(|| {
                Error::InvalidContract(
                    "connected compute provider state is missing session metadata".to_owned(),
                )
            })?
            .identity
            .session_id
            .clone();

        self.provider.disconnect(&session_id)?;
        self.session = None;
        self.state = ComputeProviderConnectionState::Disconnected;
        Ok(())
    }

    pub fn heartbeat(&mut self, now: DateTime<Utc>) -> Result<ComputeProviderConnectionState> {
        match self.state {
            ComputeProviderConnectionState::Ready
            | ComputeProviderConnectionState::Unhealthy
            | ComputeProviderConnectionState::Stale => {}
            ComputeProviderConnectionState::Disconnected
            | ComputeProviderConnectionState::Connecting
            | ComputeProviderConnectionState::Lost => {
                return Err(Error::InvalidContract(format!(
                    "compute provider heartbeat is unavailable in {} state",
                    self.state.as_str()
                )));
            }
        }

        let session_id = self
            .session
            .as_ref()
            .ok_or_else(|| {
                Error::InvalidContract(
                    "active compute provider state is missing session metadata".to_owned(),
                )
            })?
            .identity
            .session_id
            .clone();

        let heartbeat = self.provider.heartbeat(&session_id)?;
        heartbeat.validate_v1()?;
        if heartbeat.session_id != session_id {
            return Err(Error::InvalidContract(format!(
                "compute heartbeat session mismatch: expected {session_id}, found {}",
                heartbeat.session_id
            )));
        }

        let session = self.session.as_mut().expect("session validated above");
        if now < session.last_heartbeat_at {
            return Err(Error::InvalidContract(
                "compute heartbeat time must not move backwards".to_owned(),
            ));
        }

        session.last_heartbeat_at = now;
        if heartbeat.healthy {
            session.last_healthy_heartbeat_at = Some(now);
            self.state = ComputeProviderConnectionState::Ready;
        } else {
            self.state = ComputeProviderConnectionState::Unhealthy;
        }

        Ok(self.state)
    }

    pub fn refresh_liveness(
        &mut self,
        now: DateTime<Utc>,
    ) -> Result<ComputeProviderConnectionState> {
        match self.state {
            ComputeProviderConnectionState::Disconnected
            | ComputeProviderConnectionState::Connecting => return Ok(self.state),
            ComputeProviderConnectionState::Lost => {
                return Ok(ComputeProviderConnectionState::Lost)
            }
            ComputeProviderConnectionState::Ready
            | ComputeProviderConnectionState::Unhealthy
            | ComputeProviderConnectionState::Stale => {}
        }

        let session = self.session.as_ref().ok_or_else(|| {
            Error::InvalidContract(
                "active compute provider state is missing session metadata".to_owned(),
            )
        })?;
        if now < session.last_heartbeat_at {
            return Err(Error::InvalidContract(
                "compute liveness evaluation time must not precede last heartbeat".to_owned(),
            ));
        }

        let age_seconds = now
            .signed_duration_since(session.last_heartbeat_at)
            .num_seconds();
        let stale_after = i64::try_from(self.policy.stale_after_seconds).map_err(|_| {
            Error::InvalidContract("stale_after_seconds exceeds supported range".to_owned())
        })?;
        let lost_after = i64::try_from(self.policy.lost_after_seconds).map_err(|_| {
            Error::InvalidContract("lost_after_seconds exceeds supported range".to_owned())
        })?;

        if age_seconds >= lost_after {
            self.state = ComputeProviderConnectionState::Lost;
        } else if age_seconds >= stale_after {
            self.state = ComputeProviderConnectionState::Stale;
        }

        Ok(self.state)
    }

    pub fn refresh_capabilities(&mut self) -> Result<&ComputeProviderCapabilitiesV1> {
        match self.state {
            ComputeProviderConnectionState::Ready
            | ComputeProviderConnectionState::Unhealthy
            | ComputeProviderConnectionState::Stale => {}
            ComputeProviderConnectionState::Disconnected
            | ComputeProviderConnectionState::Connecting
            | ComputeProviderConnectionState::Lost => {
                return Err(Error::InvalidContract(format!(
                    "compute capability discovery is unavailable in {} state",
                    self.state.as_str()
                )));
            }
        }

        let session_id = self
            .session
            .as_ref()
            .ok_or_else(|| {
                Error::InvalidContract(
                    "active compute provider state is missing session metadata".to_owned(),
                )
            })?
            .identity
            .session_id
            .clone();
        let capabilities =
            self.validated_capabilities(self.provider.discover_capabilities(&session_id)?)?;

        let session = self.session.as_mut().expect("session validated above");
        session.capabilities = capabilities;
        Ok(&session.capabilities)
    }

    fn finish_connect(
        &mut self,
        connection: ComputeProviderConnectV1,
        now: DateTime<Utc>,
    ) -> Result<ComputeProviderSessionV1> {
        connection.validate_v1()?;

        let capabilities = self.validated_capabilities(
            self.provider
                .discover_capabilities(&connection.session_id)?,
        )?;
        let identity = ComputeProviderSessionIdentityV1 {
            provider_id: self.metadata.provider_id.clone(),
            session_id: connection.session_id,
        };
        identity.validate_v1()?;

        let session = ComputeProviderSessionV1 {
            identity,
            connected_at: now,
            last_heartbeat_at: now,
            last_healthy_heartbeat_at: Some(now),
            capabilities,
        };
        session.validate_v1()?;
        Ok(session)
    }

    fn validated_capabilities(
        &self,
        capabilities: ComputeProviderCapabilitiesV1,
    ) -> Result<ComputeProviderCapabilitiesV1> {
        let capabilities = capabilities.normalized_v1()?;
        if capabilities.provider_id != self.metadata.provider_id {
            return Err(Error::InvalidContract(format!(
                "compute capabilities provider_id mismatch: expected {}, found {}",
                self.metadata.provider_id, capabilities.provider_id
            )));
        }
        Ok(capabilities)
    }
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    if trimmed != value {
        return Err(Error::InvalidContract(format!(
            "{label} must not contain surrounding whitespace"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::InvalidContract(format!(
            "{label} must not contain control characters"
        )));
    }
    Ok(())
}

fn validate_optional_non_empty(label: &str, value: Option<&str>) -> Result<()> {
    if let Some(value) = value {
        validate_identifier(label, value)?;
    }
    Ok(())
}
