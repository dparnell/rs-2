//! Convert a WinHelp file into an IPF help library:
//! `hlp2ipf IN.HLP [OUT.HLP [OUT.IPF]]` (the IPF source too when a third
//! name is given).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(input) = args.first() else {
        eprintln!("usage: hlp2ipf IN.HLP [OUT.HLP [OUT.IPF]]");
        std::process::exit(2);
    };
    let data = std::fs::read(input).expect("read");
    let name = std::path::Path::new(input).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let opts = winhlp::Options { name: Some(name.as_bytes()), keep_source: true, ..Default::default() };
    let t = std::time::Instant::now();
    match winhlp::convert_with(&data, &opts) {
        Ok(c) => {
            println!(
                "{input}: {:?} {} bytes of IPF, {} contexts, {} map numbers, {} keywords, contents res {}, in {:?}",
                String::from_utf8_lossy(&c.title),
                c.ipf.len(),
                c.contexts.len(),
                c.map.len(),
                c.keywords.len(),
                c.contents_res,
                t.elapsed()
            );
            for w in &c.warnings {
                println!("  warning: {w}");
            }
            if let Some(o) = args.get(1) {
                std::fs::write(o, &c.ipf).expect("write");
            }
            if let Some(o) = args.get(2) {
                std::fs::write(o, &c.source).expect("write");
            }
        }
        Err(winhlp::Error::Compile { line, msg, source }) => {
            println!("{input}: IPF line {line}: {msg}");
            let l = String::from_utf8_lossy(&source).lines().nth(line.saturating_sub(1)).unwrap_or("").chars().take(300).collect::<String>();
            println!("  {l}");
            if let Some(o) = args.get(2) {
                std::fs::write(o, &source).expect("write");
            }
        }
        Err(e) => println!("{input}: {e:?}"),
    }
}
