// Parses a 4 MiB body of the given shape into serde_json::Value, as http.rs:37 does, and clones the items as http.rs:97 does.
fn main() {
    let shape = std::env::args().nth(1).unwrap();
    let max = 4usize << 20;
    let item = match shape.as_str() { "obj" => r#"{"":0}"#, "arr" => "[]", "num" => "0", _ => panic!() };
    let mut s = String::from(r#"{"ads":["#);
    while s.len() + item.len() + 3 < max { s.push_str(item); s.push(','); }
    s.pop(); s.push_str("]}");
    let v: serde_json::Value = serde_json::from_slice(s.as_bytes()).unwrap();
    let items = v.get("ads").unwrap().as_array().unwrap();
    let copy: Vec<serde_json::Value> = items.iter().cloned().collect();
    println!("{} bytes, {} items, {}", s.len(), items.len(), copy.len());
}
