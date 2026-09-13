use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
struct SessionMetadata {
    project_path: Option<String>,
    model: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(flatten)]
    extra: serde_json::Value,
}
fn main() {
    let m = SessionMetadata::default();
    let s = serde_json::to_string(&m);
    println!("serialize(default, extra=Null) = {:?}", s);
    match s {
        Ok(s) => {
            let back: SessionMetadata = serde_json::from_str(&s).unwrap();
            println!("round-trip extra = {:?} (was Null) equal={}", back.extra, back == m);
        }
        Err(e) => println!("serialize error: {e}"),
    }
    for v in [serde_json::json!("str"), serde_json::json!(1), serde_json::json!([1,2]), serde_json::json!({"k":"v"})] {
        let m = SessionMetadata { extra: v.clone(), ..Default::default() };
        println!("extra={v} -> {:?}", serde_json::to_string(&m).map_err(|e| e.to_string()));
    }
}
