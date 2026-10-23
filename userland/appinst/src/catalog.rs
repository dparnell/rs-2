//! The catalogue of OS/2 applications (apps.txt, built in): categories,
//! their applications, the files of each on the OS/2 CD and the program
//! objects made for them.

use alloc::vec::Vec;

static TEXT: &[u8] = include_bytes!("../apps.txt");

pub struct Object {
    pub title: &'static [u8],
    pub program: &'static [u8],
    pub params: &'static [u8],
    pub dir: &'static [u8],
    /// The Workplace Shell object ID.
    pub id: &'static [u8],
}

pub struct File {
    /// Path from the root of the boot drive.
    pub target: &'static [u8],
    /// Path from the root of the CD of the packed file holding it (or of
    /// the file itself).
    pub source: &'static [u8],
    /// The path it has in the packed file, when not `target` (the
    /// multimedia sounds are stored under \MMTEMP).
    pub stored: &'static [u8],
}

pub struct App {
    pub id: &'static [u8],
    pub name: &'static [u8],
    pub description: &'static [u8],
    pub needs: Vec<&'static [u8]>,
    pub objects: Vec<Object>,
    pub files: Vec<File>,
}

pub struct Category {
    pub title: &'static [u8],
    /// Desktop folder (program group) for its objects; None: no objects.
    pub folder: Option<&'static [u8]>,
    /// The folder's Workplace Shell object ID.
    pub folder_id: &'static [u8],
    pub apps: Vec<App>,
}

fn trim(mut s: &'static [u8]) -> &'static [u8] {
    while let [b' ' | b'\t' | b'\r', rest @ ..] = s {
        s = rest;
    }
    while let [rest @ .., b' ' | b'\t' | b'\r'] = s {
        s = rest;
    }
    s
}

fn fields(s: &'static [u8]) -> Vec<&'static [u8]> {
    s.split(|&c| c == b'|').map(trim).collect()
}

/// The catalogue.
pub fn load() -> Vec<Category> {
    let mut cats: Vec<Category> = Vec::new();
    for line in TEXT.split(|&c| c == b'\n') {
        let line = trim(line);
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let (word, rest) = match line.iter().position(|&c| c == b' ') {
            Some(i) => (&line[..i], trim(&line[i + 1..])),
            None => (line, &b""[..]),
        };
        let f = fields(rest);
        let get = |i: usize| f.get(i).copied().unwrap_or(b"");
        match word {
            b"category" => cats.push(Category {
                title: get(0),
                folder: if get(1) == b"-" || get(1).is_empty() { None } else { Some(get(1)) },
                folder_id: get(2),
                apps: Vec::new(),
            }),
            b"app" => {
                if let Some(c) = cats.last_mut() {
                    c.apps.push(App {
                        id: get(0),
                        name: get(1),
                        description: get(2),
                        needs: Vec::new(),
                        objects: Vec::new(),
                        files: Vec::new(),
                    });
                }
            }
            _ => {
                let Some(a) = cats.last_mut().and_then(|c| c.apps.last_mut()) else { continue };
                match word {
                    b"needs" => a.needs.push(get(0)),
                    b"object" => a.objects.push(Object { title: get(0), program: get(1), params: get(2), dir: get(3), id: get(4) }),
                    b"file" => a.files.push(File { target: get(0), source: get(1), stored: get(2) }),
                    _ => {}
                }
            }
        }
    }
    cats
}
