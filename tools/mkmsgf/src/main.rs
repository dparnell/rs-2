//! MKMSGF for the host: `.TXT` message source to an OS/2 `.MSG` file.
//!
//! ```text
//! mkmsgf infile outfile [/P codepage[,codepage...]] [/L family,sub] [/D dbcs] [/V]
//! mkmsgf @controlfile          (one "infile outfile switches" per line)
//! mkmsgf --decompile file.msg [file.txt]
//! ```
//!
//! The switches are IBM's (crates/msgf). The file name recorded in the
//! message file is the output file's name as given.

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: mkmsgf infile outfile [/P codepage[,...]] [/L family,sub] [/D dbcs-range|country] [/V]");
    eprintln!("       mkmsgf @controlfile");
    eprintln!("       mkmsgf --decompile file.msg [file.txt]");
    ExitCode::from(2)
}

fn make(args: &[String]) -> Result<(), String> {
    let mut opts = msgf::Options::default();
    let mut verbose = false;
    let files = msgf::parse_switches(args, &mut opts, &mut verbose).map_err(|e| e.to_string())?;
    let [input, output] = files.as_slice() else { return Err("need an input and an output file".into()) };
    let src = std::fs::read(input).map_err(|e| format!("{input}: {e}"))?;
    let name = output.rsplit(['/', '\\']).next().unwrap_or(output);
    opts.name = name.as_bytes().to_vec();
    let parsed = msgf::parse(&src, &opts.dbcs).map_err(|e| format!("{input}: {e}"))?;
    let data = msgf::build(&parsed, &opts).map_err(|e| format!("{input}: {e}"))?;
    if verbose {
        let cps: Vec<String> = opts.code_pages.iter().map(|c| c.to_string()).collect();
        eprintln!("  strIn     = {input}");
        eprintln!("  strOut    = {output}");
        eprintln!("  CodePages = {}", if cps.is_empty() { "none".into() } else { cps.join(",") });
        eprintln!("  Language family id = {} and sub id = {}", opts.language.0, opts.language.1);
        eprintln!("  CP_type   = {}", if opts.dbcs.is_empty() { "SBCS" } else { "DBCS" });
        eprintln!("\"{input}\": length = {} bytes.", src.len());
        eprintln!("{} messages scanned.  Writing output file...", parsed.messages.len());
        eprintln!("Size of table entry: {}", if data[0x0F] != 0 { "word" } else { "dword" });
    }
    std::fs::write(output, data).map_err(|e| format!("{output}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("/?") | Some("-?") | Some("--help") => return usage(),
        Some("--decompile") | Some("-d") => {
            let Some(input) = args.get(1) else { return usage() };
            std::fs::read(input)
                .map_err(|e| format!("{input}: {e}"))
                .and_then(|d| msgf::decompile(&d).map_err(|e| format!("{input}: {e}")))
                .and_then(|text| match args.get(2) {
                    Some(out) => std::fs::write(out, text).map_err(|e| format!("{out}: {e}")),
                    None => {
                        use std::io::Write;
                        std::io::stdout().write_all(&text).map_err(|e| e.to_string())
                    }
                })
        }
        Some(a) if a.starts_with('@') => match std::fs::read_to_string(&a[1..]) {
            Ok(text) => text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| make(&l.split_whitespace().map(String::from).collect::<Vec<_>>()))
                .collect(),
            Err(e) => Err(format!("{}: {e}", &a[1..])),
        },
        Some(_) => make(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mkmsgf: {e}");
            ExitCode::FAILURE
        }
    }
}
