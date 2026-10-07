// Copyright (c) 2026 Contributors to the Bare-Metal-Mafia project
// See the NOTICE file(s) distributed with this work for additional
// information regarding copyright ownership.
//
// This program and the accompanying materials are made available under the
// terms of the Apache License, Version 2.0 which is available at
// https://www.apache.org/licenses/LICENSE-2.0
//
// AI Disclosure: This file was largely AI-generated. The AI-generated
// portions are made available under CC0-1.0 and not subject to the
// project's licence. The human contributor has reviewed and verified
// that the code is correct.
//
// SPDX-License-Identifier: Apache-2.0 AND CC0-1.0
// Assisted-by: Anthropic Claude Opus 5.5 (claude-opus-5-5)
// Assisted-by: Anthropic Claude Fable 5.1 (claude-fable-5-1)

//! Mapping file: loading, `${name.key}` constant substitution, validation.
//! The format is documented in ros-up-bridge/README.md ("Mapping format").

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_yaml::Value as Yaml;
use up_rust::UUri;

use crate::transform::FieldSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Gazebo follows the stack; nothing may be published on uProtocol.
    Mirror,
    /// Gazebo replaces simulators and publishes their uProtocol topics.
    Replace,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MappingFile {
    pub version: u32,
    pub mode: Mode,
    pub zenoh: ZenohCfg,
    /// Already applied by `load` (substitution); kept for validation.
    #[serde(default)]
    #[allow(dead_code)]
    pub constants: BTreeMap<String, ConstantSource>,
    #[serde(default)]
    pub links: Vec<LinkCfg>,
    #[serde(default)]
    pub routes: Vec<RouteCfg>,
    #[serde(default)]
    pub http: Option<HttpCfg>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZenohCfg {
    pub key_prefix: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstantSource {
    pub file: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkDirection {
    FromRos,
    ToRos,
}

/// ROS side of a link; only its name and direction matter to the mapper.
#[derive(Debug, Deserialize)]
pub struct LinkCfg {
    pub name: String,
    pub direction: LinkDirection,
    #[allow(dead_code)]
    pub ros: Yaml,
    #[serde(default)]
    #[allow(dead_code)]
    pub max_rate_hz: Option<f64>,
}

/// `{uprotocol: <uri>}` or `{link: <name>}`.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "EndpointRepr")]
pub enum EndpointCfg {
    Uprotocol(UriRef),
    Link(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EndpointRepr {
    #[serde(default)]
    uprotocol: Option<UriRef>,
    #[serde(default)]
    link: Option<String>,
}

impl TryFrom<EndpointRepr> for EndpointCfg {
    type Error = String;

    fn try_from(repr: EndpointRepr) -> Result<Self, Self::Error> {
        match (repr.uprotocol, repr.link) {
            (Some(uri), None) => Ok(EndpointCfg::Uprotocol(uri)),
            (None, Some(link)) => Ok(EndpointCfg::Link(link)),
            _ => Err("an endpoint is exactly one of {uprotocol: ...} or {link: ...}".into()),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum UriRef {
    /// Name of a URI function in services/src/lib.rs, e.g. `vss_window_state`.
    Named(String),
    Parts {
        authority: String,
        ue_id: u32,
        version: u8,
        resource: u16,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteCfg {
    /// Every input message is mapped field by field to one output message.
    Forward {
        name: String,
        from: EndpointCfg,
        to: EndpointCfg,
        fields: BTreeMap<String, FieldSpec>,
        /// Re-send the last output this often (links only): a lost or
        /// missed setpoint is then corrected instead of persisting.
        #[serde(default)]
        repeat_last_ms: Option<u64>,
    },
}

impl RouteCfg {
    pub fn name(&self) -> &str {
        match self {
            RouteCfg::Forward { name, .. } => name,
        }
    }

    pub fn outputs(&self) -> Vec<&EndpointCfg> {
        match self {
            RouteCfg::Forward { to, .. } => vec![to],
        }
    }

    pub fn inputs(&self) -> Vec<&EndpointCfg> {
        match self {
            RouteCfg::Forward { from, .. } => vec![from],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpCfg {
    pub port: u16,
}

pub type ConfigError = Box<dyn std::error::Error + Send + Sync>;

/// Load, substitute constants and validate a mapping file.
pub fn load(path: &Path) -> Result<MappingFile, ConfigError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read mapping {}: {e}", path.display()))?;
    let mut raw: Yaml = serde_yaml::from_str(&text)?;

    let constants = load_constants(&raw)?;
    substitute(&mut raw, &constants)?;
    let cfg: MappingFile = serde_yaml::from_value(raw)
        .map_err(|e| format!("invalid mapping {}: {e}", path.display()))?;
    validate(&cfg)?;
    Ok(cfg)
}

fn load_constants(raw: &Yaml) -> Result<BTreeMap<String, Yaml>, ConfigError> {
    let mut out = BTreeMap::new();
    let Some(Yaml::Mapping(sources)) = raw.get("constants") else {
        return Ok(out);
    };
    for (name, source) in sources {
        let name = name.as_str().ok_or("constant names must be strings")?.to_string();
        let source: ConstantSource = serde_yaml::from_value(source.clone())?;
        let text = std::fs::read_to_string(&source.file)
            .map_err(|e| format!("constant {name}: cannot read {}: {e}", source.file))?;
        let mut value: Yaml = serde_yaml::from_str(&text)?;
        if let Some(path) = &source.path {
            for key in path.split('.') {
                value = value
                    .get(key)
                    .cloned()
                    .ok_or_else(|| format!("constant {name}: {}: no key {path}", source.file))?;
            }
        }
        out.insert(name, value);
    }
    Ok(out)
}

fn lookup(constants: &BTreeMap<String, Yaml>, expr: &str) -> Result<Yaml, ConfigError> {
    let mut parts = expr.split('.');
    let head = parts.next().unwrap_or_default();
    let mut value = constants
        .get(head)
        .cloned()
        .ok_or_else(|| format!("unknown constant ${{{expr}}}"))?;
    for key in parts {
        value = value
            .get(key)
            .cloned()
            .ok_or_else(|| format!("unknown constant ${{{expr}}}"))?;
    }
    Ok(value)
}

/// Replace `${name.key}` everywhere except in the `constants` section. A
/// string that is exactly one reference takes the constant's type (so a
/// number stays a number); references inside longer strings are spliced in
/// as text.
fn substitute(raw: &mut Yaml, constants: &BTreeMap<String, Yaml>) -> Result<(), ConfigError> {
    fn walk(v: &mut Yaml, c: &BTreeMap<String, Yaml>) -> Result<(), ConfigError> {
        match v {
            Yaml::String(s) if s.contains("${") => {
                if s.starts_with("${") && s.ends_with('}') && s.matches("${").count() == 1 {
                    *v = lookup(c, &s[2..s.len() - 1])?;
                    return Ok(());
                }
                let mut out = String::new();
                let mut rest = s.as_str();
                while let Some(start) = rest.find("${") {
                    out.push_str(&rest[..start]);
                    let end = rest[start..].find('}').ok_or("unterminated ${")? + start;
                    let value = lookup(c, &rest[start + 2..end])?;
                    out.push_str(&match value {
                        Yaml::String(s) => s,
                        other => serde_yaml::to_string(&other)?.trim().to_string(),
                    });
                    rest = &rest[end + 1..];
                }
                out.push_str(rest);
                *s = out;
            }
            Yaml::Sequence(items) => {
                for item in items {
                    walk(item, c)?;
                }
            }
            Yaml::Mapping(map) => {
                for (key, value) in map.iter_mut() {
                    if key.as_str() != Some("constants") {
                        walk(value, c)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    walk(raw, constants)
}

fn validate(cfg: &MappingFile) -> Result<(), ConfigError> {
    if cfg.version != 1 {
        return Err(format!("unsupported mapping version {}", cfg.version).into());
    }
    let link_dir = |name: &str| cfg.links.iter().find(|l| l.name == name).map(|l| l.direction);
    let mut names = std::collections::BTreeSet::new();
    for route in &cfg.routes {
        if !names.insert(route.name()) {
            return Err(format!("duplicate route name {}", route.name()).into());
        }
        if let RouteCfg::Forward { repeat_last_ms: Some(_), to, name, .. } = route {
            if !matches!(to, EndpointCfg::Link(_)) {
                return Err(format!(
                    "route {name}: repeat_last_ms is only allowed for link outputs (a repeated uProtocol event would be a duplicate)"
                )
                .into());
            }
        }
        for input in route.inputs() {
            if let EndpointCfg::Link(l) = input {
                if link_dir(l) != Some(LinkDirection::FromRos) {
                    return Err(format!("route {}: input link {l} must exist with direction from_ros", route.name()).into());
                }
            }
            if let EndpointCfg::Uprotocol(u) = input {
                resolve_uri(u)?;
            }
        }
        for output in route.outputs() {
            match output {
                EndpointCfg::Link(l) => {
                    if link_dir(l) != Some(LinkDirection::ToRos) {
                        return Err(format!("route {}: output link {l} must exist with direction to_ros", route.name()).into());
                    }
                }
                EndpointCfg::Uprotocol(u) => {
                    resolve_uri(u)?;
                    if cfg.mode == Mode::Mirror {
                        return Err(format!(
                            "route {}: mode mirror must not publish on uProtocol",
                            route.name()
                        )
                        .into());
                    }
                }
            }
        }
    }
    Ok(())
}

/// Resolve a URI reference. Named URIs come from services/src/lib.rs so the
/// IDs are defined in exactly one place.
pub fn resolve_uri(uri: &UriRef) -> Result<UUri, ConfigError> {
    use guardian_sil as g;
    match uri {
        UriRef::Named(name) => Ok(match name.as_str() {
            "vss_child_presence" => g::vss_child_presence_uri(),
            "vss_cabin_temperature" => g::vss_cabin_temperature_uri(),
            "vss_guardian_state" => g::vss_guardian_state_uri(),
            "vss_window_state" => g::vss_window_state_uri(),
            "vss_hvac_set_temperature" => g::vss_hvac_set_temperature_uri(),
            "vss_hvac_active_state" => g::vss_hvac_active_state_uri(),
            "hvac_state" => g::hvac_state_uri(),
            "diag_window_cmd" => g::diag_window_cmd_uri(),
            "diag_alarm_cmd" => g::diag_alarm_cmd_uri(),
            "diag_hvac_cmd" => g::diag_hvac_cmd_uri(),
            "uds_window_cmd" => g::uds_window_cmd_uri(),
            "uds_alarm_cmd" => g::uds_alarm_cmd_uri(),
            "uds_hvac_cmd" => g::uds_hvac_cmd_uri(),
            other => return Err(format!("unknown uProtocol URI name {other}").into()),
        }),
        UriRef::Parts { authority, ue_id, version, resource } => {
            UUri::try_from_parts(authority, *ue_id, *version, *resource)
                .map_err(|e| format!("invalid uProtocol URI parts: {e}").into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tmp(name: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ros_up_mapper_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn mapping(window: &Path, mode: &str, to: &str) -> String {
        format!(
            "version: 1\nmode: {mode}\nzenoh: {{key_prefix: p}}\n\
             constants: {{window: {{file: {}, path: w}}}}\n\
             links: [{{name: cmd, direction: to_ros, ros: {{topic: /t, type: std_msgs/msg/Float64}}}}]\n\
             routes:\n  - name: r\n    kind: forward\n    from: {{uprotocol: vss_window_state}}\n\
             \x20   to: {to}\n    fields:\n      data: {{from: x, ops: [{{mul: \"${{window.travel_m}}\"}}]}}\n",
            window.display()
        )
    }

    #[test]
    fn constants_are_substituted() {
        let window = write_tmp("w1.yaml", "w: {travel_m: 0.4, joint_name: j}\n");
        let path = write_tmp("m1.yaml", &mapping(&window, "mirror", "{link: cmd}"));
        let cfg = load(&path).unwrap();
        let RouteCfg::Forward { fields, .. } = &cfg.routes[0];
        assert!(matches!(fields["data"].ops[0], crate::transform::Op::Mul(x) if x == 0.4));
    }

    #[test]
    fn mirror_must_not_publish_on_uprotocol() {
        let window = write_tmp("w2.yaml", "w: {travel_m: 0.4}\n");
        let path = write_tmp("m2.yaml", &mapping(&window, "mirror", "{uprotocol: vss_window_state}"));
        let err = load(&path).unwrap_err().to_string();
        assert!(err.contains("mirror must not publish"), "{err}");
        let path = write_tmp("m3.yaml", &mapping(&window, "replace", "{uprotocol: vss_window_state}"));
        assert!(load(&path).is_ok());
    }

    #[test]
    fn unknown_constant_is_an_error() {
        let window = write_tmp("w4.yaml", "w: {other: 1}\n");
        let path = write_tmp("m4.yaml", &mapping(&window, "mirror", "{link: cmd}"));
        assert!(load(&path).unwrap_err().to_string().contains("unknown constant"));
    }
}

#[cfg(test)]
mod repeat_tests {
    use super::*;

    #[test]
    fn repeat_only_for_link_outputs() {
        let dir = std::env::temp_dir().join(format!("ros_up_mapper_rep_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.yaml");
        let text = |to: &str| {
            format!(
                "version: 1\nmode: replace\nzenoh: {{key_prefix: p}}\n\
                 links: [{{name: cmd, direction: to_ros, ros: {{topic: /t, type: std_msgs/msg/Float64}}}}]\n\
                 routes:\n  - {{name: r, kind: forward, from: {{uprotocol: uds_window_cmd}}, to: {to}, \
                 repeat_last_ms: 1000, fields: {{data: {{const: 1}}}}}}\n"
            )
        };
        std::fs::write(&path, text("{link: cmd}")).unwrap();
        assert!(load(&path).is_ok());
        std::fs::write(&path, text("{uprotocol: vss_window_state}")).unwrap();
        assert!(load(&path).unwrap_err().to_string().contains("repeat_last_ms"));
    }
}
