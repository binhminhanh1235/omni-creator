use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    EffectiveStudioPackV1, Error, PluginRegistry, Result, StudioAutomationLevelV1,
    StudioPackCatalogV1, StudioPackRouteTargetV1, StudioPackRouteV1, StudioPackV1,
    STUDIO_PACK_SCHEMA_V1, STUDIO_PACK_VERSION_V1,
};

pub const STUDIO_PACK_CATALOG_SCHEMA_V1: &str = "omnicreator.studio-pack-catalog";
pub const STUDIO_PACK_CATALOG_VERSION_V1: u32 = 1;
pub const STICK_FIGURE_VISUAL_CAPABILITY_V1: &str = "stick_figure_visual";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PortableStudioPackCatalogV1 {
    pub schema: String,
    pub schema_version: u32,
    pub packs: Vec<StudioPackV1>,
}

impl PortableStudioPackCatalogV1 {
    pub fn from_packs_v1(packs: Vec<StudioPackV1>) -> Result<Self> {
        let catalog = Self {
            schema: STUDIO_PACK_CATALOG_SCHEMA_V1.to_owned(),
            schema_version: STUDIO_PACK_CATALOG_VERSION_V1,
            packs,
        };
        catalog.validate_v1()?;
        Ok(catalog)
    }

    pub fn from_json_v1(raw: &str) -> Result<Self> {
        let catalog: Self = serde_json::from_str(raw)?;
        catalog.validate_v1()?;
        Ok(catalog)
    }

    pub fn validate_v1(&self) -> Result<()> {
        if self.schema != STUDIO_PACK_CATALOG_SCHEMA_V1 {
            return Err(Error::InvalidContract(format!(
                "expected schema {STUDIO_PACK_CATALOG_SCHEMA_V1}, found {}",
                self.schema
            )));
        }
        if self.schema_version != STUDIO_PACK_CATALOG_VERSION_V1 {
            return Err(Error::InvalidContract(format!(
                "unsupported {STUDIO_PACK_CATALOG_SCHEMA_V1} schema version {}; expected {STUDIO_PACK_CATALOG_VERSION_V1}",
                self.schema_version
            )));
        }
        self.runtime_catalog_v1().map(|_| ())
    }

    pub fn canonical_json_v1(&self) -> Result<String> {
        self.validate_v1()?;
        let mut canonical = self.clone();
        canonical
            .packs
            .sort_by(|left, right| left.id.cmp(&right.id));
        Ok(serde_json::to_string(&canonical)?)
    }

    pub fn list_definitions_v1(&self) -> Result<Vec<&StudioPackV1>> {
        self.validate_v1()?;
        let mut packs = self.packs.iter().collect::<Vec<_>>();
        packs.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(packs)
    }

    pub fn resolve_v1(&self, id: &str) -> Result<EffectiveStudioPackV1> {
        self.runtime_catalog_v1()?.resolve_v1(id)
    }

    pub fn inspect_requirements_v1(&self, id: &str) -> Result<Vec<StudioPackRouteRequirementV1>> {
        let effective = self.resolve_v1(id)?;
        Ok(effective
            .config
            .routes
            .into_iter()
            .map(|(route, route_config)| StudioPackRouteRequirementV1 {
                route,
                preferred: route_config.targets[0].clone(),
                fallbacks: route_config.targets[1..].to_vec(),
            })
            .collect())
    }

    pub fn evaluate_availability_v1(
        &self,
        id: &str,
        registry: &PluginRegistry,
        runtime: &StudioPackRuntimeSnapshotV1,
    ) -> Result<StudioPackAvailabilityV1> {
        let effective = self.resolve_v1(id)?;
        let mut status = StudioPackAvailabilityStatusV1::Available;
        let mut reasons = Vec::new();

        for (route_key, route) in &effective.config.routes {
            let route_result = evaluate_route_v1(route_key, route, registry, runtime);
            status = status.max(route_result.status);
            reasons.extend(route_result.reasons);
        }

        Ok(StudioPackAvailabilityV1 {
            pack_id: effective.id,
            status,
            reasons,
        })
    }

    fn runtime_catalog_v1(&self) -> Result<StudioPackCatalogV1> {
        StudioPackCatalogV1::from_packs_v1(self.packs.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudioPackRouteRequirementV1 {
    pub route: String,
    pub preferred: StudioPackRouteTargetV1,
    #[serde(default)]
    pub fallbacks: Vec<StudioPackRouteTargetV1>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StudioPackAvailabilityStatusV1 {
    Available,
    AvailableWithSetup,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StudioPackAvailabilityReasonCodeV1 {
    RequiredCapabilityMissing,
    PreferredPluginMissing,
    PluginUnavailable,
    SetupRequired,
    OptionalFallbackUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudioPackAvailabilityReasonV1 {
    pub code: StudioPackAvailabilityReasonCodeV1,
    pub route: String,
    pub plugin_type: String,
    pub capability: String,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub runtime_reason: Option<String>,
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudioPackAvailabilityV1 {
    pub pack_id: String,
    pub status: StudioPackAvailabilityStatusV1,
    #[serde(default)]
    pub reasons: Vec<StudioPackAvailabilityReasonV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRuntimeReadinessV1 {
    Ready,
    SetupRequired { reason_code: String },
    Unavailable { reason_code: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StudioPackRuntimeSnapshotV1 {
    plugins: BTreeMap<String, PluginRuntimeReadinessV1>,
}

impl StudioPackRuntimeSnapshotV1 {
    pub fn set_v1(&mut self, plugin_id: impl Into<String>, readiness: PluginRuntimeReadinessV1) {
        self.plugins.insert(plugin_id.into(), readiness);
    }

    pub fn get_v1(&self, plugin_id: &str) -> PluginRuntimeReadinessV1 {
        self.plugins
            .get(plugin_id)
            .cloned()
            .unwrap_or(PluginRuntimeReadinessV1::Ready)
    }
}

#[derive(Debug)]
struct RouteAvailabilityV1 {
    status: StudioPackAvailabilityStatusV1,
    reasons: Vec<StudioPackAvailabilityReasonV1>,
}

fn evaluate_route_v1(
    route_key: &str,
    route: &StudioPackRouteV1,
    registry: &PluginRegistry,
    runtime: &StudioPackRuntimeSnapshotV1,
) -> RouteAvailabilityV1 {
    let mut any_installed = false;
    let mut any_ready = false;
    let mut any_setup_required = false;
    let mut reasons = Vec::new();

    for (index, target) in route.targets.iter().enumerate() {
        let matching = matching_plugin_ids_v1(registry, target);

        if index == 0 && target.plugin_id.is_some() && matching.is_empty() {
            reasons.push(reason_for_target_v1(
                StudioPackAvailabilityReasonCodeV1::PreferredPluginMissing,
                route_key,
                target,
                None,
                false,
            ));
        } else if index > 0 && matching.is_empty() {
            reasons.push(reason_for_target_v1(
                StudioPackAvailabilityReasonCodeV1::OptionalFallbackUnavailable,
                route_key,
                target,
                None,
                false,
            ));
        }

        if !matching.is_empty() {
            any_installed = true;
        }

        for plugin_id in matching {
            match runtime.get_v1(&plugin_id) {
                PluginRuntimeReadinessV1::Ready => {
                    any_ready = true;
                }
                PluginRuntimeReadinessV1::SetupRequired { reason_code } => {
                    any_setup_required = true;
                    reasons.push(reason_for_target_v1(
                        StudioPackAvailabilityReasonCodeV1::SetupRequired,
                        route_key,
                        target,
                        Some((&plugin_id, &reason_code)),
                        false,
                    ));
                }
                PluginRuntimeReadinessV1::Unavailable { reason_code } => {
                    reasons.push(reason_for_target_v1(
                        StudioPackAvailabilityReasonCodeV1::PluginUnavailable,
                        route_key,
                        target,
                        Some((&plugin_id, &reason_code)),
                        false,
                    ));
                }
            }
        }
    }

    if any_ready {
        return RouteAvailabilityV1 {
            status: StudioPackAvailabilityStatusV1::Available,
            reasons,
        };
    }

    if any_setup_required {
        for reason in &mut reasons {
            if reason.code == StudioPackAvailabilityReasonCodeV1::SetupRequired {
                reason.blocking = true;
            }
        }
        return RouteAvailabilityV1 {
            status: StudioPackAvailabilityStatusV1::AvailableWithSetup,
            reasons,
        };
    }

    if any_installed {
        for reason in &mut reasons {
            if reason.code == StudioPackAvailabilityReasonCodeV1::PluginUnavailable {
                reason.blocking = true;
            }
        }
        return RouteAvailabilityV1 {
            status: StudioPackAvailabilityStatusV1::Unavailable,
            reasons,
        };
    }

    let required = &route.targets[0];
    reasons.push(reason_for_target_v1(
        StudioPackAvailabilityReasonCodeV1::RequiredCapabilityMissing,
        route_key,
        required,
        None,
        true,
    ));

    RouteAvailabilityV1 {
        status: StudioPackAvailabilityStatusV1::Unavailable,
        reasons,
    }
}

fn matching_plugin_ids_v1(
    registry: &PluginRegistry,
    target: &StudioPackRouteTargetV1,
) -> Vec<String> {
    registry
        .plugins()
        .filter(|plugin| {
            let manifest = &plugin.manifest;
            let type_match = manifest
                .types
                .iter()
                .any(|plugin_type| plugin_type.trim() == target.plugin_type);
            let capability_match = manifest
                .capabilities
                .iter()
                .any(|capability| capability.trim() == target.capability);
            let id_match = match target.plugin_id.as_deref() {
                Some(plugin_id) => plugin_id == manifest.id,
                None => true,
            };
            type_match && capability_match && id_match
        })
        .map(|plugin| plugin.manifest.id.clone())
        .collect()
}

fn reason_for_target_v1(
    code: StudioPackAvailabilityReasonCodeV1,
    route: &str,
    target: &StudioPackRouteTargetV1,
    runtime: Option<(&str, &str)>,
    blocking: bool,
) -> StudioPackAvailabilityReasonV1 {
    StudioPackAvailabilityReasonV1 {
        code,
        route: route.to_owned(),
        plugin_type: target.plugin_type.clone(),
        capability: target.capability.clone(),
        plugin_id: runtime
            .map(|(plugin_id, _)| plugin_id.to_owned())
            .or_else(|| target.plugin_id.clone()),
        runtime_reason: runtime.map(|(_, reason)| reason.to_owned()),
        blocking,
    }
}

pub fn initial_studio_pack_catalog_v1() -> Result<PortableStudioPackCatalogV1> {
    PortableStudioPackCatalogV1::from_packs_v1(vec![
        christian_cinematic_v1(),
        bible_illustrated_v1(),
        night_devotional_v1(),
        sleep_scripture_v1(),
        christian_stick_explainer_v1(),
    ])
}

fn christian_cinematic_v1() -> StudioPackV1 {
    let mut pack = pack_v1("christian-cinematic", "Christian Cinematic", None);
    pack.overrides.automation_level = Some(StudioAutomationLevelV1::Balanced);
    pack.overrides.routes.insert(
        "visual.literal".to_owned(),
        route_v1(vec![
            target_v1("visual", "stock_video", Some("pexels"), None),
            target_v1("visual", "generated_still", None, Some("cinematic")),
        ]),
    );
    pack.overrides.routes.insert(
        "visual.emotional".to_owned(),
        route_v1(vec![
            target_v1("visual", "stock_video", Some("pexels"), None),
            target_v1("visual", "generated_still", None, Some("cinematic")),
        ]),
    );
    pack.overrides.routes.insert(
        "visual.conceptual".to_owned(),
        route_v1(vec![
            target_v1("visual", "generated_still", None, Some("cinematic")),
            target_v1("visual", "stock_image", Some("pexels"), None),
        ]),
    );
    pack.overrides
        .presets
        .insert("visual_style".to_owned(), "cinematic".to_owned());
    pack.overrides
        .presets
        .insert("thumbnail".to_owned(), "cinematic".to_owned());
    pack.overrides
        .quality_thresholds
        .insert("visual".to_owned(), 80);
    pack
}

fn bible_illustrated_v1() -> StudioPackV1 {
    let mut pack = pack_v1(
        "bible-illustrated",
        "Bible Illustrated",
        Some("christian-cinematic"),
    );
    for route_key in ["visual.literal", "visual.emotional", "visual.conceptual"] {
        pack.overrides.routes.insert(
            route_key.to_owned(),
            route_v1(vec![
                target_v1("visual", "generated_still", None, Some("bible-illustrated")),
                target_v1("visual", "stock_image", Some("pexels"), None),
            ]),
        );
    }
    pack.overrides
        .presets
        .insert("visual_style".to_owned(), "bible-illustrated".to_owned());
    pack.overrides
        .quality_thresholds
        .insert("visual".to_owned(), 84);
    pack
}

fn night_devotional_v1() -> StudioPackV1 {
    let mut pack = pack_v1(
        "night-devotional",
        "Night Devotional",
        Some("christian-cinematic"),
    );
    for route_key in ["visual.emotional", "visual.conceptual"] {
        pack.overrides.routes.insert(
            route_key.to_owned(),
            route_v1(vec![
                target_v1("visual", "generated_still", None, Some("night-devotional")),
                target_v1("visual", "stock_video", Some("pexels"), None),
            ]),
        );
    }
    pack.overrides
        .presets
        .insert("visual_style".to_owned(), "night-devotional".to_owned());
    pack.overrides
        .quality_thresholds
        .insert("visual".to_owned(), 82);
    pack
}

fn sleep_scripture_v1() -> StudioPackV1 {
    let mut pack = pack_v1(
        "sleep-scripture",
        "Sleep Scripture",
        Some("night-devotional"),
    );
    pack.overrides.automation_level = Some(StudioAutomationLevelV1::Autopilot);
    pack.overrides
        .presets
        .insert("visual_style".to_owned(), "sleep-scripture".to_owned());
    pack.overrides
        .presets
        .insert("pacing".to_owned(), "sleep-scripture".to_owned());
    pack.overrides
        .quality_thresholds
        .insert("visual".to_owned(), 78);
    pack
}

fn christian_stick_explainer_v1() -> StudioPackV1 {
    let mut pack = pack_v1(
        "christian-stick-explainer",
        "Christian Stick Explainer",
        Some("bible-illustrated"),
    );
    pack.overrides.routes.insert(
        "visual.conceptual".to_owned(),
        route_v1(vec![target_v1(
            "visual",
            STICK_FIGURE_VISUAL_CAPABILITY_V1,
            None,
            Some("christian-stick-explainer"),
        )]),
    );
    pack.overrides.presets.insert(
        "visual_style".to_owned(),
        "christian-stick-explainer".to_owned(),
    );
    pack
}

fn pack_v1(id: &str, name: &str, extends: Option<&str>) -> StudioPackV1 {
    StudioPackV1 {
        schema: STUDIO_PACK_SCHEMA_V1.to_owned(),
        schema_version: STUDIO_PACK_VERSION_V1,
        id: id.to_owned(),
        name: name.to_owned(),
        extends: extends.map(str::to_owned),
        overrides: Default::default(),
    }
}

fn route_v1(targets: Vec<StudioPackRouteTargetV1>) -> StudioPackRouteV1 {
    StudioPackRouteV1 { targets }
}

fn target_v1(
    plugin_type: &str,
    capability: &str,
    plugin_id: Option<&str>,
    preset: Option<&str>,
) -> StudioPackRouteTargetV1 {
    StudioPackRouteTargetV1 {
        plugin_type: plugin_type.to_owned(),
        capability: capability.to_owned(),
        plugin_id: plugin_id.map(str::to_owned),
        preset: preset.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::{
        scan_plugin_roots, PLUGIN_API_VERSION, PLUGIN_MANIFEST_SCHEMA,
        PLUGIN_MANIFEST_SCHEMA_VERSION,
    };

    fn checked_in_registry_v1() -> PluginRegistry {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
        let report = scan_plugin_roots(&[root]);
        assert!(
            report.diagnostics.is_empty(),
            "checked-in plugin manifests must scan cleanly: {:?}",
            report.diagnostics
        );
        report.registry
    }

    fn write_plugin_v1(
        root: &Path,
        directory: &str,
        id: &str,
        plugin_type: &str,
        capabilities: &[&str],
    ) {
        let plugin_dir = root.join(directory);
        fs::create_dir_all(&plugin_dir).unwrap();
        let manifest = json!({
            "schema": PLUGIN_MANIFEST_SCHEMA,
            "schema_version": PLUGIN_MANIFEST_SCHEMA_VERSION,
            "id": id,
            "name": format!("{id} Plugin"),
            "version": "1.0.0",
            "api_version": PLUGIN_API_VERSION,
            "types": [plugin_type],
            "entrypoint": {"command": "plugin-bin", "args": []},
            "capabilities": capabilities,
            "scene_types": [],
            "permissions": {"filesystem": ["job-workspace"], "network": []},
            "settings": null
        });
        fs::write(
            plugin_dir.join("plugin.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }

    fn registry_with_v1(specs: &[(&str, &str, &[&str])], reverse_roots: bool) -> PluginRegistry {
        let temp = tempdir().unwrap();
        let root_a = temp.path().join("plugins-a");
        let root_b = temp.path().join("plugins-b");
        fs::create_dir_all(&root_a).unwrap();
        fs::create_dir_all(&root_b).unwrap();

        for (index, (id, plugin_type, capabilities)) in specs.iter().enumerate() {
            let root = if index % 2 == 0 { &root_a } else { &root_b };
            write_plugin_v1(root, id, id, plugin_type, capabilities);
        }

        let roots = if reverse_roots {
            vec![root_b, root_a]
        } else {
            vec![root_a, root_b]
        };
        let report = scan_plugin_roots(&roots);
        assert!(report.diagnostics.is_empty());
        report.registry
    }

    #[test]
    fn initial_catalog_and_serialization_are_deterministic() {
        let first = initial_studio_pack_catalog_v1().unwrap();
        let mut reversed = first.packs.clone();
        reversed.reverse();
        let second = PortableStudioPackCatalogV1::from_packs_v1(reversed).unwrap();

        assert_eq!(
            first
                .list_definitions_v1()
                .unwrap()
                .iter()
                .map(|pack| pack.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "bible-illustrated",
                "christian-cinematic",
                "christian-stick-explainer",
                "night-devotional",
                "sleep-scripture"
            ]
        );
        assert_eq!(
            first.canonical_json_v1().unwrap(),
            second.canonical_json_v1().unwrap()
        );
    }

    #[test]
    fn real_initial_pack_inheritance_resolves_expected_lineage() {
        let catalog = initial_studio_pack_catalog_v1().unwrap();

        let illustrated = catalog.resolve_v1("bible-illustrated").unwrap();
        assert_eq!(
            illustrated.lineage,
            vec!["christian-cinematic", "bible-illustrated"]
        );
        assert_eq!(
            illustrated.config.routes["visual.literal"].targets[0].capability,
            "generated_still"
        );

        let sleep = catalog.resolve_v1("sleep-scripture").unwrap();
        assert_eq!(
            sleep.lineage,
            vec!["christian-cinematic", "night-devotional", "sleep-scripture"]
        );
        assert_eq!(
            sleep.config.automation_level,
            StudioAutomationLevelV1::Autopilot
        );
        assert_eq!(sleep.config.presets["visual_style"], "sleep-scripture");
    }

    #[test]
    fn portable_catalog_contains_no_provider_transport_or_secret_fields() {
        let json = initial_studio_pack_catalog_v1()
            .unwrap()
            .canonical_json_v1()
            .unwrap();

        for forbidden in [
            "/Users/", "/home/", "C:\\", "api_key", "secret", "endpoint", "model_id", "base_url",
        ] {
            assert!(
                !json.contains(forbidden),
                "found forbidden token: {forbidden}"
            );
        }
    }

    #[test]
    fn initial_usable_packs_match_capabilities_in_checked_in_registry() {
        let catalog = initial_studio_pack_catalog_v1().unwrap();
        let registry = checked_in_registry_v1();
        let runtime = StudioPackRuntimeSnapshotV1::default();

        assert!(registry.get("pexels").is_some());
        assert!(registry.get("generated-image-reference").is_some());
        assert!(registry
            .plugin_ids_for_capability(STICK_FIGURE_VISUAL_CAPABILITY_V1)
            .is_empty());

        for pack_id in [
            "christian-cinematic",
            "bible-illustrated",
            "night-devotional",
            "sleep-scripture",
        ] {
            assert_eq!(
                catalog
                    .evaluate_availability_v1(pack_id, &registry, &runtime)
                    .unwrap()
                    .status,
                StudioPackAvailabilityStatusV1::Available
            );
        }
    }

    #[test]
    fn missing_required_capability_is_unavailable_not_corrupt() {
        let catalog = initial_studio_pack_catalog_v1().unwrap();
        let registry = registry_with_v1(&[], false);
        let availability = catalog
            .evaluate_availability_v1(
                "christian-cinematic",
                &registry,
                &StudioPackRuntimeSnapshotV1::default(),
            )
            .unwrap();

        assert_eq!(
            availability.status,
            StudioPackAvailabilityStatusV1::Unavailable
        );
        assert!(availability.reasons.iter().any(|reason| {
            reason.code == StudioPackAvailabilityReasonCodeV1::RequiredCapabilityMissing
                && reason.blocking
        }));
        assert!(catalog.resolve_v1("christian-cinematic").is_ok());
    }

    #[test]
    fn missing_preferred_plugin_uses_compatible_fallback() {
        let generated = ["generated_still"];
        let registry =
            registry_with_v1(&[("generated-only", "visual", generated.as_slice())], false);
        let catalog = initial_studio_pack_catalog_v1().unwrap();
        let availability = catalog
            .evaluate_availability_v1(
                "christian-cinematic",
                &registry,
                &StudioPackRuntimeSnapshotV1::default(),
            )
            .unwrap();

        assert_eq!(
            availability.status,
            StudioPackAvailabilityStatusV1::Available
        );
        assert!(availability.reasons.iter().any(|reason| {
            reason.code == StudioPackAvailabilityReasonCodeV1::PreferredPluginMissing
                && !reason.blocking
        }));
    }

    #[test]
    fn setup_required_is_distinct_from_missing_capability() {
        let generated = ["generated_still"];
        let registry = registry_with_v1(
            &[("image-needs-setup", "visual", generated.as_slice())],
            false,
        );
        let mut runtime = StudioPackRuntimeSnapshotV1::default();
        runtime.set_v1(
            "image-needs-setup",
            PluginRuntimeReadinessV1::SetupRequired {
                reason_code: "credential_required".to_owned(),
            },
        );

        let availability = initial_studio_pack_catalog_v1()
            .unwrap()
            .evaluate_availability_v1("bible-illustrated", &registry, &runtime)
            .unwrap();

        assert_eq!(
            availability.status,
            StudioPackAvailabilityStatusV1::AvailableWithSetup
        );
        assert!(availability.reasons.iter().any(|reason| {
            reason.code == StudioPackAvailabilityReasonCodeV1::SetupRequired
                && reason.runtime_reason.as_deref() == Some("credential_required")
                && reason.blocking
        }));
        assert!(!availability.reasons.iter().any(|reason| {
            reason.code == StudioPackAvailabilityReasonCodeV1::RequiredCapabilityMissing
        }));
    }

    #[test]
    fn stick_explainer_is_blocked_without_stick_capability() {
        let catalog = initial_studio_pack_catalog_v1().unwrap();
        let registry = checked_in_registry_v1();
        let availability = catalog
            .evaluate_availability_v1(
                "christian-stick-explainer",
                &registry,
                &StudioPackRuntimeSnapshotV1::default(),
            )
            .unwrap();

        assert_eq!(
            availability.status,
            StudioPackAvailabilityStatusV1::Unavailable
        );
        assert!(availability.reasons.iter().any(|reason| {
            reason.route == "visual.conceptual"
                && reason.capability == STICK_FIGURE_VISUAL_CAPABILITY_V1
                && reason.code == StudioPackAvailabilityReasonCodeV1::RequiredCapabilityMissing
                && reason.blocking
        }));
    }

    #[test]
    fn compatible_future_stick_capability_unlocks_pack_without_definition_change() {
        let catalog = initial_studio_pack_catalog_v1().unwrap();
        let before = catalog.canonical_json_v1().unwrap();
        let generated = ["generated_still"];
        let stock = ["stock_image", "stock_video"];
        let stick = [STICK_FIGURE_VISUAL_CAPABILITY_V1];
        let registry = registry_with_v1(
            &[
                ("generated", "visual", generated.as_slice()),
                ("pexels", "visual", stock.as_slice()),
                ("future-stick", "visual", stick.as_slice()),
            ],
            true,
        );

        let availability = catalog
            .evaluate_availability_v1(
                "christian-stick-explainer",
                &registry,
                &StudioPackRuntimeSnapshotV1::default(),
            )
            .unwrap();

        assert_eq!(
            availability.status,
            StudioPackAvailabilityStatusV1::Available
        );
        assert_eq!(before, catalog.canonical_json_v1().unwrap());
    }

    #[test]
    fn availability_is_independent_of_plugin_root_discovery_order() {
        let generated = ["generated_still"];
        let stock = ["stock_image", "stock_video"];
        let specs = [
            ("generated", "visual", generated.as_slice()),
            ("pexels", "visual", stock.as_slice()),
        ];
        let first = registry_with_v1(&specs, false);
        let second = registry_with_v1(&specs, true);
        let catalog = initial_studio_pack_catalog_v1().unwrap();
        let runtime = StudioPackRuntimeSnapshotV1::default();

        assert_eq!(
            catalog
                .evaluate_availability_v1("christian-cinematic", &first, &runtime)
                .unwrap(),
            catalog
                .evaluate_availability_v1("christian-cinematic", &second, &runtime)
                .unwrap()
        );
    }

    #[test]
    fn catalog_version_and_unknown_fields_are_rejected() {
        let future =
            r#"{"schema":"omnicreator.studio-pack-catalog","schema_version":2,"packs":[]}"#;
        assert!(matches!(
            PortableStudioPackCatalogV1::from_json_v1(future),
            Err(Error::InvalidContract(message)) if message.contains("unsupported")
        ));

        let unknown = r#"{"schema":"omnicreator.studio-pack-catalog","schema_version":1,"packs":[],"endpoint":"https://example.invalid"}"#;
        assert!(PortableStudioPackCatalogV1::from_json_v1(unknown).is_err());
    }
}
