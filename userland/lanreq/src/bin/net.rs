//! NET: the LAN requester's command, with the command forms and message
//! numbers of IBM's NET.EXE (File and Print Client):
//! NET USE, NET VIEW, NET START/STOP REQUESTER, NET LOGON/LOGOFF,
//! NET CONFIG. Exit codes: 0 done, 1 finished with errors, 2 failed.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use lanreq::msg;
os2::entry!(main);

fn out(s: &str) {
    os2::write_text(1, s);
}

fn dashes() {
    out("-------------------------------------------------------------------------------\r\n");
}

fn pad(s: &str, w: usize) -> String {
    let mut v: String = s.chars().take(w).collect();
    while v.chars().count() < w {
        v.push(' ');
    }
    v
}

/// Options: `/NAME[:value]` (case-insensitive, any unique prefix of the
/// names given) and the plain words.
struct Args {
    words: Vec<String>,
    switches: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(v: &[String]) -> Args {
        let mut a = Args { words: Vec::new(), switches: Vec::new() };
        for x in v {
            if let Some(s) = x.strip_prefix('/').filter(|_| !x.starts_with("//")) {
                let (k, val) = match s.split_once(':') {
                    Some((k, v)) => (k.to_ascii_uppercase(), Some(String::from(v))),
                    None => (s.to_ascii_uppercase(), None),
                };
                a.switches.push((k, val));
            } else {
                a.words.push(x.clone());
            }
        }
        a
    }

    /// The switch whose name starts with `abbr` and is a prefix of `full`.
    fn switch(&self, full: &str, min: usize) -> Option<Option<String>> {
        self.switches.iter().find(|(k, _)| k.len() >= min && full.starts_with(k.as_str())).map(|(_, v)| v.clone())
    }
}

fn fail(rc: u32) -> u32 {
    msg::error(rc);
    2
}

fn ask(n: u32, default_yes: bool, a: &Args) -> bool {
    if a.switch("YES", 1).is_some() {
        return true;
    }
    if a.switch("NO", 1).is_some() {
        return false;
    }
    let d = if default_yes { "Y" } else { "N" };
    msg::print(n, &[d]);
    let line = utils::ask("");
    match line.trim().chars().next() {
        Some('y' | 'Y') => true,
        Some('n' | 'N') => false,
        _ => default_yes,
    }
}

/// Start the requester if it is not running (as IBM's NET does before
/// USE and VIEW).
fn autostart() -> Result<(), u32> {
    if lanreq::status().started {
        return Ok(());
    }
    msg::print(3521, &["REQUESTER"]);
    start_requester(&[])
}

fn start_requester(sw: &[String]) -> Result<(), u32> {
    msg::print(3522, &["REQUESTER"]);
    out(".");
    match lanreq::start_requester(sw) {
        Ok(()) => {
            out("\r\n");
            msg::print(3524, &["REQUESTER"]);
            Ok(())
        }
        Err(e) => {
            out("\r\n");
            msg::eprint(3523, &["REQUESTER"]);
            Err(e)
        }
    }
}

fn type_name(t: u32) -> String {
    msg::text(4710 + t.min(3))
}

fn status_name(s: u32) -> String {
    msg::text(4715 + if s == 0 { 0 } else { (s + 1).min(6) })
}

fn read_password(prompt_msg: u32, ins: &str) -> Option<String> {
    msg::print(prompt_msg, &[ins]);
    lanreq::read_password()
}

// ---- NET USE ----

fn is_device(s: &str) -> bool {
    let u = s.trim_end_matches(':').to_ascii_uppercase();
    (u.len() == 1 && u.as_bytes()[0].is_ascii_uppercase()) || (u.len() == 4 && u.starts_with("LPT") && u.as_bytes()[3].is_ascii_digit()) || u.starts_with("COM")
}

fn use_cmd(a: &Args) -> u32 {
    if let Err(e) = autostart() {
        return fail(e);
    }
    let w = &a.words[1..];
    let delete = a.switch("DELETE", 1).is_some();
    if w.is_empty() {
        if delete {
            return fail(lanreq::ERROR_INVALID_PARAMETER);
        }
        return use_list();
    }
    let (dev, rest) = if is_device(&w[0]) { (Some(w[0].clone()), &w[1..]) } else { (None, w) };
    if let Some(d) = &dev {
        if d.to_ascii_uppercase().starts_with("COM") {
            return fail(3716);
        }
    }
    if delete {
        let name = dev.clone().or_else(|| rest.first().cloned()).unwrap_or_default();
        return use_del(&name, a);
    }
    let remote = rest.first().cloned();
    let Some(remote) = remote else {
        // NET USE d: shows one connection.
        return use_show(dev.as_deref().unwrap_or(""));
    };
    if !remote.starts_with("\\\\") && !remote.starts_with("//") {
        return fail(lanreq::ERROR_INVALID_PARAMETER);
    }
    let mut password = rest.get(1).cloned();
    if password.as_deref() == Some("*") {
        password = read_password(4357, &remote);
    }
    let user = a.switch("USER", 1).flatten();
    let local = dev.map(|d| String::from(d.trim_end_matches(':'))).unwrap_or_default();
    match lanreq::use_add(&local, &remote, password.as_deref(), user.as_deref()) {
        Ok(()) => {
            msg::print(3500, &[]);
            0
        }
        Err(86 | 5) if password.is_none() => fail(2758),
        Err(e) => fail(e),
    }
}

fn use_list() -> u32 {
    let v = match lanreq::uses() {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    if v.is_empty() {
        msg::print(3683, &[]);
        return 0;
    }
    out("\r\n");
    msg::print(4714, &[]);
    dashes();
    let mut v = v;
    v.sort_by(|a, b| a.local.is_empty().cmp(&b.local.is_empty()).then(a.local.cmp(&b.local)).then(a.remote.cmp(&b.remote)));
    for u in v {
        out(&alloc::format!("{} {} {}\r\n", pad(&status_name(u.status), 15), pad(&u.local, 15), u.remote));
    }
    msg::print(3500, &[]);
    0
}

fn use_show(dev: &str) -> u32 {
    let d = dev.trim_end_matches(':').to_ascii_uppercase();
    let v = lanreq::uses().unwrap_or_default();
    let Some(u) = v.iter().find(|u| u.local.trim_end_matches(':').eq_ignore_ascii_case(&d)) else {
        return fail(lanreq::NERR_USE_NOT_FOUND);
    };
    let labels: Vec<String> = (4722..=4727).map(msg::text).collect();
    let w = labels.iter().map(|l| l.len()).max().unwrap_or(10) + 5;
    let line = |i: usize, v: &str| out(&alloc::format!("{}{}\r\n", pad(&labels[i], w), v));
    line(1, &u.local);
    line(2, &u.remote);
    line(3, &type_name(u.typ));
    line(0, &status_name(u.status));
    line(4, &alloc::format!("{}", u.opens));
    line(5, "1");
    msg::print(3500, &[]);
    0
}

fn use_del(name: &str, a: &Args) -> u32 {
    let name = String::from(name.trim_end_matches(':'));
    let shown = if name.len() == 1 { alloc::format!("{}:", name.to_ascii_uppercase()) } else { name.to_ascii_uppercase() };
    match lanreq::use_del(&name, false) {
        Ok(()) => {}
        Err(lanreq::NERR_OPEN_FILES) => {
            msg::print(3691, &[&shown]);
            if !ask(5289, false, a) {
                return 2;
            }
            if let Err(e) = lanreq::use_del(&name, true) {
                return fail(e);
            }
        }
        Err(e) => return fail(e),
    }
    msg::print(4650, &[&shown]);
    0
}

// ---- NET VIEW ----

fn view_cmd(a: &Args) -> u32 {
    if let Err(e) = autostart() {
        return fail(e);
    }
    match a.words.get(1) {
        Some(s) => view_server(s),
        None => {
            let dom = a.switch("DOMAIN", 1).map(|d| d.unwrap_or_else(|| lanreq::status().workgroup));
            view_servers(dom.as_deref())
        }
    }
}

fn remark(s: &str, w: usize) -> String {
    if s.chars().count() > w { alloc::format!("{}...", s.chars().take(w - 3).collect::<String>()) } else { String::from(s) }
}

fn view_servers(domain: Option<&str>) -> u32 {
    let mut v = match lanreq::servers(domain) {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    if v.is_empty() {
        msg::print(3683, &[]);
        return 0;
    }
    v.sort_by(|a, b| a.name.cmp(&b.name));
    out("\r\n");
    msg::print(4700, &[]);
    dashes();
    for s in v {
        out(&alloc::format!("\\\\{} {}\r\n", pad(&s.name, 20), remark(&s.comment, 56)));
    }
    msg::print(3500, &[]);
    0
}

fn view_server(server: &str) -> u32 {
    let name = String::from(server.trim_start_matches(['\\', '/'])).to_ascii_uppercase();
    let mut v = match lanreq::shares(&name) {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    v.retain(|s| !s.name.ends_with('$'));
    if v.is_empty() {
        msg::print(3683, &[]);
        return 0;
    }
    v.sort_by(|a, b| a.name.cmp(&b.name));
    let unc = alloc::format!("\\\\{name}");
    msg::print(4377, &[&unc]);
    out("\r\n");
    msg::print(4701, &[]);
    dashes();
    let uses = lanreq::uses().unwrap_or_default();
    for s in v {
        let remote = alloc::format!("\\\\{}\\{}", name, s.name.to_ascii_uppercase());
        let used = uses.iter().find(|u| u.remote.eq_ignore_ascii_case(&remote)).map(|u| if u.local.is_empty() { msg::text(4702) } else { u.local.clone() }).unwrap_or_default();
        out(&alloc::format!("{} {} {} {}\r\n", pad(&s.name, 12), pad(&type_name(s.typ), 12), pad(&used, 8), remark(&s.remark, 44)).trim_end());
        out("\r\n");
    }
    msg::print(3500, &[]);
    0
}

// ---- START / STOP / CONFIG ----

fn is_requester(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "requester" | "req" | "redirector" | "redir" | "rdr" | "workstation" | "wksta" | "prdr")
}

fn start_cmd(a: &Args, raw: &[String]) -> u32 {
    let Some(svc) = a.words.get(1) else {
        // NET START: the services running.
        if lanreq::status().started {
            msg::print(4378, &[]);
            out("    REQUESTER\r\n");
            msg::print(3500, &[]);
        } else {
            msg::print(3683, &[]);
        }
        return 0;
    };
    if !is_requester(svc) {
        return fail(lanreq::NERR_BAD_SERVICE_NAME);
    }
    let sw: Vec<String> = raw.iter().filter(|x| x.starts_with('/')).cloned().collect();
    if lanreq::status().started {
        if !sw.is_empty() {
            msg::print(3689, &[]);
            return 0;
        }
        return fail(lanreq::NERR_SERVICE_INSTALLED);
    }
    match start_requester(&sw) {
        Ok(()) => 0,
        Err(e) => fail(e),
    }
}

fn stop_cmd(a: &Args) -> u32 {
    let Some(svc) = a.words.get(1) else { return fail(lanreq::NERR_BAD_SERVICE_NAME) };
    if !is_requester(svc) {
        return fail(lanreq::NERR_BAD_SERVICE_NAME);
    }
    let st = lanreq::status();
    if !st.started {
        return fail(lanreq::NERR_SERVICE_NOT_INSTALLED);
    }
    if !st.user.is_empty() {
        msg::print(3532, &[]);
        if !ask(5284, true, a) {
            return 2;
        }
    }
    let uses = lanreq::uses().unwrap_or_default();
    if !uses.is_empty() {
        msg::print(3670, &[]);
        out("\r\n");
        for u in &uses {
            out(&alloc::format!("    {} {}\r\n", pad(&u.local, 8), u.remote));
        }
        out("\r\n");
        msg::print(3671, &[]);
        if !ask(5286, true, a) {
            return 2;
        }
    }
    msg::print(3527, &["REQUESTER"]);
    let r = lanreq::stop(true);
    out("\r\n");
    match r {
        Ok(()) => {
            msg::print(3529, &["REQUESTER"]);
            0
        }
        Err(e) => {
            msg::eprint(3528, &["REQUESTER"]);
            fail(e)
        }
    }
}

fn config_cmd(a: &Args) -> u32 {
    let st = lanreq::status();
    match a.words.get(1) {
        None => {
            if !st.started {
                msg::print(3683, &[]);
                return 0;
            }
            msg::print(4378, &[]);
            out("    REQUESTER\r\n");
            msg::print(3500, &[]);
            0
        }
        Some(s) if is_requester(s) => {
            if !st.started {
                return fail(lanreq::NERR_WKSTA_NOT_STARTED);
            }
            let labels: Vec<String> = (4450..=4457).map(msg::text).collect();
            let w = labels.iter().map(|l| l.len()).max().unwrap_or(10) + 5;
            let line = |i: usize, v: &str| out(&alloc::format!("{}{}\r\n", pad(&labels[i], w), v));
            line(0, &alloc::format!("\\\\{}", st.computer));
            line(1, &st.user);
            out("\r\n");
            line(3, "SMB2 SMB1 NBT");
            line(2, "5.00");
            line(4, &alloc::format!("{}:\\IBMLAN", lanreq::boot_drive()));
            out("\r\n");
            line(5, &st.workgroup);
            line(6, &st.domain);
            line(7, &st.other_domains);
            msg::print(3500, &[]);
            0
        }
        Some(_) => fail(lanreq::NERR_BAD_SERVICE_NAME),
    }
}

// ---- LOGON / LOGOFF ----

fn logon_cmd(a: &Args) -> u32 {
    if let Err(e) = autostart() {
        return fail(e);
    }
    let st = lanreq::status();
    let user = match a.words.get(1) {
        Some(u) => u.clone(),
        None => {
            let u = utils::ask(&msg::text(4451).replace('\r', "") as &str);
            let u = String::from(u.trim());
            if u.is_empty() {
                return fail(lanreq::NERR_BAD_USERNAME);
            }
            u
        }
    };
    let password = match a.words.get(2).map(|s| s.as_str()) {
        Some("*") | None => match read_password(4360, "") {
            Some(p) => p,
            None => return 2,
        },
        Some(p) => String::from(p),
    };
    let domain = a.switch("DOMAIN", 1).flatten().unwrap_or(st.workgroup.clone());
    match lanreq::logon(&user, &password, &domain) {
        Ok(()) => {
            let d = domain.to_ascii_uppercase();
            msg::print(8800, &[&user, &d]);
            0
        }
        Err(e) => fail(e),
    }
}

fn logoff_cmd() -> u32 {
    let st = lanreq::status();
    if !st.started || st.user.is_empty() {
        return fail(lanreq::NERR_NOT_LOGGED_ON);
    }
    match lanreq::logon("", "", "") {
        Ok(()) => {
            msg::print(8801, &[&st.user]);
            0
        }
        Err(e) => fail(e),
    }
}

fn help() -> u32 {
    for n in 8803..=8810 {
        msg::print(n, &[]);
    }
    0
}

fn main(s: &os2::Startup) -> u32 {
    let raw = utils::args(s);
    let a = Args::parse(&raw);
    let Some(cmd) = a.words.first().map(|c| c.to_ascii_uppercase()) else { return help() };
    match cmd.as_str() {
        "USE" => use_cmd(&a),
        "VIEW" => view_cmd(&a),
        "START" => start_cmd(&a, &raw),
        "STOP" => stop_cmd(&a),
        "CONFIG" => config_cmd(&a),
        "LOGON" => logon_cmd(&a),
        "LOGOFF" => logoff_cmd(),
        "HELP" | "?" | "/?" => help(),
        _ => {
            help();
            1
        }
    }
}
