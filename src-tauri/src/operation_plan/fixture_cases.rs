//! Shared negative fixtures describe mutations to a named valid sample.
use serde_json::Value;

pub(super) fn materialize(bases: &Value, case: &Value) -> Value {
    let name = case["base"].as_str().expect("fixture requires a base");
    let mut value = bases.get(name).expect("fixture base must exist").clone();
    for change in case["changes"]
        .as_array()
        .expect("fixture requires changes")
    {
        let pointer = change["pointer"]
            .as_str()
            .expect("fixture requires a pointer");
        let operation = change["op"]
            .as_str()
            .expect("fixture requires an operation");
        if pointer.is_empty() {
            assert_eq!(operation, "set");
            value = change.get("value").expect("set requires a value").clone();
            continue;
        }
        let (parent, key) = pointer
            .rsplit_once('/')
            .expect("fixture pointer is absolute");
        let key = key.replace("~1", "/").replace("~0", "~");
        let target = value
            .pointer_mut(parent)
            .expect("fixture parent must exist");
        match (operation, target) {
            ("remove", Value::Object(fields)) => {
                assert!(fields.remove(&key).is_some(), "fixture removal must exist");
            }
            ("set", Value::Object(fields)) => {
                fields.insert(
                    key,
                    change.get("value").expect("set requires value").clone(),
                );
            }
            ("set", Value::Array(items)) => {
                let index: usize = key.parse().expect("fixture array index must be numeric");
                items[index] = change.get("value").expect("set requires value").clone();
            }
            _ => panic!("unsupported fixture operation or target"),
        }
    }
    value
}
