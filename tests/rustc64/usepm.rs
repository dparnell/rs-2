#[derive(pm::Name)]
struct Thing;
fn main() { println!("macro says {} {}", pm::answer!(40), Thing::name()); }
