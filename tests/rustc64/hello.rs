use std::collections::HashMap;
fn main() {
    let mut m = HashMap::new();
    for (i, w) in ["rust", "on", "rs2"].iter().enumerate() { m.insert(*w, i); }
    let mut v: Vec<_> = m.into_iter().collect();
    v.sort_by_key(|x| x.1);
    let t = std::thread::spawn(move || v.iter().map(|x| x.0).collect::<Vec<_>>().join(" "));
    println!("hello: {} {:?}", t.join().unwrap(), std::env::args().skip(1).collect::<Vec<_>>());
}
