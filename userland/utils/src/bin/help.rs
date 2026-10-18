//! HELP: help for commands and messages.
//!
//! ```text
//! HELP                 overview of the commands
//! HELP command         one line about a command
//! HELP SYSnnnn | nnnn  the text of message nnnn (and its explanation)
//! ```
//! Message text comes from OSO001.MSG and the explanation from
//! OSO001H.MSG through DosGetMessage, when those files are installed.
#![no_std]
#![no_main]
extern crate alloc;
use alloc::string::String;
use os2::*;
use utils::*;
os2::entry!(main);

const COMMANDS: &[(&str, &str)] = &[
    ("ATTRIB", "Shows or changes file attributes."),
    ("CALL", "Runs a batch file from another batch file."),
    ("CD", "Shows or changes the current directory (also CHDIR)."),
    ("CHKDSK", "Checks a disk and reports its use; /F repairs FAT volumes."),
    ("CLS", "Clears the screen."),
    ("COPY", "Copies files."),
    ("DATE", "Shows or sets the date."),
    ("DEL", "Deletes files (also ERASE)."),
    ("DIR", "Lists the files in a directory."),
    ("ECHO", "Shows messages, or turns command echoing on or off."),
    ("EXIT", "Ends the command interpreter."),
    ("FIND", "Searches files for a string."),
    ("FOR", "Runs a command for each file in a set."),
    ("FORMAT", "Creates a FAT or HPFS file system on a disk."),
    ("GOTO", "Continues a batch file at a label."),
    ("HELP", "Shows help for commands and messages."),
    ("IF", "Runs a command depending on a condition."),
    ("INSTALL", "Installs rs2 on a hard disk."),
    ("LABEL", "Shows or changes the volume label of a disk."),
    ("MD", "Creates a directory (also MKDIR)."),
    ("MODE", "Shows or sets the screen mode."),
    ("MORE", "Shows output one screen at a time."),
    ("MOVE", "Moves or renames files."),
    ("PATH", "Shows or sets the search path for programs."),
    ("PAUSE", "Waits for a key in a batch file."),
    ("PROMPT", "Changes the command prompt."),
    ("RD", "Removes a directory (also RMDIR)."),
    ("REN", "Renames files (also RENAME)."),
    ("SET", "Shows or sets environment variables."),
    ("SORT", "Sorts lines of input."),
    ("START", "Starts a program in another session."),
    ("TIME", "Shows or sets the time."),
    ("TREE", "Shows the directory structure of a drive."),
    ("TYPE", "Shows the contents of text files."),
    ("VER", "Shows the system version."),
    ("VOL", "Shows the volume label and serial number of a disk."),
    ("XCOPY", "Copies files and directory trees."),
];

fn main(s: &os2::Startup) -> u32 {
    let a = args(s);
    let Some(topic) = a.first().map(|t| t.to_ascii_uppercase()) else {
        overview();
        return 0;
    };
    let digits = topic.strip_prefix("SYS").unwrap_or(&topic);
    if !digits.is_empty() && digits.len() <= 4 && digits.bytes().all(|b| b.is_ascii_digit()) {
        return message(digits.parse().unwrap_or(0));
    }
    match topic.as_str() {
        "ON" | "OFF" => {
            outln("HELP ON and HELP OFF (the help line in the prompt) are not available.");
            0
        }
        t => match COMMANDS.iter().find(|c| c.0 == t) {
            Some((name, text)) => {
                outln(&alloc::format!("{name}: {text}"));
                0
            }
            None => {
                outln(&alloc::format!("No help is available for {t}.  Type HELP for a list of commands."));
                1
            }
        },
    }
}

fn overview() {
    outln("rs2 commands (type HELP command or HELP message-number):\r\n");
    let w = COMMANDS.iter().map(|c| c.0.len()).max().unwrap_or(8) + 2;
    for (name, text) in COMMANDS {
        outln(&alloc::format!("  {name:w$}{text}"));
    }
    outln("\r\nAlso: SHIFT SETLOCAL ENDLOCAL REM in batch files (.CMD); operators");
    outln("< > >> 2> 2>&1 | & && || on the command line.");
}

fn get(num: u32, file: &[u8]) -> Result<String, u32> {
    let mut buf = alloc::vec![0u8; 4096];
    let mut n = 0;
    let rc = unsafe { DosGetMessage(core::ptr::null(), 0, buf.as_mut_ptr(), buf.len() as u32, num, file.as_ptr(), &mut n) };
    if rc == 0 { Ok(buf[..n as usize].iter().map(|&b| b as char).collect()) } else { Err(rc) }
}

fn message(num: u32) -> u32 {
    let text = match get(num, b"OSO001.MSG\0") {
        Ok(t) => t,
        // ERROR_MR_UN_ACC_MSGF
        Err(318) => {
            outln(&alloc::format!("No help is available for SYS{num:04}: the message file OSO001.MSG is not installed."));
            return 1;
        }
        Err(_) => {
            outln(&alloc::format!("There is no message SYS{num:04}."));
            return 1;
        }
    };
    out("\r\n");
    out(&text);
    if let Ok(h) = get(num, b"OSO001H.MSG\0") {
        out("\r\n");
        out(&h);
    }
    0
}
