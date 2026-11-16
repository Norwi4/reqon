use prost_reflect::{DynamicMessage, Kind, MessageDescriptor, SerializeOptions, Value};
use serde_json::Value as Json;

fn is_wrapper(name: &str) -> bool {
    matches!(
        name,
        "google.protobuf.DoubleValue"
            | "google.protobuf.FloatValue"
            | "google.protobuf.Int64Value"
            | "google.protobuf.UInt64Value"
            | "google.protobuf.Int32Value"
            | "google.protobuf.UInt32Value"
            | "google.protobuf.BoolValue"
            | "google.protobuf.StringValue"
            | "google.protobuf.BytesValue"
    )
}

fn has_custom_json(name: &str) -> bool {
    is_wrapper(name)
        || matches!(
            name,
            "google.protobuf.Any"
                | "google.protobuf.Timestamp"
                | "google.protobuf.Duration"
                | "google.protobuf.Struct"
                | "google.protobuf.Value"
                | "google.protobuf.ListValue"
                | "google.protobuf.FieldMask"
                | "google.protobuf.Empty"
        )
}

// Some gRPC clients expose wrappers as their protobuf message shape, {"value": ...}.
// Accept that shape as well as ProtoJSON scalars, without unwrapping custom messages
// or hiding unknown fields from prost-reflect's normal validation.
fn normalize_wrappers(desc: &MessageDescriptor, value: &mut Json) {
    if is_wrapper(desc.full_name()) {
        if let Some(object) = value.as_object_mut() {
            if object.len() == 1 && object.contains_key("value") {
                *value = object.remove("value").unwrap();
            }
        }
        return;
    }
    if desc.full_name() == "google.protobuf.Any" {
        if let Some(embedded) = value
            .get("@type")
            .and_then(Json::as_str)
            .and_then(|url| url.rsplit('/').next())
            .and_then(|name| desc.parent_pool().get_message_by_name(name))
        {
            if has_custom_json(embedded.full_name()) {
                if let Some(payload) = value.get_mut("value") {
                    normalize_wrappers(&embedded, payload);
                }
            } else {
                normalize_wrappers(&embedded, value);
            }
        }
        return;
    }
    // Struct/Value contain arbitrary JSON, not protobuf field names.
    if has_custom_json(desc.full_name()) {
        return;
    }
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (name, value) in object {
        let Some(field) = desc
            .get_field_by_json_name(name)
            .or_else(|| desc.get_field_by_name(name))
        else {
            continue;
        };
        let Kind::Message(message) = field.kind() else {
            continue;
        };
        if field.is_map() {
            if let (Kind::Message(entry), Some(values)) = (
                message.map_entry_value_field().kind(),
                value.as_object_mut(),
            ) {
                for item in values.values_mut() {
                    normalize_wrappers(&entry, item);
                }
            }
        } else if field.is_list() {
            if let Some(values) = value.as_array_mut() {
                for item in values {
                    normalize_wrappers(&message, item);
                }
            }
        } else {
            normalize_wrappers(&message, value);
        }
    }
}

pub fn deserialize(
    desc: MessageDescriptor,
    mut value: Json,
) -> Result<DynamicMessage, serde_json::Error> {
    normalize_wrappers(&desc, &mut value);
    DynamicMessage::deserialize(desc, value)
}

fn example_message(
    desc: MessageDescriptor,
    ancestors: &mut Vec<String>,
    budget: &mut usize,
) -> Option<DynamicMessage> {
    // Any needs a user-selected payload type. Recursive fields remain absent.
    if desc.full_name() == "google.protobuf.Any"
        || ancestors.len() >= 12
        || *budget == 0
        || ancestors.iter().any(|name| name == desc.full_name())
    {
        return None;
    }
    *budget -= 1;
    let mut message = DynamicMessage::new(desc.clone());
    if has_custom_json(desc.full_name()) {
        return Some(message);
    }
    ancestors.push(desc.full_name().to_owned());
    let mut oneofs = std::collections::HashSet::new();
    for field in desc.fields() {
        if field.is_list() || field.is_map() {
            continue;
        }
        let oneof = field.containing_oneof();
        if oneof.as_ref().is_some_and(|o| oneofs.contains(o.name())) {
            continue;
        }
        let value = match field.kind() {
            Kind::Message(child) => {
                let Some(child) = example_message(child, ancestors, budget) else {
                    continue;
                };
                Value::Message(child)
            }
            _ => field.default_value(),
        };
        message.set_field(&field, value);
        if let Some(oneof) = oneof {
            oneofs.insert(oneof.name().to_owned());
        }
    }
    ancestors.pop();
    Some(message)
}

pub fn example(desc: MessageDescriptor) -> String {
    let message = example_message(desc.clone(), &mut Vec::new(), &mut 256)
        .unwrap_or_else(|| DynamicMessage::new(desc));
    let options = SerializeOptions::new()
        .skip_default_fields(false)
        .use_proto_field_name(true);
    let value = message
        .serialize_with_options(serde_json::value::Serializer, &options)
        .unwrap_or(serde_json::json!({}));
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use prost_reflect::DescriptorPool;
    use serde_json::json;

    fn schema() -> MessageDescriptor {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input.proto");
        std::fs::write(
            &path,
            r#"
            syntax = "proto3"; package qa;
            import "google/protobuf/wrappers.proto";
            import "google/protobuf/struct.proto";
            import "google/protobuf/timestamp.proto";
            import "google/protobuf/duration.proto";
            import "google/protobuf/any.proto";
            message Custom { int64 value = 1; }
            message Child { google.protobuf.BoolValue active = 1; }
            message Input {
                google.protobuf.Int64Value signed_id = 1;
                google.protobuf.UInt64Value unsigned_id = 2;
                google.protobuf.Int32Value small_signed = 3;
                google.protobuf.UInt32Value small_unsigned = 4;
                google.protobuf.FloatValue float_value = 5;
                google.protobuf.DoubleValue double_value = 6;
                google.protobuf.BoolValue flag = 7;
                google.protobuf.StringValue text = 8;
                google.protobuf.BytesValue data = 9;
                repeated google.protobuf.Int64Value ids = 10;
                map<string, google.protobuf.Int64Value> counts = 11;
                Child child = 12;
                Custom custom = 13;
                google.protobuf.Struct arbitrary = 14;
                google.protobuf.Value arbitrary_value = 15;
                google.protobuf.Timestamp at = 16;
                google.protobuf.Duration elapsed = 17;
                oneof choice { string label = 18; int32 number = 19; }
                optional string optional_text = 20;
                Input recursive = 21;
                google.protobuf.Any extra = 22;
                repeated Child children = 23;
                map<string, Child> child_map = 24;
            }
        "#,
        )
        .unwrap();
        let pool = DescriptorPool::from_file_descriptor_set(
            protox::compile([path], [dir.path()]).unwrap(),
        )
        .unwrap();
        pool.get_message_by_name("qa.Input").unwrap()
    }

    #[test]
    fn wrapper_objects_match_canonical_wire_bytes_without_losing_integer_precision() {
        let desc = schema();
        let wrapped = json!({
            "signed_id":{"value":"-9223372036854775808"}, "unsignedId":{"value":"18446744073709551615"},
            "small_signed":{"value":-7}, "smallUnsigned":{"value":7},
            "float_value":{"value":1.5}, "doubleValue":{"value":"Infinity"},
            "flag":{"value":false}, "text":{"value":""}, "data":{"value":"AAE="},
            "ids":[{"value":"9007199254740993"}, "2"], "counts":{"a":{"value":3}},
            "child":{"active":{"value":true}}, "custom":{"value":"4"},
            "arbitrary":{"signed_id":{"value":"untouched"}}, "arbitrary_value":{"value":5},
            "extra":{"@type":"type.googleapis.com/qa.Child","active":{"value":true}},
            "children":[{"active":{"value":false}}], "child_map":{"a":{"active":{"value":true}}}
        });
        let canonical = json!({
            "signedId":"-9223372036854775808", "unsignedId":"18446744073709551615",
            "smallSigned":-7, "smallUnsigned":7, "floatValue":1.5, "doubleValue":"Infinity",
            "flag":false, "text":"", "data":"AAE=", "ids":["9007199254740993","2"],
            "counts":{"a":"3"}, "child":{"active":true}, "custom":{"value":"4"},
            "arbitrary":{"signed_id":{"value":"untouched"}}, "arbitraryValue":{"value":5},
            "extra":{"@type":"type.googleapis.com/qa.Child","active":true},
            "children":[{"active":false}], "childMap":{"a":{"active":true}}
        });
        let expected = DynamicMessage::deserialize(desc.clone(), &canonical).unwrap();
        assert_eq!(
            deserialize(desc.clone(), wrapped).unwrap().encode_to_vec(),
            expected.encode_to_vec()
        );
        assert_eq!(
            deserialize(desc, canonical).unwrap().encode_to_vec(),
            expected.encode_to_vec()
        );
    }

    #[test]
    fn malformed_wrappers_and_custom_messages_still_fail_validation() {
        let desc = schema();
        for invalid in [
            json!({"signedId":{"value":1,"typo":2}}),
            json!({"signedId":{"typo":1}}),
            json!({"signedId":{"value":"9223372036854775808"}}),
            json!({"flag":{"value":"false"}}),
            json!({"custom":1}),
            json!({"missing":{"value":1}}),
        ] {
            assert!(
                deserialize(desc.clone(), invalid.clone()).is_err(),
                "accepted {invalid}"
            );
        }
        assert_eq!(
            deserialize(desc.clone(), json!({"signedId":null}))
                .unwrap()
                .encode_to_vec(),
            Vec::<u8>::new()
        );
        assert!(
            deserialize(desc, json!({"signedId":{"value":null}}))
                .unwrap()
                .encode_to_vec()
                .is_empty()
        );
    }

    #[test]
    fn examples_include_nested_optional_and_wrapper_fields_with_valid_oneofs() {
        let desc = schema();
        let value: Json = serde_json::from_str(&example(desc.clone())).unwrap();
        assert_eq!(value["signed_id"], "0");
        assert_eq!(value["child"]["active"], false);
        assert_eq!(value["custom"]["value"], "0");
        assert_eq!(value["optional_text"], "");
        assert_eq!(value["at"], "1970-01-01T00:00:00Z");
        assert_eq!(value["elapsed"], "0s");
        assert_eq!(value["ids"], json!([]));
        assert_eq!(value["counts"], json!({}));
        assert_eq!(value["label"], "");
        assert!(value.get("number").is_none());
        assert!(value.get("recursive").is_none());
        DynamicMessage::deserialize(desc, &value).unwrap();
    }
}
