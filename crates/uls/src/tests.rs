use super::*;
use alloc::string::ToString;
use alloc::vec;

fn s(u: &[u16]) -> String {
    char::decode_utf16(u.iter().copied()).map(|c| c.unwrap()).collect()
}

fn en_us() -> usize {
    find_system("en_US").unwrap()
}

#[test]
fn system_locales_have_every_item() {
    for i in 0..system_count() {
        for item in 1..=MAXITEM {
            let z = system_item_z(i, item);
            assert_eq!(z.last(), Some(&0));
        }
        assert_eq!(s(system_item(i, 100)), system_name(i));
        assert_eq!(item_value(system_item(i, 54)), Some(MAXITEM as i32));
    }
}

#[test]
fn lookup_by_name() {
    assert_eq!(find_system("EN_us"), Some(en_us()));
    assert_eq!(find_system("en_US.IBM-850"), Some(en_us()));
    assert_eq!(find_system("C"), find_system("POSIX"));
    assert_eq!(system_name(find_system("C").unwrap()), "posix");
    assert_eq!(find_system("xx_XX"), None);
    assert_eq!(item_by_name("sabbrevmonthname10"), Some(29));
    assert_eq!(ITEM_NAMES[LOCI_MAX - 1], "jEraStrings");
}

const LOCI_MAX: usize = MAXITEM;

#[test]
fn conventions_follow_country_data() {
    let de = find_system("de_DE").unwrap();
    assert_eq!(s(system_item(de, 44)), ",");
    assert_eq!(s(system_item(de, 45)), ".");
    assert_eq!(s(system_item(de, 2)), "%d.%m.%y");
    assert_eq!(s(system_item(de, 14)), "Montag");
    assert_eq!(s(system_item(de, 34)), "März");
    assert_eq!(item_value(system_item(de, 108)), Some(49));
    assert_eq!(item_value(system_item(de, 111)), Some(437));
    let us = en_us();
    assert_eq!(s(system_item(us, 2)), "%m/%d/%y");
    assert_eq!(item_value(system_item(us, 51)), Some(0x409));
    assert_eq!(item_category(44), LC_NUMERIC);
    assert_eq!(item_category(48), LC_MONETARY);
    assert_eq!(item_category(1), LC_TIME);
    assert_eq!(item_category(46), LC_MESSAGES);
}

#[test]
fn country_mapping() {
    assert_eq!(locale_for_country(1), Some(en_us()));
    assert_eq!(system_name(locale_for_country(49).unwrap()), "de_DE");
    assert_eq!(locale_for_country(9999), None);
}

#[test]
fn names() {
    assert_eq!(s(language_name("de", "en").unwrap()), "German");
    assert_eq!(s(language_name("de", "de").unwrap()), "Deutsch");
    assert_eq!(s(country_name("DE", "de").unwrap()), "Deutschland");
    assert_eq!(s(country_name("CH", "fr").unwrap()), "Suisse");
    assert_eq!(s(country_name("CH", "en").unwrap()), "Switzerland");
    assert!(language_name("xx", "en").is_none());
    assert!(country_name("XX", "en").is_none());
}

#[test]
fn user_locale_lifecycle() {
    let mut ul = UserLocales::default();
    let us = en_us();
    // An override of a system locale, as GLOCALE makes on its first start.
    assert_eq!(ul.make(&ucs("en_US"), "en_US", false), ULS_SUCCESS);
    assert_eq!(ul.make(&ucs("en_US"), "en_US", false), ULS_NOOP);
    assert_eq!(ul.make(&ucs("en_US"), "de_DE", false), ULS_BADOBJECT);
    assert_eq!(ul.make(&ucs("bad name"), "en_US", false), ULS_INVALID);
    assert_eq!(ul.make(&ucs("mine"), "xx_XX", false), ULS_NOMATCH);
    assert_eq!(ul.find("en_US"), Some(Locale::User(0)));
    // A new locale.
    assert_eq!(ul.make(&ucs("my_Own"), "de_DE", false), ULS_SUCCESS);
    assert_eq!(ul.make(&ucs("my_Own"), "fr_FR", false), ULS_INVALID);
    assert_eq!(ul.make(&ucs("my_Own"), "fr_FR", true), ULS_SUCCESS);
    let mine = ul.find("MY_OWN").unwrap();
    assert_eq!(s(ul.item(mine, 14, false)), "lundi");

    assert_eq!(ul.set_item("en_US", 3, &ucs("%I:%M %p")), ULS_SUCCESS);
    assert_eq!(s(ul.item(Locale::User(0), 3, false)), "%I:%M %p");
    assert_eq!(s(ul.item(Locale::User(0), 3, true)), s(system_item(us, 3)));
    assert_eq!(ul.set_item("de_DE", 3, &ucs("x")), ULS_BADOBJECT);
    assert_eq!(ul.set_item("nope", 3, &ucs("x")), ULS_NOMATCH);
    assert_eq!(ul.set_item("en_US", 0, &ucs("x")), ULS_INVALID);
    assert_eq!(ul.set_item("en_US", 137, &ucs("x")), ULS_INVALID);

    assert_eq!(ul.delete("de_DE"), ULS_INVALID);
    assert_eq!(ul.delete("nope"), ULS_NOMATCH);
    assert_eq!(ul.delete("my_own"), ULS_SUCCESS);
    assert_eq!(ul.find("my_own"), None);
}

#[test]
fn user_loc_round_trip() {
    let mut ul = UserLocales::default();
    ul.make(&ucs("en_US"), "en_US", false);
    ul.make(&ucs("Mine"), "fr_FR", false);
    ul.set_item("en_US", 48, &ucs("US$"));
    ul.set_item("en_US", 63, &ucs("/")); // same as the base: not written
    ul.set_item("Mine", 32, &ucs("janvier [1]"));
    ul.set_item("Mine", 13, &[0x44, 0xe9, 0x20ac, 0x22, 0x0a]);
    let text = ul.serialize();
    let t = String::from_utf8_lossy(&text).to_string();
    assert!(t.starts_with("[codepage 850]\r\n\r\n[locale en_US en_US]\r\n"), "{t}");
    assert!(t.contains("    sCurrency           = \"US$\"\r\n"), "{t}");
    assert!(!t.contains("sDate "), "{t}");
    assert!(t.contains("[locale Mine fr_FR]\r\n"), "{t}");
    assert!(t.contains("sMonthName1         = \"janvier [[1]\"\r\n"), "{t}");
    // é is 0x82 in code page 850; the euro sign and controls are hex.
    let line = text.windows(9).position(|w| w == b"sDayName7").unwrap();
    let rest = &text[line..];
    let end = rest.iter().position(|&b| b == b'\r').unwrap();
    assert_eq!(&rest[..end], b"sDayName7           = \"D\x82[20AC][22][0A]\"");

    let back = UserLocales::parse(&text);
    assert_eq!(back.list.len(), 2);
    assert_eq!(back.list[0].name, "en_US");
    assert_eq!(s(back.item(Locale::User(0), 48, false)), "US$");
    assert_eq!(s(back.item(Locale::User(1), 32, false)), "janvier [1]");
    assert_eq!(back.item(Locale::User(1), 13, false), &[0x44, 0xe9, 0x20ac, 0x22, 0x0a]);
    assert!(!back.changed);
}

#[test]
fn parse_is_forgiving() {
    let text = b"[codepage 437]\n[locale  x_Y   de_DE ]\n\tsDecimal = \",\"\n  iDigits=3\n bogus = 1\n[locale z_Z nowhere]\n sList = \";\"\n";
    let ul = UserLocales::parse(text);
    assert_eq!(ul.list.len(), 1);
    assert_eq!(ul.list[0].base, find_system("de_DE").unwrap());
    assert_eq!(s(ul.item(Locale::User(0), 72, false)), "3");
    // Byte 0x81 in code page 437 is ü.
    let ul = UserLocales::parse(b"[codepage 437]\r\n[locale a_B en_US]\r\n sCurrency = \"\x81\"\r\n");
    assert_eq!(ul.item(Locale::User(0), 48, false), &[0xfc]);
}

#[test]
fn locale_lists() {
    let mut ul = UserLocales::default();
    ul.make(&ucs("en_US"), "en_US", false);
    ul.make(&ucs("my_Own"), "de_DE", false);
    let names = |v: Vec<u16>| -> Vec<String> {
        let mut out = vec![];
        for part in v.split(|&c| c == 0) {
            if part.is_empty() {
                break;
            }
            out.push(s(part));
        }
        assert_eq!(&v[v.len() - 2..], &[0, 0]);
        out
    };
    let sys = names(ul.locale_list(1, 4096).unwrap());
    assert_eq!(sys.len(), system_count());
    assert!(sys.contains(&"de_CH".to_string()));
    assert_eq!(names(ul.locale_list(2, 4096).unwrap()), ["en_US", "my_Own"]);
    let all = names(ul.locale_list(0, 4096).unwrap());
    assert_eq!(all.len(), system_count() + 1);
    assert_eq!(all.last().unwrap(), "my_Own");
    assert_eq!(ul.locale_list(1, 20), Err(ULS_BUFFERFULL));
    // Exactly enough: "en_US\0my_Own\0\0".
    assert!(ul.locale_list(2, 14).is_ok());
    assert_eq!(ul.locale_list(2, 13), Err(ULS_BUFFERFULL));
}

#[test]
fn strftime_formats() {
    let us = en_us();
    let item = |n: usize| system_item(us, n);
    let tm = Tm { sec: 5, min: 4, hour: 15, mday: 7, mon: 1, year: 101, wday: 3, yday: 37, isdst: 0 };
    let f = |fmt: &str| s(&strftime(&item, &ucs(fmt), &tm));
    assert_eq!(f("%c"), "Wed  7 Feb 15:04:05 2001");
    assert_eq!(f("%x %X"), "02/07/01 15:04:05");
    assert_eq!(f("%A, %B %d, %Y"), "Wednesday, February 07, 2001");
    assert_eq!(f("%I:%M %p|%j|%y|%C|%%|%u|%w"), "03:04 PM|038|01|20|%|3|3");
    assert_eq!(f("%U %W"), "05 06");
    let de = find_system("de_DE").unwrap();
    let item = |n: usize| system_item(de, n);
    assert_eq!(s(&strftime(&item, system_item(de, 96), &tm)), "Mittwoch, 07. Februar 2001");
}

#[test]
fn lconv_positions_follow_ibm_table() {
    assert_eq!(lconv_positions(0, 0), [1, 0, 1, 0, 1, 0, 0]);
    assert_eq!(lconv_positions(3, 8), [0, 1, 0, 1, 1, 2, 3]);
}

#[test]
fn char_types() {
    let a = char_type(b'a' as u16);
    assert_eq!(a.itype & (CT_LOWER | CT_ALPHA | CT_ALNUM | CT_XDIGIT | CT_ASCII | CT_PRINT | CT_GRAPH),
               CT_LOWER | CT_ALPHA | CT_ALNUM | CT_XDIGIT | CT_ASCII | CT_PRINT | CT_GRAPH);
    assert_eq!((a.bidi, a.charset), (1, 2));
    let d = char_type(b'7' as u16);
    assert_ne!(d.itype & CT_DIGIT, 0);
    let sp = char_type(b' ' as u16);
    assert_eq!(sp.itype & (CT_SPACE | CT_BLANK | CT_GRAPH), CT_SPACE | CT_BLANK);
    assert_ne!(char_type(b',' as u16).itype & CT_PUNCT, 0);
    let alef = char_type(0x5d0);
    assert_eq!((alef.bidi, alef.charset), (2, 6));
    assert_eq!(char_type(0x627).charset, 4);
    assert_ne!(char_type(0xe9).codepage & 2, 0);
    assert_eq!(to_upper(0xe9), 0xc9);
    assert_eq!(to_lower(b'Q' as u16), b'q' as u16);
    assert_eq!(to_upper(0xdf), 0xdf);
}

#[test]
fn code_pages() {
    assert_eq!(cp_to_ucs(850, 0x82), 0xe9);
    assert_eq!(ucs_to_cp(850, 0xe9), Some(0x82));
    assert_eq!(cp_to_ucs(850, 0x0a), 0x0a);
    assert_eq!(ucs_to_cp(850, 0x20ac), None);
    assert!(dbcs_ranges(932).is_some());
}
