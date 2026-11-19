//! The BonusPak's parts as the window shows them: rs2's own names,
//! groups and descriptions (the CD's packages have only a title each),
//! with the Feature Installer package that installs each.

/// (feature ID, its directory under `FEATURE_\OS2BONUS`), in the order
/// IBM's BonusPak package lists them: the order the media directories'
/// .EAS files follow.
pub const LOAD_ORDER: &[(&[u8], &[u8])] = &[
    (b"BPASKPSP", b"BPASKPSP"),
    (b"BPCIM", b"BPCIM"),
    (b"BPFAXWORKS", b"BPFAXWOR"),
    (b"BPHAL", b"BPHAL"),
    (b"BPHPCOMMON", b"BPHPCOMM"),
    (b"BPHPJETCLIENT", b"BPHPJETC"),
    (b"BPHPJETSERVER", b"BPHPJETS"),
    (b"BPIBMWORKS", b"BPWORKS"),
    (b"BPMARKCOMMON", b"BPMARKCO"),
    (b"BPMARKNET", b"BPMARKNE"),
    (b"BPMARKVIS", b"BPMARKVI"),
    (b"BPRS2", b"BPRS2"),
    (b"BPVIDEOIN", b"BPVIDEOI"),
];

pub struct Part {
    pub id: &'static [u8],
    pub name: &'static [u8],
    pub description: &'static [u8],
}

pub struct Group {
    pub title: &'static [u8],
    pub parts: &'static [Part],
}

pub const GROUPS: &[Group] = &[
    Group {
        title: b"Productivity",
        parts: &[
            Part {
                id: b"BPIBMWORKS",
                name: b"IBM Works",
                description: b"An office suite: word processor, spreadsheet, charts, database, report writer and a personal information manager (appointments, phone book, to-do list, planner). Its folder goes in Programs. Runs on rs2.",
            },
            Part {
                id: b"BPASKPSP",
                name: b"Ask PSP",
                description: b"IBM's searchable collection of questions and answers about OS/2 Warp (CasePoint), with its books. Adds its directory to CONFIG.SYS's BOOKSHELF and HELP. Not yet tried on rs2.",
            },
        ],
    },
    Group {
        title: b"Communications",
        parts: &[
            Part {
                id: b"BPHAL",
                name: b"HyperACCESS Lite",
                description: b"A terminal program for modems and bulletin boards, with settings for many online services. Its object goes in Utilities. Not yet tried on rs2.",
            },
            Part {
                id: b"BPCIM",
                name: b"CompuServe Information Manager",
                description: b"The CompuServe client and its sign-up program. Not yet tried on rs2 (and the service itself is long gone).",
            },
            Part {
                id: b"BPFAXWORKS",
                name: b"FaxWorks",
                description: b"Sends and receives faxes with a fax modem; adds a fax printer driver and the FaxWorks object classes. Not yet tried on rs2.",
            },
        ],
    },
    Group {
        title: b"Printing",
        parts: &[
            Part {
                id: b"BPHPJETCLIENT",
                name: b"HP JetAdmin (client)",
                description: b"Sets up and watches HP network printers from this computer. Installed with the JetAdmin common files. Not yet tried on rs2.",
            },
            Part {
                id: b"BPHPJETSERVER",
                name: b"HP JetAdmin (server)",
                description: b"JetAdmin for a print server: adds its port driver and its programs to the spooler. Installed with the JetAdmin common files. Not yet tried on rs2.",
            },
            Part {
                id: b"BPHPCOMMON",
                name: b"HP JetAdmin common files",
                description: b"Files both JetAdmin parts use (it adds its directory to LIBPATH and HELP).",
            },
            Part {
                id: b"BPMARKVIS",
                name: b"Lexmark MarkVision",
                description: b"Watches and sets up Lexmark printers. Installed with the Lexmark common files. Not yet tried on rs2.",
            },
            Part {
                id: b"BPMARKNET",
                name: b"Lexmark MarkNet port driver",
                description: b"A printer port for Lexmark printers on the network (MarkNet adapters). Installed with the Lexmark common files. Not yet tried on rs2.",
            },
            Part {
                id: b"BPMARKCOMMON",
                name: b"Lexmark common files",
                description: b"Files MarkVision and the MarkNet port driver use (it adds its directory to LIBPATH and HELP).",
            },
        ],
    },
    Group {
        title: b"Multimedia",
        parts: &[Part {
            id: b"BPVIDEOIN",
            name: b"VideoIn",
            description: b"Records video from a capture card into AVI files, with the AVI File Utility and the VideoIn guide. Puts its programs in \\MMOS2. Not yet tried on rs2.",
        }],
    },
    Group {
        title: b"Service",
        parts: &[Part {
            id: b"BPRS2",
            name: b"Remote Support",
            description: b"Lets a support person see and work this computer from another one. It adds two device drivers and a CALL= line to CONFIG.SYS, which rs2 cannot load: install it only to look at its files.",
        }],
    },
];
