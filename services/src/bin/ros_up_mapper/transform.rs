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

//! JSON field mapping: one `FieldSpec` per output field.
//!
//! A field value comes from exactly one source (`from` path in the input,
//! `state` name, `const`, or `now_ms`), then runs through `ops` in order and
//! is finally converted to `type`.
//!
//! Paths: `a.b`, `a[2]`, and `position[name==joint]`, which picks the element
//! of the array `position` at the index where the sibling array `name`
//! equals `joint` (the sensor_msgs/JointState layout).

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default, rename = "const")]
    pub constant: Option<Value>,
    #[serde(default)]
    pub now_ms: bool,
    #[serde(default)]
    pub ops: Vec<Op>,
    #[serde(default, rename = "type")]
    pub out_type: Option<OutType>,
}

/// One arithmetic step. Written in YAML as `round` or a one-key map such as
/// `{div: 100}` / `{clamp: [0, 100]}` (serde_yaml 0.9 would otherwise expect
/// `!div 100` tags, hence the explicit conversion from `OpRepr`).
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "OpRepr")]
pub enum Op {
    Add(f64),
    Sub(f64),
    Mul(f64),
    Div(f64),
    Round,
    Clamp(f64, f64),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OpRepr {
    Name(String),
    Map(BTreeMap<String, Value>),
}

impl TryFrom<OpRepr> for Op {
    type Error = String;

    fn try_from(repr: OpRepr) -> Result<Self, Self::Error> {
        let (name, arg) = match repr {
            OpRepr::Name(name) => (name, Value::Null),
            OpRepr::Map(map) if map.len() == 1 => map.into_iter().next().unwrap(),
            OpRepr::Map(_) => return Err("an op is one key, e.g. {div: 100}".into()),
        };
        let num = |v: &Value| v.as_f64().ok_or_else(|| format!("op {name}: expected a number, got {v}"));
        Ok(match name.as_str() {
            "add" => Op::Add(num(&arg)?),
            "sub" => Op::Sub(num(&arg)?),
            "mul" => Op::Mul(num(&arg)?),
            "div" => Op::Div(num(&arg)?),
            "round" => Op::Round,
            "clamp" => match arg.as_array().map(Vec::as_slice) {
                Some([lo, hi]) => Op::Clamp(num(lo)?, num(hi)?),
                _ => return Err("op clamp: expected [min, max]".into()),
            },
            other => return Err(format!("unknown op {other}")),
        })
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutType {
    F64,
    I64,
    U8,
    U64,
    Bool,
    String,
}

pub type TransformError = String;

/// Values a field can draw from besides the input message.
#[derive(Default)]
pub struct Context<'a> {
    pub input: Option<&'a Value>,
    pub state: Option<&'a BTreeMap<String, Value>>,
}

impl FieldSpec {
    pub fn eval(&self, ctx: &Context) -> Result<Value, TransformError> {
        let sources = [self.from.is_some(), self.state.is_some(), self.constant.is_some(), self.now_ms];
        if sources.iter().filter(|s| **s).count() != 1 {
            return Err("field needs exactly one of from / state / const / now_ms".into());
        }
        let mut value = if let Some(path) = &self.from {
            let input = ctx.input.ok_or("no input message")?;
            get_path(input, path).ok_or_else(|| format!("path {path} not found"))?
        } else if let Some(name) = &self.state {
            ctx.state
                .and_then(|s| s.get(name))
                .cloned()
                .ok_or_else(|| format!("state {name} not set"))?
        } else if let Some(c) = &self.constant {
            c.clone()
        } else {
            Value::from(now_ms())
        };

        if !self.ops.is_empty() {
            let mut x = value.as_f64().ok_or_else(|| format!("ops need a number, got {value}"))?;
            for op in &self.ops {
                x = match op {
                    Op::Add(v) => x + v,
                    Op::Sub(v) => x - v,
                    Op::Mul(v) => x * v,
                    Op::Div(v) => x / v,
                    Op::Round => x.round(),
                    Op::Clamp(lo, hi) => x.clamp(*lo, *hi),
                };
            }
            value = Value::from(x);
        }
        match self.out_type {
            Some(t) => convert(value, t),
            None => Ok(value),
        }
    }
}

/// Build the output object; dotted field names create nested objects.
pub fn map_fields(
    fields: &BTreeMap<String, FieldSpec>,
    ctx: &Context,
) -> Result<Value, TransformError> {
    let mut out = Map::new();
    for (name, spec) in fields {
        let value = spec.eval(ctx).map_err(|e| format!("field {name}: {e}"))?;
        let mut target = &mut out;
        let mut parts: Vec<&str> = name.split('.').collect();
        let leaf = parts.pop().unwrap_or_default();
        for part in parts {
            target = target
                .entry(part.to_string())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .ok_or_else(|| format!("field {name}: {part} is not an object"))?;
        }
        target.insert(leaf.to_string(), value);
    }
    Ok(Value::Object(out))
}

fn convert(value: Value, t: OutType) -> Result<Value, TransformError> {
    let num = || value.as_f64().ok_or_else(|| format!("expected a number, got {value}"));
    Ok(match t {
        OutType::F64 => Value::from(num()?),
        OutType::I64 => Value::from(num()?.round() as i64),
        OutType::U8 => {
            let x = num()?.round();
            if !(0.0..=255.0).contains(&x) {
                return Err(format!("{x} out of range for u8"));
            }
            Value::from(x as u8)
        }
        OutType::U64 => {
            let x = num()?.round();
            if x < 0.0 {
                return Err(format!("{x} out of range for u64"));
            }
            Value::from(x as u64)
        }
        OutType::Bool => match &value {
            Value::Bool(b) => Value::Bool(*b),
            Value::Number(_) => Value::Bool(num()? != 0.0),
            other => return Err(format!("expected a bool, got {other}")),
        },
        OutType::String => match value {
            Value::String(s) => Value::String(s),
            other => Value::String(other.to_string()),
        },
    })
}

/// Look up a path (see module docs) in a JSON value.
pub fn get_path(root: &Value, path: &str) -> Option<Value> {
    let mut current = root.clone();
    for segment in path.split('.') {
        let (field, selectors) = match segment.find('[') {
            Some(i) => (&segment[..i], &segment[i..]),
            None => (segment, ""),
        };
        let parent = current.clone();
        if !field.is_empty() {
            current = current.get(field)?.clone();
        }
        let mut rest = selectors;
        while let Some(stripped) = rest.strip_prefix('[') {
            let end = stripped.find(']')?;
            let selector = &stripped[..end];
            rest = &stripped[end + 1..];
            let index = if let Some((key, wanted)) = selector.split_once("==") {
                // index of `wanted` in the sibling array `key`
                parent
                    .get(key)?
                    .as_array()?
                    .iter()
                    .position(|v| v.as_str() == Some(wanted) || v.to_string() == wanted)?
            } else {
                selector.parse::<usize>().ok()?
            };
            current = current.get(index)?.clone();
        }
    }
    Some(current)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn spec(yaml: &str) -> FieldSpec {
        serde_yaml::from_str(yaml).unwrap()
    }

    #[test]
    fn percent_to_metres_and_back() {
        let to_m = spec("{from: window_percentage, ops: [{div: 100}, {mul: 0.4}, {add: 0.0}], type: f64}");
        let input = json!({"window_percentage": 25, "alarm_enabled": true});
        let ctx = Context { input: Some(&input), ..Default::default() };
        assert!((to_m.eval(&ctx).unwrap().as_f64().unwrap() - 0.1).abs() < 1e-12);

        let to_pct = spec(
            "{from: 'position[name==w]', ops: [{sub: 0.0}, {div: 0.4}, {mul: 100}, round, {clamp: [0, 100]}], type: u8}",
        );
        let js = json!({"name": ["x", "w"], "position": [9.0, 0.0987]});
        let ctx = Context { input: Some(&js), ..Default::default() };
        assert_eq!(to_pct.eval(&ctx).unwrap(), json!(25));
    }

    #[test]
    fn paths_and_nesting() {
        let v = json!({"a": {"b": [1, {"c": "x"}]}});
        assert_eq!(get_path(&v, "a.b[1].c"), Some(json!("x")));
        assert_eq!(get_path(&v, "a.missing"), None);
        let fields: BTreeMap<String, FieldSpec> =
            serde_yaml::from_str("{header.frame_id: {const: base}, data: {const: 1.5}}").unwrap();
        let out = map_fields(&fields, &Context::default()).unwrap();
        assert_eq!(out, json!({"data": 1.5, "header": {"frame_id": "base"}}));
    }

    #[test]
    fn exactly_one_source() {
        assert!(spec("{const: 1, now_ms: true}").eval(&Context::default()).is_err());
        assert!(spec("{ops: [round]}").eval(&Context::default()).is_err());
    }
}
