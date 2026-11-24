include!(concat!(env!("OUT_DIR"), "/gen.rs"));

fn main() {
    assert!(inifile::parse(b"").profile.apps.is_empty());
    println!("app: {} {} {} {:?}", GENERATED, mac::twice!(21), env!("FROM_BUILD"), std::env::args().skip(1).collect::<Vec<_>>());
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated() {
        assert_eq!(super::GENERATED, 7);
    }
    #[test]
    #[should_panic]
    fn panics() {
        panic!("as it should");
    }
}
