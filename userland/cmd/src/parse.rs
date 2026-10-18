//! Command line parsing: quoting, escapes, pipes, command separators and
//! redirection.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sep {
    /// `&` or end of line: always run the next command.
    Always,
    /// `&&`: run the next command if this one succeeded.
    And,
    /// `||`: run the next command if this one failed.
    Or,
}

#[derive(Clone, Debug)]
pub enum Target {
    File { path: String, append: bool },
    Handle(u32),
}

#[derive(Clone, Debug)]
pub struct Redir {
    /// Handle being redirected (0 = input, 1 = output, 2 = errors).
    pub handle: u32,
    pub target: Target,
}

#[derive(Clone, Debug, Default)]
pub struct Simple {
    /// The command word.
    pub name: String,
    /// The argument text as typed (without redirections), for commands
    /// like ECHO and SET that take it verbatim.
    pub args: String,
    pub redirs: Vec<Redir>,
}

impl Simple {
    /// Arguments split on blanks, quotes removed.
    pub fn words(&self) -> Vec<String> {
        split_words(&self.args)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Pipeline {
    pub stages: Vec<Simple>,
}

pub fn split_words(s: &str) -> Vec<String> {
    let mut v = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            ' ' | '\t' | ',' | ';' if !quoted => {
                if any || !cur.is_empty() {
                    v.push(core::mem::take(&mut cur));
                    any = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        v.push(cur);
    }
    v
}

/// Split a line into pipelines joined by separators.
pub fn parse(line: &str) -> Vec<(Pipeline, Sep)> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut pipeline = Pipeline::default();
    let mut text = String::new();
    let mut redirs: Vec<Redir> = Vec::new();
    let mut i = 0;
    let mut quoted = false;

    let finish_stage = |text: &mut String, redirs: &mut Vec<Redir>, p: &mut Pipeline| {
        let t = text.trim();
        if t.is_empty() && redirs.is_empty() {
            text.clear();
            return;
        }
        let (name, args) = split_command(t);
        p.stages.push(Simple { name, args, redirs: core::mem::take(redirs) });
        text.clear();
    };

    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            quoted = !quoted;
            text.push(c);
            i += 1;
            continue;
        }
        if quoted {
            text.push(c);
            i += 1;
            continue;
        }
        match c {
            '^' if i + 1 < chars.len() => {
                text.push(chars[i + 1]);
                i += 2;
            }
            '|' if chars.get(i + 1) == Some(&'|') => {
                finish_stage(&mut text, &mut redirs, &mut pipeline);
                out.push((core::mem::take(&mut pipeline), Sep::Or));
                i += 2;
            }
            '|' => {
                finish_stage(&mut text, &mut redirs, &mut pipeline);
                i += 1;
            }
            '&' if chars.get(i + 1) == Some(&'&') => {
                finish_stage(&mut text, &mut redirs, &mut pipeline);
                out.push((core::mem::take(&mut pipeline), Sep::And));
                i += 2;
            }
            '&' => {
                finish_stage(&mut text, &mut redirs, &mut pipeline);
                out.push((core::mem::take(&mut pipeline), Sep::Always));
                i += 1;
            }
            '<' | '>' => {
                // Optional handle number just before the operator.
                let mut handle = if c == '<' { 0 } else { 1 };
                if let Some(d) = text.chars().last().and_then(|d| d.to_digit(10)) {
                    let before = text.chars().rev().nth(1);
                    if before.is_none_or(|b| b == ' ') {
                        handle = d;
                        text.pop();
                    }
                }
                let mut append = false;
                i += 1;
                if c == '>' && chars.get(i) == Some(&'>') {
                    append = true;
                    i += 1;
                }
                // ">&n" duplicates a handle.
                if chars.get(i) == Some(&'&') {
                    if let Some(d) = chars.get(i + 1).and_then(|d| d.to_digit(10)) {
                        redirs.push(Redir { handle, target: Target::Handle(d) });
                        i += 2;
                        continue;
                    }
                }
                while chars.get(i) == Some(&' ') {
                    i += 1;
                }
                let mut path = String::new();
                let mut q = false;
                while i < chars.len() {
                    let ch = chars[i];
                    if ch == '"' {
                        q = !q;
                        i += 1;
                        continue;
                    }
                    if !q && (ch == ' ' || ch == '|' || ch == '&' || ch == '<' || ch == '>') {
                        break;
                    }
                    path.push(ch);
                    i += 1;
                }
                redirs.push(Redir { handle, target: Target::File { path, append } });
            }
            _ => {
                text.push(c);
                i += 1;
            }
        }
    }
    finish_stage(&mut text, &mut redirs, &mut pipeline);
    if !pipeline.stages.is_empty() {
        out.push((pipeline, Sep::Always));
    }
    out
}

/// Separate the command word from its arguments. Commands may be followed
/// directly by `.`, `\` or `/` ("cd\", "echo.", "dir/w").
fn split_command(t: &str) -> (String, String) {
    let t = t.trim_start();
    if let Some(rest) = t.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return (String::from(&rest[..end]), String::from(rest[end + 1..].trim_start()));
        }
    }
    let end = t
        .char_indices()
        .find(|&(i, ch)| ch == ' ' || ch == '\t' || (i > 0 && (ch == '/' || ch == '=' || ch == ',')))
        .map(|(i, _)| i)
        .unwrap_or(t.len());
    let mut name = String::from(&t[..end]);
    let mut rest = &t[end..];
    // "echo." / "cd\" / "cd.." forms.
    for (pfx, len) in [("ECHO.", 4), ("CD\\", 2), ("CD..", 2), ("CD.", 2)] {
        if name.len() > len && name.to_ascii_uppercase().starts_with(pfx) {
            rest = &t[len..];
            name.truncate(len);
            break;
        }
    }
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    (name, String::from(rest))
}
