//! The JSON Schema of a tool, made from its specification.

use serde_json::{json, Map, Value};

use super::{Param, ParamDefault, ParamType, ToolSpec};

/// The entry of one tool in the `tools/list` result.
pub(in crate::mcp) fn tool_definition(spec: &ToolSpec) -> Value {
    json!({
        "name": spec.name,
        "description": spec.description,
        "inputSchema": input_schema(spec)
    })
}

/// The schema rejects each argument that is not in the specification.
pub(in crate::mcp) fn input_schema(spec: &ToolSpec) -> Value {
    let properties: Map<String, Value> = spec
        .parameters()
        .map(|param| (param.name.to_string(), property(param)))
        .collect();
    let required: Vec<&str> = spec
        .parameters()
        .filter(|param| param.required)
        .map(|param| param.name)
        .collect();
    let mut schema = json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}

fn property(param: &Param) -> Value {
    let mut property = match param.kind {
        ParamType::String => json!({"type": "string"}),
        ParamType::Integer { minimum, maximum } => {
            json!({"type": "integer", "minimum": minimum, "maximum": maximum})
        }
        ParamType::Number { minimum, maximum } => {
            json!({"type": "number", "minimum": minimum, "maximum": maximum})
        }
        ParamType::Boolean => json!({"type": "boolean"}),
        ParamType::Enum(values) => json!({"type": "string", "enum": values}),
        ParamType::StringArray => json!({"type": "array", "items": {"type": "string"}}),
    };
    property["description"] = match param.default {
        Some(ParamDefault::Configuration(key)) => json!(format!(
            "{} The default is `{key}` in the configuration.",
            param.description
        )),
        _ => json!(param.description),
    };
    let default = match param.default {
        Some(ParamDefault::String(value)) => Some(json!(value)),
        Some(ParamDefault::Integer(value)) => Some(json!(value)),
        Some(ParamDefault::Number(value)) => Some(json!(value)),
        Some(ParamDefault::Boolean(value)) => Some(json!(value)),
        // The value is not the same for each server, thus the schema cannot
        // show it. The description above names the configuration key.
        Some(ParamDefault::Configuration(_)) | None => None,
    };
    if let Some(default) = default {
        property["default"] = default;
    }
    property
}

#[cfg(test)]
mod tests {
    use super::super::{find, Pagination, ToolOutput};
    use super::*;

    static SAMPLE: ToolSpec = ToolSpec {
        name: "sample",
        description: "A sample.",
        params: &[
            Param::string("name", "A name.").required(),
            Param::integer("depth", 0, 4, "A depth.").with_default(ParamDefault::Integer(2)),
            Param::number("score", 0.0, 1.0, "A score.").with_default(ParamDefault::Number(0.5)),
            Param::boolean("deep", "A flag.").with_default(ParamDefault::Boolean(true)),
            Param::choice("mode", &["fast", "slow"], "A mode.")
                .with_default(ParamDefault::String("fast")),
            Param::string_array("files", "Some files."),
            Param::string("symbol", "A symbol.").required(),
        ],
        paginated: Pagination::None,
        handler: |_, _| Ok(ToolOutput::Text(String::new())),
    };

    #[test]
    fn schema_has_one_property_for_each_parameter_type() {
        assert_eq!(
            input_schema(&SAMPLE),
            json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["name", "symbol"],
                "properties": {
                    "name": {"type": "string", "description": "A name."},
                    "depth": {
                        "type": "integer", "minimum": 0, "maximum": 4, "default": 2,
                        "description": "A depth."
                    },
                    "score": {
                        "type": "number", "minimum": 0.0, "maximum": 1.0, "default": 0.5,
                        "description": "A score."
                    },
                    "deep": {"type": "boolean", "default": true, "description": "A flag."},
                    "mode": {
                        "type": "string", "enum": ["fast", "slow"], "default": "fast",
                        "description": "A mode."
                    },
                    "files": {
                        "type": "array", "items": {"type": "string"},
                        "description": "Some files."
                    },
                    "symbol": {"type": "string", "description": "A symbol."}
                }
            })
        );
    }

    #[test]
    fn schema_of_impact_comes_from_its_specification() {
        let schema = input_schema(find("impact").expect("impact specification"));
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["symbol"]));
        let properties = schema["properties"].as_object().expect("properties");
        let names: Vec<&str> = properties.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "compact",
                "depth",
                "direction",
                "limit",
                "offset",
                "path",
                "symbol"
            ]
        );
        assert_eq!(
            properties["direction"]["enum"],
            json!(["callers", "callees", "both"])
        );
        assert_eq!(properties["direction"]["default"], "both");
        assert_eq!(properties["limit"]["default"], 50);
        assert_eq!(properties["limit"]["minimum"], 0);
        assert_eq!(properties["limit"]["maximum"], 4294967295_u64);
        assert_eq!(properties["offset"]["default"], 0);
    }

    #[test]
    fn each_integer_property_has_the_bounds_that_the_validation_applies() {
        for spec in super::super::all() {
            let schema = input_schema(spec);
            for param in spec.parameters() {
                if let ParamType::Integer { minimum, maximum } = param.kind {
                    let property = &schema["properties"][param.name];
                    assert_eq!(property["minimum"], minimum, "{}.{}", spec.name, param.name);
                    assert_eq!(property["maximum"], maximum, "{}.{}", spec.name, param.name);
                }
            }
        }
    }

    #[test]
    fn a_default_from_the_configuration_is_not_in_the_schema() {
        for (tool, param) in [
            ("flags", "stale_days"),
            ("flags", "provider"),
            ("defect", "days"),
            ("hotspot", "days"),
        ] {
            let schema = input_schema(find(tool).expect("specification"));
            let property = &schema["properties"][param];
            assert!(property.get("default").is_none(), "{tool}.{param}");
            assert!(
                property["description"]
                    .as_str()
                    .unwrap()
                    .ends_with("in the configuration."),
                "{tool}.{param}"
            );
        }
    }

    #[test]
    fn schema_without_a_required_parameter_has_no_required_key() {
        let schema = input_schema(find("complexity").expect("complexity specification"));
        assert!(schema.get("required").is_none());
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn schema_of_a_tool_without_pagination_has_no_limit() {
        let schema = input_schema(find("task_context").expect("task_context specification"));
        assert!(schema["properties"].get("limit").is_none());
        assert!(schema["properties"].get("offset").is_none());
        assert_eq!(schema["properties"]["max_bytes"]["minimum"], 1024);
    }

    #[test]
    fn definition_has_the_name_the_description_and_the_schema() {
        let definition = tool_definition(&SAMPLE);
        assert_eq!(definition["name"], "sample");
        assert_eq!(definition["description"], "A sample.");
        assert_eq!(definition["inputSchema"], input_schema(&SAMPLE));
    }
}
