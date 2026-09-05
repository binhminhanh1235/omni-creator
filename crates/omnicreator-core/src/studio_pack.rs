use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const STUDIO_PACK_SCHEMA_V1: &str = "omnicreator.studio-pack";
pub const STUDIO_PACK_VERSION_V1: u32 = 1;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StudioAutomationLevelV1 {
    Assisted,
    #[default]
    Balanced,
    Autopilot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudioPackRouteTargetV1 {
    pub plugin_type: String,
    pub capability: String,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub preset: Option<String>,
}

impl StudioPackRouteTargetV1 {
    pub fn validate_v1(&self) -> Result<()> {
        require_portable_identifier_v1("studio pack plugin_type", &self.plugin_type)?;
        require_portable_identifier_v1("studio pack capability", &self.capability)?;
        if let Some(plugin_id) = self.plugin_id.as_deref() {
            require_portable_identifier_v1("studio pack plugin_id", plugin_id)?;
        }
        if let Some(preset) = self.preset.as_deref() {
            require_portable_identifier_v1("studio pack preset", preset)?;
        }
        Ok(())
    }

    fn stable_identity_v1(&self) -> String {
        format!(
            "{}\u{0}{}\u{0}{}\u{0}{}",
            self.plugin_type,
            self.capability,
            self.plugin_id.as_deref().unwrap_or_default(),
            self.preset.as_deref().unwrap_or_default()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudioPackRouteV1 {
    #[serde(default)]
    pub targets: Vec<StudioPackRouteTargetV1>,
}

impl StudioPackRouteV1 {
    pub fn validate_v1(&self) -> Result<()> {
        if self.targets.is_empty() {
            return Err(Error::InvalidContract(
                "studio pack route must contain at least one target".to_owned(),
            ));
        }

        let mut seen = BTreeSet::new();
        for target in &self.targets {
            target.validate_v1()?;
            if !seen.insert(target.stable_identity_v1()) {
                return Err(Error::InvalidContract(
                    "studio pack route contains a duplicate target".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudioPackOverridesV1 {
    #[serde(default)]
    pub automation_level: Option<StudioAutomationLevelV1>,
    #[serde(default)]
    pub routes: BTreeMap<String, StudioPackRouteV1>,
    #[serde(default)]
    pub presets: BTreeMap<String, String>,
    #[serde(default)]
    pub quality_thresholds: BTreeMap<String, u8>,
    #[serde(default)]
    pub remove_routes: BTreeSet<String>,
    #[serde(default)]
    pub remove_presets: BTreeSet<String>,
    #[serde(default)]
    pub remove_quality_thresholds: BTreeSet<String>,
}

impl StudioPackOverridesV1 {
    pub fn validate_v1(&self) -> Result<()> {
        for (route_key, route) in &self.routes {
            require_portable_identifier_v1("studio pack route key", route_key)?;
            route.validate_v1()?;
            if self.remove_routes.contains(route_key) {
                return Err(Error::InvalidContract(format!(
                    "studio pack route {route_key} cannot be both replaced and removed"
                )));
            }
        }
        for route_key in &self.remove_routes {
            require_portable_identifier_v1("studio pack removed route key", route_key)?;
        }

        for (preset_key, preset) in &self.presets {
            require_portable_identifier_v1("studio pack preset key", preset_key)?;
            require_portable_identifier_v1("studio pack preset value", preset)?;
            if self.remove_presets.contains(preset_key) {
                return Err(Error::InvalidContract(format!(
                    "studio pack preset {preset_key} cannot be both replaced and removed"
                )));
            }
        }
        for preset_key in &self.remove_presets {
            require_portable_identifier_v1("studio pack removed preset key", preset_key)?;
        }

        for (quality_key, threshold) in &self.quality_thresholds {
            require_portable_identifier_v1("studio pack quality key", quality_key)?;
            if *threshold > 100 {
                return Err(Error::InvalidContract(format!(
                    "studio pack quality threshold {quality_key} must be <= 100"
                )));
            }
            if self.remove_quality_thresholds.contains(quality_key) {
                return Err(Error::InvalidContract(format!(
                    "studio pack quality threshold {quality_key} cannot be both replaced and removed"
                )));
            }
        }
        for quality_key in &self.remove_quality_thresholds {
            require_portable_identifier_v1("studio pack removed quality key", quality_key)?;
        }

        Ok(())
    }

    fn apply_to_v1(&self, effective: &mut StudioPackEffectiveConfigV1) -> Result<()> {
        self.validate_v1()?;

        if let Some(level) = self.automation_level {
            effective.automation_level = level;
        }

        for key in &self.remove_routes {
            effective.routes.remove(key);
        }
        for (key, value) in &self.routes {
            effective.routes.insert(key.clone(), value.clone());
        }

        for key in &self.remove_presets {
            effective.presets.remove(key);
        }
        for (key, value) in &self.presets {
            effective.presets.insert(key.clone(), value.clone());
        }

        for key in &self.remove_quality_thresholds {
            effective.quality_thresholds.remove(key);
        }
        for (key, value) in &self.quality_thresholds {
            effective.quality_thresholds.insert(key.clone(), *value);
        }

        effective.validate_v1()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudioPackV1 {
    pub schema: String,
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub overrides: StudioPackOverridesV1,
}

impl StudioPackV1 {
    pub fn from_json_v1(raw: &str) -> Result<Self> {
        let pack: Self = serde_json::from_str(raw)?;
        pack.validate_v1()?;
        Ok(pack)
    }

    pub fn validate_v1(&self) -> Result<()> {
        validate_studio_pack_header_v1(&self.schema, self.schema_version)?;
        require_portable_identifier_v1("studio pack id", &self.id)?;
        require_display_name_v1(&self.name)?;
        if let Some(parent) = self.extends.as_deref() {
            require_portable_identifier_v1("studio pack extends", parent)?;
            if parent == self.id {
                return Err(Error::InvalidContract(format!(
                    "studio pack {} cannot extend itself",
                    self.id
                )));
            }
        }
        self.overrides.validate_v1()
    }

    pub fn canonical_json_v1(&self) -> Result<String> {
        self.validate_v1()?;
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StudioPackEffectiveConfigV1 {
    #[serde(default)]
    pub automation_level: StudioAutomationLevelV1,
    #[serde(default)]
    pub routes: BTreeMap<String, StudioPackRouteV1>,
    #[serde(default)]
    pub presets: BTreeMap<String, String>,
    #[serde(default)]
    pub quality_thresholds: BTreeMap<String, u8>,
}

impl StudioPackEffectiveConfigV1 {
    pub fn validate_v1(&self) -> Result<()> {
        for (route_key, route) in &self.routes {
            require_portable_identifier_v1("studio pack route key", route_key)?;
            route.validate_v1()?;
        }
        for (preset_key, preset) in &self.presets {
            require_portable_identifier_v1("studio pack preset key", preset_key)?;
            require_portable_identifier_v1("studio pack preset value", preset)?;
        }
        for (quality_key, threshold) in &self.quality_thresholds {
            require_portable_identifier_v1("studio pack quality key", quality_key)?;
            if *threshold > 100 {
                return Err(Error::InvalidContract(format!(
                    "studio pack quality threshold {quality_key} must be <= 100"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EffectiveStudioPackV1 {
    pub schema: String,
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub lineage: Vec<String>,
    pub config: StudioPackEffectiveConfigV1,
}

impl EffectiveStudioPackV1 {
    pub fn validate_v1(&self) -> Result<()> {
        validate_studio_pack_header_v1(&self.schema, self.schema_version)?;
        require_portable_identifier_v1("effective studio pack id", &self.id)?;
        require_display_name_v1(&self.name)?;
        if self.lineage.is_empty() || self.lineage.last() != Some(&self.id) {
            return Err(Error::InvalidContract(
                "effective studio pack lineage must end with the selected pack id".to_owned(),
            ));
        }
        for id in &self.lineage {
            require_portable_identifier_v1("effective studio pack lineage id", id)?;
        }
        self.config.validate_v1()
    }

    pub fn canonical_json_v1(&self) -> Result<String> {
        self.validate_v1()?;
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Debug, Clone, Default)]
pub struct StudioPackCatalogV1 {
    packs: BTreeMap<String, StudioPackV1>,
}

impl StudioPackCatalogV1 {
    pub fn from_packs_v1(packs: Vec<StudioPackV1>) -> Result<Self> {
        let mut catalog = Self::default();
        for pack in packs {
            pack.validate_v1()?;
            if catalog.packs.insert(pack.id.clone(), pack).is_some() {
                return Err(Error::InvalidContract(
                    "duplicate studio pack id in catalog".to_owned(),
                ));
            }
        }
        catalog.validate_v1()?;
        Ok(catalog)
    }

    pub fn get_v1(&self, id: &str) -> Option<&StudioPackV1> {
        self.packs.get(id)
    }

    pub fn ids_v1(&self) -> impl Iterator<Item = &str> {
        self.packs.keys().map(String::as_str)
    }

    pub fn validate_v1(&self) -> Result<()> {
        for id in self.packs.keys() {
            self.resolve_v1(id)?;
        }
        Ok(())
    }

    pub fn resolve_v1(&self, id: &str) -> Result<EffectiveStudioPackV1> {
        require_portable_identifier_v1("studio pack lookup id", id)?;
        let mut visiting = Vec::new();
        let (config, lineage) = self.resolve_config_v1(id, &mut visiting)?;
        let selected = self.packs.get(id).ok_or_else(|| {
            Error::InvalidContract(format!("studio pack parent or selection not found: {id}"))
        })?;
        let resolved = EffectiveStudioPackV1 {
            schema: STUDIO_PACK_SCHEMA_V1.to_owned(),
            schema_version: STUDIO_PACK_VERSION_V1,
            id: selected.id.clone(),
            name: selected.name.clone(),
            lineage,
            config,
        };
        resolved.validate_v1()?;
        Ok(resolved)
    }

    fn resolve_config_v1(
        &self,
        id: &str,
        visiting: &mut Vec<String>,
    ) -> Result<(StudioPackEffectiveConfigV1, Vec<String>)> {
        if let Some(position) = visiting.iter().position(|candidate| candidate == id) {
            let mut cycle = visiting[position..].to_vec();
            cycle.push(id.to_owned());
            return Err(Error::InvalidContract(format!(
                "studio pack inheritance cycle: {}",
                cycle.join(" -> ")
            )));
        }

        let pack = self.packs.get(id).ok_or_else(|| {
            Error::InvalidContract(format!("studio pack parent or selection not found: {id}"))
        })?;
        visiting.push(id.to_owned());

        let (mut config, mut lineage) = if let Some(parent) = pack.extends.as_deref() {
            self.resolve_config_v1(parent, visiting)?
        } else {
            (StudioPackEffectiveConfigV1::default(), Vec::new())
        };

        pack.overrides.apply_to_v1(&mut config)?;
        lineage.push(pack.id.clone());
        visiting.pop();

        Ok((config, lineage))
    }
}

fn validate_studio_pack_header_v1(schema: &str, schema_version: u32) -> Result<()> {
    if schema != STUDIO_PACK_SCHEMA_V1 {
        return Err(Error::InvalidContract(format!(
            "expected schema {STUDIO_PACK_SCHEMA_V1}, found {schema}"
        )));
    }
    if schema_version != STUDIO_PACK_VERSION_V1 {
        return Err(Error::InvalidContract(format!(
            "unsupported {STUDIO_PACK_SCHEMA_V1} schema version {schema_version}; expected {STUDIO_PACK_VERSION_V1}"
        )));
    }
    Ok(())
}

fn require_display_name_v1(value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(Error::InvalidContract(
            "studio pack name must not be empty".to_owned(),
        ));
    }
    if value.trim() != value || value.chars().any(char::is_control) {
        return Err(Error::InvalidContract(
            "studio pack name must be trimmed and contain no control characters".to_owned(),
        ));
    }
    Ok(())
}

fn require_portable_identifier_v1(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidContract(format!("{label} must not be empty")));
    }
    if value.trim() != value {
        return Err(Error::InvalidContract(format!(
            "{label} must not contain surrounding whitespace"
        )));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(Error::InvalidContract(format!(
            "{label} must be a portable identifier using only ASCII letters, digits, '.', '_' or '-'"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(
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

    fn route(targets: Vec<StudioPackRouteTargetV1>) -> StudioPackRouteV1 {
        StudioPackRouteV1 { targets }
    }

    fn pack(id: &str, name: &str, extends: Option<&str>) -> StudioPackV1 {
        StudioPackV1 {
            schema: STUDIO_PACK_SCHEMA_V1.to_owned(),
            schema_version: STUDIO_PACK_VERSION_V1,
            id: id.to_owned(),
            name: name.to_owned(),
            extends: extends.map(str::to_owned),
            overrides: StudioPackOverridesV1::default(),
        }
    }

    #[test]
    fn studio_pack_serialization_is_deterministic() {
        let mut first = pack("christian-cinematic", "Christian Cinematic", None);
        first
            .overrides
            .presets
            .insert("voice".to_owned(), "warm-narrator".to_owned());
        first
            .overrides
            .presets
            .insert("thumbnail".to_owned(), "cinematic".to_owned());
        first.overrides.routes.insert(
            "visual.literal".to_owned(),
            route(vec![
                target("visual", "local_asset", None, None),
                target("visual", "stock_video", None, None),
                target("visual", "generated_image", None, None),
            ]),
        );

        let mut second = pack("christian-cinematic", "Christian Cinematic", None);
        second.overrides.routes.insert(
            "visual.literal".to_owned(),
            route(vec![
                target("visual", "local_asset", None, None),
                target("visual", "stock_video", None, None),
                target("visual", "generated_image", None, None),
            ]),
        );
        second
            .overrides
            .presets
            .insert("thumbnail".to_owned(), "cinematic".to_owned());
        second
            .overrides
            .presets
            .insert("voice".to_owned(), "warm-narrator".to_owned());

        assert_eq!(
            first.canonical_json_v1().unwrap(),
            second.canonical_json_v1().unwrap()
        );
    }

    #[test]
    fn inheritance_replaces_removes_and_resolves_deterministically() {
        let mut base = pack("base", "Base", None);
        base.overrides.automation_level = Some(StudioAutomationLevelV1::Assisted);
        base.overrides.routes.insert(
            "visual.literal".to_owned(),
            route(vec![target("visual", "stock_video", None, None)]),
        );
        base.overrides.routes.insert(
            "visual.conceptual".to_owned(),
            route(vec![target("visual", "generated_image", None, None)]),
        );
        base.overrides
            .presets
            .insert("voice".to_owned(), "warm".to_owned());
        base.overrides
            .quality_thresholds
            .insert("visual".to_owned(), 80);

        let mut child = pack("child", "Child", Some("base"));
        child.overrides.automation_level = Some(StudioAutomationLevelV1::Autopilot);
        child.overrides.routes.insert(
            "visual.literal".to_owned(),
            route(vec![target(
                "visual",
                "generated_image",
                Some("generated-image"),
                Some("cinematic"),
            )]),
        );
        child
            .overrides
            .remove_routes
            .insert("visual.conceptual".to_owned());
        child
            .overrides
            .presets
            .insert("voice".to_owned(), "gentle".to_owned());
        child
            .overrides
            .remove_quality_thresholds
            .insert("visual".to_owned());

        let catalog = StudioPackCatalogV1::from_packs_v1(vec![child, base]).unwrap();
        let effective = catalog.resolve_v1("child").unwrap();

        assert_eq!(effective.lineage, vec!["base", "child"]);
        assert_eq!(
            effective.config.automation_level,
            StudioAutomationLevelV1::Autopilot
        );
        assert_eq!(effective.config.routes.len(), 1);
        assert_eq!(
            effective.config.routes["visual.literal"].targets[0]
                .plugin_id
                .as_deref(),
            Some("generated-image")
        );
        assert_eq!(effective.config.presets["voice"], "gentle");
        assert!(effective.config.quality_thresholds.is_empty());

        assert_eq!(
            effective.canonical_json_v1().unwrap(),
            catalog
                .resolve_v1("child")
                .unwrap()
                .canonical_json_v1()
                .unwrap()
        );
    }

    #[test]
    fn inheritance_cycle_and_missing_parent_are_rejected() {
        let a = pack("a", "A", Some("b"));
        let b = pack("b", "B", Some("a"));
        let cycle = StudioPackCatalogV1::from_packs_v1(vec![a, b]);
        assert!(
            matches!(cycle, Err(Error::InvalidContract(message)) if message.contains("a -> b -> a") || message.contains("b -> a -> b"))
        );

        let orphan = pack("orphan", "Orphan", Some("missing"));
        assert!(matches!(
            StudioPackCatalogV1::from_packs_v1(vec![orphan]),
            Err(Error::InvalidContract(message)) if message.contains("not found")
        ));
    }

    #[test]
    fn unknown_fields_and_unsupported_versions_are_explicitly_rejected() {
        let unknown = r#"{
            "schema":"omnicreator.studio-pack",
            "schema_version":1,
            "id":"pack",
            "name":"Pack",
            "model_id":"provider-specific-model"
        }"#;
        assert!(StudioPackV1::from_json_v1(unknown).is_err());

        let future = r#"{
            "schema":"omnicreator.studio-pack",
            "schema_version":2,
            "id":"pack",
            "name":"Pack"
        }"#;
        assert!(matches!(
            StudioPackV1::from_json_v1(future),
            Err(Error::InvalidContract(message)) if message.contains("unsupported")
        ));
    }

    #[test]
    fn missing_optional_v1_fields_use_backward_compatible_defaults() {
        let raw = r#"{
            "schema":"omnicreator.studio-pack",
            "schema_version":1,
            "id":"minimal",
            "name":"Minimal"
        }"#;
        let decoded = StudioPackV1::from_json_v1(raw).unwrap();

        assert_eq!(decoded.extends, None);
        assert_eq!(decoded.overrides, StudioPackOverridesV1::default());

        let catalog = StudioPackCatalogV1::from_packs_v1(vec![decoded]).unwrap();
        assert_eq!(
            catalog
                .resolve_v1("minimal")
                .unwrap()
                .config
                .automation_level,
            StudioAutomationLevelV1::Balanced
        );
    }

    #[test]
    fn portable_contract_rejects_path_like_and_provider_secret_fields() {
        let mut portable = pack("portable", "Portable", None);
        portable.overrides.routes.insert(
            "voice".to_owned(),
            route(vec![target(
                "voice",
                "tts",
                Some("omnivoice"),
                Some("warm-narrator"),
            )]),
        );
        portable.overrides.routes.insert(
            "export".to_owned(),
            route(vec![target("export", "timeline_export", None, None)]),
        );

        let json = portable.canonical_json_v1().unwrap();
        assert!(!json.contains("/Users/"));
        assert!(!json.contains("C:\\"));
        assert!(!json.contains("api_key"));
        assert!(!json.contains("endpoint"));
        assert!(!json.contains("model_id"));

        portable
            .overrides
            .presets
            .insert("voice".to_owned(), "/Users/alice/private-voice".to_owned());
        assert!(matches!(
            portable.validate_v1(),
            Err(Error::InvalidContract(_))
        ));

        let secret_field = r#"{
            "schema":"omnicreator.studio-pack",
            "schema_version":1,
            "id":"unsafe",
            "name":"Unsafe",
            "overrides":{"api_key":"secret-value"}
        }"#;
        assert!(StudioPackV1::from_json_v1(secret_field).is_err());
    }

    #[test]
    fn route_fallback_order_is_semantic_and_preserved() {
        let mut definition = pack("fallback", "Fallback", None);
        definition.overrides.routes.insert(
            "visual.literal".to_owned(),
            route(vec![
                target("visual", "local_asset", None, None),
                target("visual", "stock_video", Some("pexels"), None),
                target("visual", "generated_image", None, Some("cinematic")),
            ]),
        );

        let catalog = StudioPackCatalogV1::from_packs_v1(vec![definition]).unwrap();
        let effective = catalog.resolve_v1("fallback").unwrap();
        let capabilities = effective.config.routes["visual.literal"]
            .targets
            .iter()
            .map(|target| target.capability.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            capabilities,
            vec!["local_asset", "stock_video", "generated_image"]
        );
    }
}
