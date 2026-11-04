//! Unicode to the PC code pages OS/2 text uses (437 and 850), with
//! look-alikes for typographic characters web pages are full of.

const CP850: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{ad}±‗¾¶§÷¸°¨·¹³²■\u{a0}";
const CP437: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{a0}";

/// The process's code page (DosQueryCp), 850 if unknown.
pub fn current() -> u32 {
    #[cfg(target_os = "linux")]
    {
        unsafe extern "C" {
            #[link_name = "DOSCALLS.291"]
            fn DosQueryCp(cb: u32, cps: *mut u32, out: *mut u32) -> u32;
        }
        let mut cps = [0u32; 4];
        let mut n = 0;
        if unsafe { DosQueryCp(16, cps.as_mut_ptr(), &mut n) } == 0 && cps[0] != 0 {
            return cps[0];
        }
    }
    850
}

/// One character in code page `cp` (437 or 850; others use 850's table),
/// or a look-alike string.
pub fn encode_char(c: char, cp: u32, out: &mut Vec<u8>) {
    if (c as u32) < 0x80 {
        out.push(c as u8);
        return;
    }
    let table = if cp == 437 { CP437 } else { CP850 };
    if let Some(i) = table.chars().position(|t| t == c) {
        out.push(0x80 + i as u8);
        return;
    }
    let s: &str = match c {
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{2032}' => "'",
        '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{2033}' => "\"",
        '\u{2013}' | '\u{2010}' | '\u{2011}' | '\u{2212}' => "-",
        '\u{2014}' | '\u{2015}' => "--",
        '\u{2026}' => "...",
        '\u{2022}' | '\u{25cf}' | '\u{2023}' => "*",
        '\u{2122}' => "TM",
        '\u{20ac}' => "EUR",
        '\u{2190}' => "<-",
        '\u{2192}' => "->",
        '\u{2009}' | '\u{200a}' | '\u{2002}' | '\u{2003}' | '\u{202f}' | '\u{2007}' => " ",
        '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{ad}' => "",
        '\u{a0}' => " ",
        _ => "?",
    };
    out.extend_from_slice(s.as_bytes());
}

pub fn encode(s: &str, cp: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.chars() {
        encode_char(c, cp, &mut out);
    }
    out
}
