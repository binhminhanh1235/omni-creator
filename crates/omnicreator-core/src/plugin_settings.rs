use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

use serde::Serialize;
use serde_json::Value;

use crate::{DiscoveredPlugin, Error, Result};

pub const SETTINGS_VISIBILITY_KEY: &str = "x-omnicreator-visibility";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginSettingsLoadReport {
    pub ui: Option<PluginSettingsUi>,
    pub diagnostics: Vec<PluginSettingsDiagnostic>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginSettingsUi {
    pub plugin_id: String,
    pub schema_ref: String,
    pub title: String,
    pub description: Option<String>,
    pub fields: Vec<PluginSettingField>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PluginSettingField {
    pub key: String,
    pub label: String,
    pub description: Option<String>,
    pub setting_type: PluginSettingType,
    pub required: bool,
    pub visibility: PluginSettingVisibility,
    pub default: Option<Value>,
    pub choices: Vec<Value>,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginSettingType {
    String,
    Integer,
    Number,
    Boolean,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PluginSettingVisibility {
    Basic,
    Advanced,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum PluginSettingsDiagnosticCode {
    InvalidSchemaPath,
    SchemaNotFound,
    SchemaReadFailed,
    InvalidJson,
    InvalidRootSchema,
    InvalidProperty,
    UnsupportedPropertyType,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PluginSettingsDiagnostic {
    pub code: PluginSettingsDiagnosticCode,
    pub field: Option<String>,
    pub message: String,
}

pub fn load_plugin_settings_ui(plugin: &DiscoveredPlugin) -> PluginSettingsLoadReport {
    let Some(settings) = plugin.manifest.settings.as_ref() else {
        return PluginSettingsLoadReport::default();
    };

    let schema_ref = settings.schema.trim();
    if schema_ref.is_empty() {
        return report_error(
            PluginSettingsDiagnosticCode::InvalidSchemaPath,
            None,
            "settings.schema must not be empty",
        );
    }

    let schema_path = match resolve_settings_schema_path(&plugin.directory, schema_ref) {
        Ok(path) => path,
        Err(error) => {
            return report_error(
                PluginSettingsDiagnosticCode::InvalidSchemaPath,
                None,
                &error.to_string(),
            );
        }
    };

    let raw = match fs::read_to_string(&schema_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return report_error(
                PluginSettingsDiagnosticCode::SchemaNotFound,
                None,
                &format!("settings schema '{schema_ref}' was not found"),
            );
        }
        Err(error) => {
            return report_error(
                PluginSettingsDiagnosticCode::SchemaReadFailed,
                None,
                &format!("cannot read settings schema '{schema_ref}': {error}"),
            );
        }
    };

    let schema: Value = match serde_json::from_str(&raw) {
        Ok(schema) => schema,
        Err(error) => {
            return report_error(
                PluginSettingsDiagnosticCode::InvalidJson,
                None,
                &format!("invalid JSON in settings schema '{schema_ref}': {error}"),
            );
        }
    };

    build_settings_ui(&plugin.manifest.id, schema_ref, &schema)
}

fn build_settings_ui(
    plugin_id: &str,
    schema_ref: &str,
    schema: &Value,
) -> PluginSettingsLoadReport {
    let Some(root) = schema.as_object() else {
        return report_error(
            PluginSettingsDiagnosticCode::InvalidRootSchema,
            None,
            "settings schema root must be a JSON object",
        );
    };

    if root.get("type").and_then(Value::as_str) != Some("object") {
        return report_error(
            PluginSettingsDiagnosticCode::InvalidRootSchema,
            None,
            "settings schema root type must be 'object'",
        );
    }

    let title = root
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Plugin Settings")
        .to_owned();
    let description = optional_non_empty_string(root.get("description"));

    let required = match parse_required(root.get("required")) {
        Ok(required) => required,
        Err(message) => {
            return report_error(
                PluginSettingsDiagnosticCode::InvalidRootSchema,
                None,
                &message,
            );
        }
    };

    let properties = match root.get("properties") {
        None => None,
        Some(value) => match value.as_object() {
            Some(properties) => Some(properties),
            None => {
                return report_error(
                    PluginSettingsDiagnosticCode::InvalidRootSchema,
                    None,
                    "settings schema 'properties' must be an object",
                );
            }
        },
    };

    let mut diagnostics = Vec::new();
    let mut fields = Vec::new();

    if let Some(properties) = properties {
        let mut keys = properties.keys().cloned().collect::<Vec<_>>();
        keys.sort();

        for key in keys {
            match parse_field(&key, &properties[&key], required.contains(&key)) {
                Ok(field) => fields.push(field),
                Err((code, message)) => diagnostics.push(PluginSettingsDiagnostic {
                    code,
                    field: Some(key),
                    message,
                }),
            }
        }
    }

    diagnostics.sort_by(|left, right| {
        left.field
            .cmp(&right.field)
            .then_with(|| left.code.cmp(&right.code))
            .then_with(|| left.message.cmp(&right.message))
    });

    PluginSettingsLoadReport {
        ui: Some(PluginSettingsUi {
            plugin_id: plugin_id.to_owned(),
            schema_ref: schema_ref.to_owned(),
            title,
            description,
            fields,
        }),
        diagnostics,
    }
}

fn parse_field(
    key: &str,
    schema: &Value,
    required: bool,
) -> std::result::Result<PluginSettingField, (PluginSettingsDiagnosticCode, String)> {
    let property = schema.as_object().ok_or_else(|| {
        (
            PluginSettingsDiagnosticCode::InvalidProperty,
            "property schema must be an object".to_owned(),
        )
    })?;

    let type_name = property
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            (
                PluginSettingsDiagnosticCode::InvalidProperty,
                "property type is required".to_owned(),
            )
        })?;

    let setting_type = match type_name {
        "string" => PluginSettingType::String,
        "integer" => PluginSettingType::Integer,
        "number" => PluginSettingType::Number,
        "boolean" => PluginSettingType::Boolean,
        other => {
            return Err((
                PluginSettingsDiagnosticCode::UnsupportedPropertyType,
                format!("property type '{other}' is not supported by Plugin Runtime v1"),
            ));
        }
    };

    let visibility = match property
        .get(SETTINGS_VISIBILITY_KEY)
        .and_then(Value::as_str)
        .unwrap_or("basic")
    {
        "basic" => PluginSettingVisibility::Basic,
        "advanced" => PluginSettingVisibility::Advanced,
        other => {
            return Err((
                PluginSettingsDiagnosticCode::InvalidProperty,
                format!("{SETTINGS_VISIBILITY_KEY} must be 'basic' or 'advanced', found '{other}'"),
            ));
        }
    };

    let label = property
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| humanize_key(key));
    let description = optional_non_empty_string(property.get("description"));

    let choices = match property.get("enum") {
        None => Vec::new(),
        Some(Value::Array(values)) if !values.is_empty() => {
            for value in values {
                if !value_matches_type(value, setting_type) {
                    return Err((
                        PluginSettingsDiagnosticCode::InvalidProperty,
                        "every enum value must match the property type".to_owned(),
                    ));
                }
            }
            values.clone()
        }
        Some(_) => {
            return Err((
                PluginSettingsDiagnosticCode::InvalidProperty,
                "property enum must be a non-empty array".to_owned(),
            ));
        }
    };

    let default = property.get("default").cloned();
    if let Some(default) = &default {
        if !value_matches_type(default, setting_type) {
            return Err((
                PluginSettingsDiagnosticCode::InvalidProperty,
                "property default must match the property type".to_owned(),
            ));
        }
        if !choices.is_empty() && !choices.contains(default) {
            return Err((
                PluginSettingsDiagnosticCode::InvalidProperty,
                "property default must be one of the enum values".to_owned(),
            ));
        }
    }

    let minimum = parse_numeric_bound(property.get("minimum"), "minimum", setting_type)?;
    let maximum = parse_numeric_bound(property.get("maximum"), "maximum", setting_type)?;
    if minimum
        .zip(maximum)
        .is_some_and(|(minimum, maximum)| minimum > maximum)
    {
        return Err((
            PluginSettingsDiagnosticCode::InvalidProperty,
            "property minimum must not be greater than maximum".to_owned(),
        ));
    }

    if let Some(default_number) = default.as_ref().and_then(Value::as_f64) {
        if minimum.is_some_and(|minimum| default_number < minimum)
            || maximum.is_some_and(|maximum| default_number > maximum)
        {
            return Err((
                PluginSettingsDiagnosticCode::InvalidProperty,
                "numeric default must be within minimum/maximum bounds".to_owned(),
            ));
        }
    }

    Ok(PluginSettingField {
        key: key.to_owned(),
        label,
        description,
        setting_type,
        required,
        visibility,
        default,
        choices,
        minimum,
        maximum,
    })
}

fn resolve_settings_schema_path(plugin_directory: &Path, schema_ref: &str) -> Result<PathBuf> {
    validate_relative_schema_ref(schema_ref)?;
    let plugin_root = fs::canonicalize(plugin_directory)?;
    let candidate = plugin_root.join(schema_ref);
    let mut current = plugin_root.clone();

    for component in Path::new(schema_ref).components() {
        let Component::Normal(segment) = component else {
            return Err(Error::PathEscape(schema_ref.to_owned()));
        };
        current.push(segment);
        if current.exists() && fs::symlink_metadata(&current)?.file_type().is_symlink() {
            return Err(Error::PathEscape(current.display().to_string()));
        }
    }

    if !candidate.starts_with(&plugin_root) {
        return Err(Error::PathEscape(schema_ref.to_owned()));
    }
    Ok(candidate)
}

fn validate_relative_schema_ref(schema_ref: &str) -> Result<()> {
    if schema_ref.is_empty()
        || schema_ref.starts_with('/')
        || schema_ref.contains('\\')
        || schema_ref.contains('\0')
    {
        return Err(Error::PathEscape(schema_ref.to_owned()));
    }
    for component in Path::new(schema_ref).components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(Error::PathEscape(schema_ref.to_owned()));
        }
    }
    Ok(())
}

fn parse_required(value: Option<&Value>) -> std::result::Result<BTreeSet<String>, String> {
    let Some(value) = value else {
        return Ok(BTreeSet::new());
    };
    let Value::Array(values) = value else {
        return Err("settings schema 'required' must be an array of strings".to_owned());
    };

    let mut required = BTreeSet::new();
    for value in values {
        let Some(key) = value.as_str() else {
            return Err("settings schema 'required' must contain only strings".to_owned());
        };
        if key.trim().is_empty() {
            return Err("settings schema 'required' must not contain empty names".to_owned());
        }
        required.insert(key.to_owned());
    }
    Ok(required)
}

fn parse_numeric_bound(
    value: Option<&Value>,
    label: &str,
    setting_type: PluginSettingType,
) -> std::result::Result<Option<f64>, (PluginSettingsDiagnosticCode, String)> {
    let Some(value) = value else {
        return Ok(None);
    };

    if !matches!(
        setting_type,
        PluginSettingType::Integer | PluginSettingType::Number
    ) {
        return Err((
            PluginSettingsDiagnosticCode::InvalidProperty,
            format!("{label} is only valid for integer/number properties"),
        ));
    }

    let Some(number) = value.as_f64() else {
        return Err((
            PluginSettingsDiagnosticCode::InvalidProperty,
            format!("{label} must be numeric"),
        ));
    };
    if !number.is_finite() {
        return Err((
            PluginSettingsDiagnosticCode::InvalidProperty,
            format!("{label} must be finite"),
        ));
    }
    Ok(Some(number))
}

fn value_matches_type(value: &Value, setting_type: PluginSettingType) -> bool {
    match setting_type {
        PluginSettingType::String => value.is_string(),
        PluginSettingType::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
        PluginSettingType::Number => value.is_number(),
        PluginSettingType::Boolean => value.is_boolean(),
    }
}

fn optional_non_empty_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn humanize_key(key: &str) -> String {
    let words = key
        .split(['_', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>();

    if words.is_empty() {
        key.to_owned()
    } else {
        words.join(" ")
    }
}

fn report_error(
    code: PluginSettingsDiagnosticCode,
    field: Option<String>,
    message: &str,
) -> PluginSettingsLoadReport {
    PluginSettingsLoadReport {
        ui: None,
        diagnostics: vec![PluginSettingsDiagnostic {
            code,
            field,
            message: message.to_owned(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{
        PluginEntrypoint, PluginManifest, PluginPermissions, PluginSettings, PLUGIN_API_VERSION,
        PLUGIN_MANIFEST_SCHEMA, PLUGIN_MANIFEST_SCHEMA_VERSION,
    };
    use tempfile::tempdir;

    use super::*;

    fn plugin_with_schema(directory: &Path, schema_ref: Option<&str>) -> DiscoveredPlugin {
        DiscoveredPlugin {
            directory: directory.to_path_buf(),
            manifest_path: directory.join("plugin.json"),
            manifest: PluginManifest {
                schema: PLUGIN_MANIFEST_SCHEMA.to_owned(),
                schema_version: PLUGIN_MANIFEST_SCHEMA_VERSION,
                id: "fixture".to_owned(),
                name: "Fixture Plugin".to_owned(),
                version: "1.0.0".to_owned(),
                api_version: PLUGIN_API_VERSION,
                types: vec!["visual".to_owned()],
                entrypoint: PluginEntrypoint {
                    command: "fixture".to_owned(),
                    args: Vec::new(),
                },
                capabilities: Vec::new(),
                scene_types: Vec::new(),
                permissions: PluginPermissions::default(),
                settings: schema_ref.map(|schema| PluginSettings {
                    schema: schema.to_owned(),
                }),
                resources: None,
            },
        }
    }

    #[test]
    fn valid_schema_produces_deterministic_provider_neutral_ui() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("settings.schema.json"),
            r#"{
  "type": "object",
  "title": "Visual Settings",
  "description": "Fixture controls",
  "required": ["quality"],
  "properties": {
    "seed": {
      "type": "integer",
      "title": "Seed",
      "default": 42,
      "minimum": 0,
      "maximum": 100,
      "x-omnicreator-visibility": "advanced"
    },
    "quality": {
      "type": "string",
      "enum": ["fast", "best"],
      "default": "best"
    },
    "enabled": {
      "type": "boolean",
      "default": true
    }
  }
}"#,
        )
        .unwrap();

        let plugin = plugin_with_schema(&plugin_dir, Some("settings.schema.json"));
        let report = load_plugin_settings_ui(&plugin);

        assert!(report.diagnostics.is_empty());
        let ui = report.ui.unwrap();
        assert_eq!(ui.title, "Visual Settings");
        assert_eq!(
            ui.fields
                .iter()
                .map(|field| field.key.as_str())
                .collect::<Vec<_>>(),
            vec!["enabled", "quality", "seed"]
        );
        let quality = ui
            .fields
            .iter()
            .find(|field| field.key == "quality")
            .unwrap();
        assert!(quality.required);
        assert_eq!(quality.label, "Quality");
        assert_eq!(
            quality.choices,
            vec![Value::from("fast"), Value::from("best")]
        );

        let seed = ui.fields.iter().find(|field| field.key == "seed").unwrap();
        assert_eq!(seed.visibility, PluginSettingVisibility::Advanced);
        assert_eq!(seed.minimum, Some(0.0));
        assert_eq!(seed.maximum, Some(100.0));
    }

    #[test]
    fn malformed_schema_is_reported_without_panicking() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("settings.schema.json"), "{not-json").unwrap();

        let plugin = plugin_with_schema(&plugin_dir, Some("settings.schema.json"));
        let report = load_plugin_settings_ui(&plugin);

        assert!(report.ui.is_none());
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            PluginSettingsDiagnosticCode::InvalidJson
        );
    }

    #[test]
    fn unsupported_property_is_skipped_while_valid_fields_remain() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("settings.schema.json"),
            r#"{
  "type": "object",
  "properties": {
    "tags": {"type": "array"},
    "model": {"type": "string", "default": "default"}
  }
}"#,
        )
        .unwrap();

        let plugin = plugin_with_schema(&plugin_dir, Some("settings.schema.json"));
        let report = load_plugin_settings_ui(&plugin);

        let ui = report.ui.unwrap();
        assert_eq!(ui.fields.len(), 1);
        assert_eq!(ui.fields[0].key, "model");
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code,
            PluginSettingsDiagnosticCode::UnsupportedPropertyType
        );
        assert_eq!(report.diagnostics[0].field.as_deref(), Some("tags"));
    }

    #[test]
    fn invalid_visibility_or_default_is_a_field_diagnostic_not_runtime_failure() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("settings.schema.json"),
            r#"{
  "type": "object",
  "properties": {
    "bad_visibility": {
      "type": "string",
      "x-omnicreator-visibility": "expert"
    },
    "bad_default": {
      "type": "integer",
      "default": "not-an-integer"
    },
    "good": {
      "type": "number",
      "default": 0.5,
      "minimum": 0,
      "maximum": 1
    }
  }
}"#,
        )
        .unwrap();

        let plugin = plugin_with_schema(&plugin_dir, Some("settings.schema.json"));
        let report = load_plugin_settings_ui(&plugin);

        let ui = report.ui.unwrap();
        assert_eq!(ui.fields.len(), 1);
        assert_eq!(ui.fields[0].key, "good");
        assert_eq!(report.diagnostics.len(), 2);
    }

    #[test]
    fn schema_path_traversal_is_rejected() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(temp.path().join("outside.json"), r#"{"type":"object"}"#).unwrap();

        let plugin = plugin_with_schema(&plugin_dir, Some("../outside.json"));
        let report = load_plugin_settings_ui(&plugin);

        assert!(report.ui.is_none());
        assert_eq!(
            report.diagnostics[0].code,
            PluginSettingsDiagnosticCode::InvalidSchemaPath
        );
    }

    #[test]
    fn plugin_without_settings_schema_has_empty_report() {
        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        let plugin = plugin_with_schema(&plugin_dir, None);

        let report = load_plugin_settings_ui(&plugin);

        assert!(report.ui.is_none());
        assert!(report.diagnostics.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn schema_symlink_cannot_escape_plugin_directory() {
        use std::os::unix::fs::symlink;

        let temp = tempdir().unwrap();
        let plugin_dir = temp.path().join("plugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        let outside = temp.path().join("outside.json");
        fs::write(&outside, r#"{"type":"object"}"#).unwrap();
        symlink(&outside, plugin_dir.join("settings.schema.json")).unwrap();

        let plugin = plugin_with_schema(&plugin_dir, Some("settings.schema.json"));
        let report = load_plugin_settings_ui(&plugin);

        assert!(report.ui.is_none());
        assert_eq!(
            report.diagnostics[0].code,
            PluginSettingsDiagnosticCode::InvalidSchemaPath
        );
    }
}
