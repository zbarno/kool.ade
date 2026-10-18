use serde_json::Value;
use zeroize::Zeroize;

pub(super) fn clear_payload(payload: &mut Value) {
    zeroize_value(payload);
    *payload = Value::Null;
}

fn zeroize_value(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(values) => {
            for value in values.iter_mut() {
                zeroize_value(value);
            }
            values.clear();
        }
        Value::Object(object) => {
            let entries = std::mem::take(object);
            for (mut key, mut value) in entries {
                key.zeroize();
                zeroize_value(&mut value);
            }
        }
        // JSON byte arrays have historically represented bytes as inline
        // numbers. Overwrite those enum slots before their vectors are cleared.
        Value::Number(_) => *value = Value::Null,
        Value::Bool(_) => *value = Value::Null,
        Value::Null => {}
    }
}
