use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use omnicreator_core::{
    ArtifactIrV1, AssetV1, AttemptIrV1, ComputeProviderCapabilitiesV1, JobIrV1,
    PluginManifest, PluginProgressEvent, PluginRequest, PluginResponse, ProjectIrV1, SceneIntentV1,
};

fn assert_fixture<T>(
    raw: &str,
    validate: impl Fn(&T) -> omnicreator_core::Result<()>,
) where
    T: DeserializeOwned + Serialize,
{
    let expected: Value = serde_json::from_str(raw).unwrap();
    let decoded: T = serde_json::from_str(raw).unwrap();
    validate(&decoded).unwrap();

    let first = serde_json::to_string(&decoded).unwrap();
    let second = serde_json::to_string(&decoded).unwrap();
    assert_eq!(first, second, "serialization must be deterministic");

    let actual: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(actual, expected, "v1 fixture semantics changed");
}

#[test]
fn durable_ir_v1_fixtures_are_compatible() {
    assert_fixture::<ProjectIrV1>(
        include_str!("fixtures/contracts/v1/project-ir.json"),
        ProjectIrV1::validate_v1,
    );
    assert_fixture::<SceneIntentV1>(
        include_str!("fixtures/contracts/v1/scene-intent.json"),
        SceneIntentV1::validate_v1,
    );
    assert_fixture::<AssetV1>(
        include_str!("fixtures/contracts/v1/asset.json"),
        AssetV1::validate_v1,
    );
    assert_fixture::<ArtifactIrV1>(
        include_str!("fixtures/contracts/v1/artifact.json"),
        ArtifactIrV1::validate_v1,
    );
    assert_fixture::<JobIrV1>(
        include_str!("fixtures/contracts/v1/job.json"),
        JobIrV1::validate_v1,
    );
    assert_fixture::<AttemptIrV1>(
        include_str!("fixtures/contracts/v1/attempt.json"),
        AttemptIrV1::validate_v1,
    );
    assert_fixture::<ComputeProviderCapabilitiesV1>(
        include_str!("fixtures/contracts/v1/compute-capabilities.json"),
        ComputeProviderCapabilitiesV1::validate_v1,
    );
}

#[test]
fn plugin_api_v1_fixtures_are_compatible() {
    assert_fixture::<PluginManifest>(
        include_str!("fixtures/contracts/v1/plugin-manifest.json"),
        PluginManifest::validate_v1,
    );
    assert_fixture::<PluginRequest>(
        include_str!("fixtures/contracts/v1/plugin-request.json"),
        PluginRequest::validate_v1,
    );
    assert_fixture::<PluginResponse>(
        include_str!("fixtures/contracts/v1/plugin-response.json"),
        PluginResponse::validate_v1,
    );
    assert_fixture::<PluginProgressEvent>(
        include_str!("fixtures/contracts/v1/plugin-progress.json"),
        PluginProgressEvent::validate_v1,
    );
}

#[test]
fn canonical_fixtures_do_not_embed_machine_absolute_paths() {
    let fixtures = [
        include_str!("fixtures/contracts/v1/project-ir.json"),
        include_str!("fixtures/contracts/v1/scene-intent.json"),
        include_str!("fixtures/contracts/v1/asset.json"),
        include_str!("fixtures/contracts/v1/artifact.json"),
        include_str!("fixtures/contracts/v1/job.json"),
        include_str!("fixtures/contracts/v1/attempt.json"),
        include_str!("fixtures/contracts/v1/compute-capabilities.json"),
        include_str!("fixtures/contracts/v1/plugin-manifest.json"),
    ];

    for fixture in fixtures {
        assert!(!fixture.contains("/Users/"));
        assert!(!fixture.contains("/home/"));
        assert!(!fixture.contains("C:\\"));
    }
}
