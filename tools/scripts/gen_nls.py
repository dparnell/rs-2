"""Generate kernel/src/nls_data.rs (code page tables, case maps,
collating sequences, DBCS lead byte ranges, country information) and
kernel/src/kbd/layouts.rs (keyboard layouts with dead keys and AltGr).

usage: python3 -I tools/scripts/gen_nls.py kernel/src
       python3 -I tools/scripts/gen_nls.py --check COUNTRY.SYS

Sources:
- Byte to Unicode mappings come from Python's codecs (generated from the
  Unicode Consortium's mapping files; Python is under the PSF licence).
- Case maps and collating weights are derived from those mappings with
  unicodedata (upper case; base letter of the canonical decomposition),
  with the few DOS-compatible rules described below.
- The country conventions (date order, separators, currency) and the
  keyboard layouts (national standard layouts as printed on the keys) are
  general knowledge, written down here; dead key compositions come from
  unicodedata (NFC).
--check compares the result with an OS/2 COUNTRY.SYS (for testing only;
nothing from that file is copied into the output).
"""
import codecs, struct, sys, unicodedata

# PC code pages: byte 0x00-0x1F and 0x7F show the PC graphic characters.
LOW = ' ☺☻♥♦♣♠•◘○◙♂♀♪♫☼►◄↕‼¶§▬↨↑↓→←∟↔▲▼'

# OS/2 code page number -> Python codec.
SBCS = {
    437: 'cp437', 850: 'cp850', 852: 'cp852', 855: 'cp855', 857: 'cp857',
    860: 'cp860', 861: 'cp861', 862: 'cp862', 863: 'cp863', 864: 'cp864',
    865: 'cp865', 866: 'cp866', 869: 'cp869', 874: 'cp874', 1125: 'cp1125',
    813: 'iso8859_7', 819: 'latin_1', 912: 'iso8859_2', 915: 'iso8859_5',
    921: 'iso8859_13', 1004: 'cp1252',
}

# Lead byte ranges of the double-byte code pages.
DBCS = {
    932: [(0x81, 0x9f), (0xe0, 0xfc)], 942: [(0x81, 0x9f), (0xe0, 0xfc)],
    943: [(0x81, 0x9f), (0xe0, 0xfc)], 934: [(0x81, 0xbf)], 944: [(0x81, 0xbf)],
    936: [(0x81, 0xfc)], 938: [(0x81, 0xfc)], 946: [(0x81, 0xfc)], 948: [(0x81, 0xfc)],
    949: [(0x81, 0xfe)], 950: [(0x81, 0xfe)], 1381: [(0x8c, 0xfe)], 1386: [(0x81, 0xfe)],
}

# Upper case: a letter takes its upper-case form when the code page has
# it. STRIP_ACCENTS: otherwise it loses its accent (the old PC rule);
# STRIP_ALL: upper-case letters never keep accents (French Canadian).
STRIP_ACCENTS = {437, 865}
STRIP_ALL = {863}
# Only Latin letters change case (the Greek letters of 437 are symbols).
LATIN_ONLY = {437, 850, 860, 861, 863, 865}
# Old PC exceptions: (code page, country or 0) -> {letter: upper case}.
UCASE_EXTRA = {(437, 0): {'é': 'E'}}
# Countries with their own upper-case table: (code page, country) -> rule.
UCASE_COUNTRY = {(437, 32): 'plain', (437, 46): 'plain', (437, 55): 'plain',
                 (437, 358): 'plain', (437, 31): 'strip_all'}
# Dotless i keeps its form except in the Turkish code page.
DOTLESS_I = {857}

# Collating, Western code pages: letters sort as their upper-case base
# letter; these fold further (ligatures, symbols that sort with their
# ASCII relatives).
FOLD = {
    'æ': 'A', 'Æ': 'A', 'ø': 'O', 'Ø': 'O', 'ð': 'D', 'Ð': 'D', 'ß': 'S',
    '¿': '?', '¡': '!', '«': '"', '»': '"', '№': '#',
    '¢': '$', '£': '$', '¥': '$', '₧': '$', 'ƒ': '$', '¤': '$',
}
# Country-specific collating: letters that sort after Z, or that keep a
# weight of their own.
COLLATE_AFTER_Z = {
    # Sweden, Finland: Å Ä Ö (Æ with Ä, Ø with Ö), Ü as Y
    46: {'Å': 0x5b, 'Ä': 0x5c, 'Æ': 0x5c, 'Ö': 0x5d, 'Ø': 0x5d, 'Õ': 0x5d, 'Ü': 0x59},
    358: {'Å': 0x5b, 'Ä': 0x5c, 'Æ': 0x5c, 'Ö': 0x5d, 'Ø': 0x5d, 'Õ': 0x5d, 'Ü': 0x59},
    # Denmark, Norway: Æ Ø Å
    45: {'Æ': 0x5b, 'Ä': 0x5b, 'Ø': 0x5c, 'Ö': 0x5c, 'Å': 0x5d, 'Ü': 0x59},
    47: {'Æ': 0x5b, 'Ä': 0x5b, 'Ø': 0x5c, 'Ö': 0x5c, 'Å': 0x5d, 'Ü': 0x59},
}
COLLATE_OWN = {34: 'ÇÑ'}  # Spain: Ç and Ñ are letters of their own

# Code pages whose letters get consecutive weights in alphabet order
# (groups of letters that sort alike): from 0x41 with the other bytes
# above 0x40 renumbered after them (Central European, Turkish), or from
# 0x80 after the Latin letters (Cyrillic, Greek).
CENTRAL = ['AÁ', 'Ä', 'Ą', 'Ă', 'Â', 'B', 'C', 'Ć', 'Č', 'Ç', 'D', 'Đ', 'Ď', 'E', 'É', 'Ě', 'Ę', 'Ë', 'F',
           'G', 'H', 'I', 'Í', 'Î', 'J', 'K', 'L', 'Ĺ', 'Ľ', 'Ł', 'M', 'N', 'Ň', 'Ń', 'O', 'Ó', 'Ô', 'Ö',
           'Ő', 'P', 'Q', 'R', 'Ŕ', 'Ř', 'S', 'ß', 'Š', 'Ş', 'Ś', 'T', 'Ť', 'Ţ', 'U', 'Ú', 'Ů', 'Ü', 'Ű',
           'V', 'W', 'X', 'Y', 'Ý', 'Z', 'Ž', 'Ź', 'Ż']
TURKISH = ['AÂÄÀÅÆÁÃª', 'B', 'C', 'Ç', 'D', 'EÉÊËÈ', 'F', 'G', 'Ğ', 'H', 'Iı', 'İÏÎÍÌ', 'J', 'K', 'L',
           'M', 'NÑ', 'OÔÒØÓºÕ', 'Ö', 'P', 'Q', 'R', 'Sß', 'Ş', 'T', 'UÛÙÚ', 'Ü', 'V', 'W', 'X', 'YŸ', 'Z']
CYRILLIC = ['А', 'Б', 'В', 'Г', 'Ґ', 'Д', 'Ђ', 'Ѓ', 'Е', 'Ё', 'Є', 'Ж', 'З', 'Ѕ', 'И', 'І', 'Ї', 'Й',
            'Ј', 'К', 'Л', 'Љ', 'М', 'Н', 'Њ', 'О', 'П', 'Р', 'С', 'Т', 'Ћ', 'Ќ', 'У', 'Ў', 'Ф', 'Х',
            'Ц', 'Ч', 'Џ', 'Ш', 'Щ', 'Ъ', 'Ы', 'Ь', 'Э', 'Ю', 'Я']
GREEK = ['ΑΆ', 'Β', 'Γ', 'Δ', 'ΕΈ', 'Ζ', 'ΗΉ', 'Θ', 'ΙΊΪ', 'Κ', 'Λ', 'Μ', 'Ν', 'Ξ', 'ΟΌ', 'Π', 'Ρ',
         'Σ', 'Τ', 'ΥΎΫ', 'Φ', 'Χ', 'Ψ', 'ΩΏ']
ALPHABET_FROM_A = {852: CENTRAL, 912: CENTRAL, 857: TURKISH}
ALPHABET_AFTER_LATIN = {855: CYRILLIC, 866: CYRILLIC, 1125: CYRILLIC, 915: CYRILLIC,
                        869: GREEK, 813: GREEK}

# Countries: code, date order (0 MDY, 1 DMY, 2 YMD), currency (preferred,
# ASCII fallback), thousands, decimal, date and time separators, currency
# format (0 = symbol first, 1 = after, 2 = first + blank, 3 = after +
# blank, 4 = replaces the decimal point), decimals, 24-hour clock, list
# separator, CODEPAGE defaults (primary, secondary).
COUNTRIES = [
    (1, 0, '$', '$', ',', '.', '-', ':', 0, 2, 0, ',', 437, 850),       # United States
    (2, 2, '$', '$', ' ', ',', '-', ':', 3, 2, 1, ';', 863, 850),       # Canada (French)
    (3, 0, 'RD$', 'RD$', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),   # Latin America
    (7, 1, 'руб.', 'rub.', ' ', ',', '.', ':', 3, 2, 1, ';', 866, 850),  # Russia
    (20, 2, 'ج.م.', 'EGP', '.', ',', '/', ':', 2, 3, 0, ';', 864, 850),  # Egypt
    (27, 2, 'R', 'R', '.', ',', '-', ':', 0, 2, 1, ',', 437, 850),       # South Africa
    (30, 1, 'Drs', 'Drs', '.', ',', '/', ':', 2, 2, 1, ',', 869, 437),   # Greece
    (31, 1, 'ƒ', 'f', '.', ',', '-', ':', 2, 2, 1, ';', 437, 850),       # Netherlands
    (32, 1, 'BEF', 'BEF', '.', ',', '/', ':', 2, 2, 1, ';', 850, 437),   # Belgium
    (33, 1, 'F', 'F', ' ', ',', '/', ':', 3, 2, 1, ';', 437, 850),       # France
    (34, 1, '₧', 'Pts', '.', ',', '/', ':', 3, 2, 1, ';', 850, 437),     # Spain
    (36, 2, 'Ft', 'Ft', '.', ',', '-', ':', 3, 2, 1, ',', 852, 850),     # Hungary
    (38, 2, 'Din', 'Din', '.', ',', '-', ':', 0, 2, 1, ',', 852, 850),   # Yugoslavia
    (39, 1, 'L.', 'L.', '.', ',', '/', ':', 0, 0, 1, ';', 437, 850),     # Italy
    (40, 1, 'Lei', 'Lei', '.', ',', '.', ':', 0, 2, 1, ';', 852, 850),   # Romania
    (41, 1, 'Fr', 'Fr', "'", '.', '.', '.', 2, 2, 1, ',', 850, 437),   # Switzerland
    (42, 2, 'Kč', 'Kc', ' ', ',', '-', ':', 3, 2, 1, ';', 852, 850),     # Czechoslovakia
    (43, 1, 'A S', 'A S', '.', ',', '.', '.', 2, 2, 1, ';', 850, 437),   # Austria
    (44, 1, '£', 'L', ',', '.', '-', ':', 0, 2, 0, ',', 437, 850),       # United Kingdom
    (45, 1, 'kr', 'kr', '.', ',', '-', '.', 2, 2, 1, ';', 865, 850),     # Denmark
    (46, 2, 'SEK', 'SEK', '.', ',', '-', '.', 2, 2, 1, ';', 437, 850),   # Sweden
    (47, 1, 'Kr', 'Kr', '.', ',', '.', '.', 2, 2, 1, ';', 865, 850),     # Norway
    (48, 2, 'zł', 'zl', '.', ',', '-', ':', 3, 2, 1, ';', 852, 850),     # Poland
    (49, 1, 'DM', 'DM', '.', ',', '.', '.', 2, 2, 1, ';', 437, 850),     # Germany
    (51, 1, 'S/', 'S/', '.', ',', '/', ':', 0, 2, 0, ';', 850, 437),     # Peru
    (52, 1, 'N$', 'N$', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),     # Mexico
    (54, 1, '$', '$', '.', ',', '/', ':', 0, 2, 1, ',', 850, 437),       # Argentina
    (55, 1, 'R$', 'R$', '.', ',', '/', ':', 2, 2, 1, ';', 850, 437),     # Brazil
    (57, 1, 'C$', 'C$', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),     # Colombia
    (58, 1, 'Bs', 'Bs', '.', ',', '/', ':', 0, 2, 0, ',', 850, 437),     # Venezuela
    (61, 1, '$', '$', ',', '.', '/', ':', 0, 2, 1, ',', 437, 850),       # Australia
    (62, 1, 'Rp', 'Rp', '.', ',', '/', ':', 0, 0, 1, ';', 850, 437),     # Indonesia
    (64, 1, '$', '$', ',', '.', '/', ':', 0, 2, 1, ',', 437, 850),       # New Zealand
    (65, 1, '$', '$', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),       # Singapore
    (66, 1, '฿', 'B', ',', '.', '-', ':', 0, 2, 1, ',', 874, 850),       # Thailand
    (81, 2, '\\', '\\', ',', '.', '-', ':', 0, 0, 1, ',', 932, 437),     # Japan
    (82, 2, '\\', '\\', ',', '.', '-', ':', 0, 0, 1, ',', 949, 437),     # Korea
    (86, 2, '\\', '\\', ',', '.', '-', ':', 0, 2, 1, ',', 1381, 437),    # China
    (88, 0, 'NT$', 'NT$', ',', '.', '-', ':', 0, 2, 1, ',', 950, 437),   # Taiwan
    (90, 1, 'TL', 'TL', '.', ',', '/', ':', 1, 0, 1, ';', 857, 850),     # Turkey
    (91, 1, 'Rs', 'Rs', ',', '.', '/', ':', 0, 2, 0, ';', 850, 437),     # India
    (92, 1, 'PKR', 'PKR', '.', ',', '/', ':', 0, 2, 1, ';', 850, 437),   # Pakistan
    (99, 2, '$', '$', ',', '.', '-', ':', 0, 2, 1, ',', 850, 437),       # Asia (English)
    (212, 2, 'د.م.', 'MAD', '.', ',', '-', ':', 2, 2, 1, ';', 864, 850),  # Morocco
    (213, 2, 'د.ج.', 'DZD', '.', ',', '-', ':', 2, 2, 0, ';', 864, 850),  # Algeria
    (216, 2, 'د.ت.', 'TND', '.', ',', '-', ':', 2, 3, 1, '/', 864, 850),  # Tunisia
    (351, 1, '$', '$', '.', ',', '/', ':', 4, 2, 1, ';', 860, 850),      # Portugal
    (352, 1, 'F', 'F', ' ', ',', '/', ':', 3, 2, 1, ';', 850, 437),      # Luxembourg
    (353, 1, '£', 'L', ',', '.', '-', ':', 0, 2, 0, ',', 850, 437),      # Ireland
    (354, 1, 'kr', 'kr', '.', ',', '-', ':', 3, 2, 1, ',', 861, 850),    # Iceland
    (355, 2, 'Lek', 'Lek', '.', ',', '-', ':', 0, 2, 1, ';', 850, 437),  # Albania
    (358, 1, 'mk', 'mk', ' ', ',', '.', '.', 3, 2, 1, ';', 437, 850),    # Finland
    (359, 1, 'Лв.', 'Lv.', ' ', ',', '.', ':', 3, 2, 1, ',', 855, 850),  # Bulgaria
    (370, 2, 'Lt', 'Lt', '.', ',', '.', ':', 3, 2, 1, ';', 921, 850),    # Lithuania
    (371, 3, 'Ls', 'Ls', ' ', ',', '.', ':', 3, 2, 1, ';', 921, 850),    # Latvia
    (372, 1, 'kr', 'kr', ' ', ',', '-', ':', 3, 2, 1, ';', 922, 850),    # Estonia
    (375, 1, 'р.', 'rub', ' ', ',', '.', ':', 3, 2, 1, ';', 866, 850),   # Belarus
    (380, 1, 'грн.', 'UAH.', ' ', ',', '.', ':', 3, 2, 1, ';', 1125, 866),  # Ukraine
    (381, 1, 'Дин', 'Din', '.', ',', '.', ':', 3, 2, 1, ';', 855, 850),  # Serbia
    (385, 1, 'Kn', 'Kn', '.', ',', '.', ':', 0, 2, 1, ',', 852, 850),    # Croatia
    (386, 1, 'SIT', 'SIT', '.', ',', '.', ':', 3, 2, 1, ';', 852, 850),  # Slovenia
    (387, 2, 'Din', 'Din', '.', ',', '-', ':', 0, 2, 1, ';', 852, 850),  # Bosnia
    (389, 2, 'Ден', 'Den', '.', ',', '-', ':', 0, 2, 1, ';', 855, 850),  # Macedonia
    (421, 2, 'Kč', 'Kc', ' ', ',', '-', ':', 3, 2, 1, ';', 852, 850),    # Czech Republic
    (422, 1, 'SKK', 'SKK', ' ', ',', '.', ':', 0, 2, 1, ',', 852, 850),  # Slovakia
    (502, 1, 'Q', 'Q', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),      # Guatemala
    (503, 0, 'C', 'C', ',', '.', '-', ':', 0, 2, 0, ';', 850, 437),      # El Salvador
    (504, 0, 'L', 'L', ',', '.', '-', ':', 0, 2, 0, ';', 850, 437),      # Honduras
    (505, 0, '$C', '$C', ',', '.', '-', ':', 0, 2, 0, ';', 850, 437),    # Nicaragua
    (506, 1, 'C', 'C', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),      # Costa Rica
    (507, 0, 'B', 'B', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),      # Panama
    (591, 1, 'Bs', 'Bs', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),    # Bolivia
    (593, 1, 'S/', 'S/', ',', '.', '/', ':', 0, 2, 0, ',', 850, 437),    # Ecuador
    (595, 1, 'G', 'G', '.', ',', '/', ':', 0, 2, 0, ',', 850, 437),      # Paraguay
    (598, 1, 'NU$', 'NU$', '.', ',', '/', ':', 2, 2, 0, ',', 850, 437),  # Uruguay
    (785, 1, 'SAR', 'SAR', '.', ',', '/', ',', 1, 2, 0, ',', 864, 850),  # Arabic
    (961, 2, 'د.ك.', 'LBP', '.', ',', '/', ':', 2, 2, 0, ';', 864, 850),  # Lebanon
    (962, 2, 'د.ا.', 'JOD', '.', ',', '/', ':', 2, 3, 0, ';', 864, 850),  # Jordan
    (963, 2, 'ل.س.', 'SYP', '.', ',', '/', ':', 2, 2, 0, ';', 864, 850),  # Syria
    (965, 2, 'د.ك.', 'KWD', '.', ',', '/', ':', 2, 3, 0, ';', 864, 850),  # Kuwait
    (966, 2, 'ر.س.', 'SAR', '.', ',', '/', ':', 2, 2, 0, ';', 864, 850),  # Saudi Arabia
    (967, 2, 'ر.ي.', 'YER', '.', ',', '/', ':', 2, 2, 0, ';', 864, 850),  # Yemen
    (968, 2, 'ر.ع.', 'OMR', '.', ',', '/', ':', 2, 2, 0, ';', 864, 850),  # Oman
    (971, 2, 'د.', 'AED', '.', ',', '/', ':', 2, 2, 0, ';', 864, 850),    # Emirates
    (972, 1, 'ש', 'NIS', ',', '.', '-', ':', 2, 2, 1, ',', 862, 850),    # Israel
    (974, 2, 'ر.ق.', 'QAR', '.', ',', '/', ':', 2, 2, 0, ';', 864, 850),  # Qatar
]


def to_unicode(cp):
    """256 characters (U+FFFD for unassigned bytes)."""
    t = []
    for b in range(256):
        try:
            ch = bytes([b]).decode(SBCS[cp])
        except UnicodeDecodeError:
            ch = '�'
        t.append(ch)
    return t


def reverse(u):
    r = {}
    for b in range(255, -1, -1):
        r[u[b]] = b
    return r


def strip(ch):
    n = unicodedata.normalize('NFD', ch)
    return n[0] if n else ch


def is_latin(ch):
    return 'LATIN' in unicodedata.name(ch, '')


def ucase(cp, country=0):
    if cp in DBCS:
        return list(range(128, 256))
    u = to_unicode(cp)
    rev = reverse(u)
    rule = UCASE_COUNTRY.get((cp, country), '')
    extra = {} if rule else UCASE_EXTRA.get((cp, 0), {})
    strip_all = cp in STRIP_ALL or rule == 'strip_all'
    out = []
    for b in range(128, 256):
        ch = u[b]
        r = b
        up = ch.upper()
        if ch in extra:
            r = rev[extra[ch]]
        elif ch == 'ı' and cp not in DOTLESS_I:
            pass
        elif len(up) == 1 and (cp not in LATIN_ONLY or is_latin(ch)):
            if strip_all and strip(up) in rev and strip(up) != ch:
                r = rev[strip(up)]
            elif up == ch:
                pass
            elif up in rev:
                r = rev[up]
            elif cp in STRIP_ACCENTS and strip(up) in rev and strip(up) != ch:
                r = rev[strip(up)]
        out.append(r)
    return out


def letter_group(ch, alphabet):
    up = ch.upper() if len(ch.upper()) == 1 else ch
    for i, g in enumerate(alphabet):
        if up in g:
            return i
    return None


def collate(cp, country=0):
    u = to_unicode(cp) if cp in SBCS else [chr(b) if b < 128 else '\ufffd' for b in range(256)]
    rev = reverse(u)
    if cp in ALPHABET_FROM_A:
        alpha = ALPHABET_FROM_A[cp]
        out = list(range(256))
        nxt = 0x41 + len(alpha)
        for b in range(0x41, 256):
            g = letter_group(u[b], alpha)
            if g is not None:
                out[b] = 0x41 + g
            else:
                out[b] = min(nxt, 255)
                nxt += 1
        return out
    script = ALPHABET_AFTER_LATIN.get(cp)
    special = COLLATE_AFTER_Z.get(country, {})
    own = COLLATE_OWN.get(country, '')
    out = []
    for b in range(256):
        ch = u[b]
        up = ch.upper() if len(ch.upper()) == 1 else ch
        if script and letter_group(ch, script) is not None:
            out.append(0x80 + letter_group(ch, script))
            continue
        if up in special:
            out.append(special[up])
            continue
        if up in own:
            out.append(rev.get(up, b))
            continue
        f = FOLD.get(ch) or strip(ch).upper()
        if len(f) == 1 and f.isascii() and (f.isalpha() or ch in FOLD):
            out.append(ord(f))
        elif len(f) == 1 and f != ch and f in rev and (cp not in LATIN_ONLY or is_latin(ch)):
            out.append(rev[f])
        else:
            out.append(b)
    return out


def encode(s, fallback, cp):
    try:
        if cp in SBCS:
            return s.encode(SBCS[cp])
        if s.isascii():
            return s.encode()
    except UnicodeEncodeError:
        pass
    return fallback.encode()


def ctry_info(row, cp):
    """The 38-byte (16-bit) COUNTRYINFO for a country and code page."""
    (code, date, cur, cur_ascii, th, dec, ds, ts, cf, dig, clock, ls, _, _) = row
    c = encode(cur, cur_ascii, cp)[:4]
    b = struct.pack('<HHH', code, cp, date) + c.ljust(5, b'\0')
    for s in (th, dec, ds, ts):
        b += s.encode() + b'\0'
    b += bytes([cf, dig, clock]) + b'\0' * 4 + ls.encode() + b'\0' + b'\0' * 10
    return b


def rust_list(vals, fmt):
    """Array items, 16 per line."""
    lines = []
    for i in range(0, len(vals), 16):
        lines.append('        ' + ', '.join(fmt % v for v in vals[i:i + 16]) + ',')
    return '\n' + '\n'.join(lines) + '\n    '


def rust_bytes(b):
    return rust_list(list(b), '0x%02x')


def generate(path):
    o = []
    o.append('//! Generated by tools/scripts/gen_nls.py: do not edit.\n')
    o.append('//! Byte to Unicode mappings from Python\'s codecs (Unicode Consortium')
    o.append('//! mapping data); case maps and collating weights derived from them.\n')
    o.append('#![allow(clippy::all)]\n')
    o.append('/// (code page, Unicode for bytes 0x00-0xFF; 0xFFFD = unassigned).')
    o.append('/// 437 and 850 come first (codepage.rs names them by index).')
    o.append('pub static CODEPAGES: &[(u16, [u16; 256])] = &[')
    for cp in [437, 850] + sorted(set(SBCS) - {437, 850}):
        u = to_unicode(cp)
        u = [LOW[b] if b < 0x20 else ('⌂' if b == 0x7f else u[b]) for b in range(256)]
        vals = rust_list([ord(ch) for ch in u], '0x%04x')
        o.append('    (%d, [%s]),' % (cp, vals))
    o.append('];\n')
    o.append('/// (country or 0 for the default, code page, upper case of bytes')
    o.append('/// 0x80-0xFF).')
    o.append('pub static UCASE: &[(u16, u16, [u8; 128])] = &[')
    for cp in sorted(list(SBCS) + list(DBCS)):
        o.append('    (0, %d, [%s]),' % (cp, rust_bytes(ucase(cp))))
        for (ucp, country) in sorted(UCASE_COUNTRY):
            if ucp == cp:
                o.append('    (%d, %d, [%s]),' % (country, cp, rust_bytes(ucase(cp, country))))
    o.append('];\n')
    o.append('/// (country or 0 for the default, code page, collating weights).')
    o.append('pub static COLLATE: &[(u16, u16, [u8; 256])] = &[')
    for cp in sorted(list(SBCS) + list(DBCS)):
        base = collate(cp)
        o.append('    (0, %d, [%s]),' % (cp, rust_bytes(base)))
        for country in sorted(set(COLLATE_AFTER_Z) | set(COLLATE_OWN)):
            t = collate(cp, country)
            if t != base:
                o.append('    (%d, %d, [%s]),' % (country, cp, rust_bytes(t)))
    o.append('];\n')
    o.append('/// (code page, DBCS lead byte ranges).')
    o.append('pub static DBCS: &[(u16, &[(u8, u8)])] = &[')
    for cp in sorted(DBCS):
        o.append('    (%d, &[%s]),' % (cp, ', '.join('(0x%02x, 0x%02x)' % r for r in DBCS[cp])))
    o.append('];\n')
    o.append('/// Country conventions; currency strings per code page come from')
    o.append('/// `currency` (Unicode) with `currency_ascii` as the fallback.')
    o.append('pub struct Country {')
    o.append('    pub code: u16,')
    o.append('    pub date_fmt: u8,')
    o.append('    pub currency: &\'static str,')
    o.append('    pub currency_ascii: &\'static str,')
    o.append('    pub thousands: u8,')
    o.append('    pub decimal: u8,')
    o.append('    pub date_sep: u8,')
    o.append('    pub time_sep: u8,')
    o.append('    pub currency_fmt: u8,')
    o.append('    pub decimals: u8,')
    o.append('    pub time_fmt: u8,')
    o.append('    pub list_sep: u8,')
    o.append('    pub codepages: [u16; 2],')
    o.append('}\n')
    o.append('pub static COUNTRIES: &[Country] = &[')
    for r in COUNTRIES:
        (code, date, cur, cur_ascii, th, dec, ds, ts, cf, dig, clock, ls, p1, p2) = r
        o.append('    Country { code: %d, date_fmt: %d, currency: %s, currency_ascii: %s, thousands: b%s, decimal: b%s, '
                 'date_sep: b%s, time_sep: b%s, currency_fmt: %d, decimals: %d, time_fmt: %d, list_sep: b%s, '
                 'codepages: [%d, %d] },' % (code, date, rs(cur), rs(cur_ascii), rc(th), rc(dec), rc(ds), rc(ts),
                                              cf, dig, clock, rc(ls), p1, p2))
    o.append('];')
    open(path, 'w').write('\n'.join(o) + '\n')


def rs(s):
    return '"' + s.replace('\\', '\\\\') + '"'


def rc(ch):
    return "'" + ch.replace('\\', '\\\\').replace("'", "\\'") + "'"


def check(path):
    """Compare with an IBM COUNTRY.SYS (DOS/OS2 format)."""
    d = open(path, 'rb').read()
    tab = struct.unpack_from('<I', d, 0x13)[0]
    n = struct.unpack_from('<H', d, tab)[0]
    p = tab + 2
    rows = {r[0]: r for r in COUNTRIES}
    stats = {'info': [0, 0], 'ucase': [0, 0], 'collate': [0, 0], 'dbcs': [0, 0]}
    for _ in range(n):
        size, ctry, cp, _, _, off = struct.unpack_from('<HHHHHI', d, p)
        p += size + 2
        if ctry not in rows or (cp not in SBCS and cp not in DBCS):
            continue
        cnt = struct.unpack_from('<H', d, off)[0]
        q = off + 2
        if cnt == 8:
            cnt, q = 6, off
        for _ in range(cnt):
            ssz, sid, soff = struct.unpack_from('<HHI', d, q)
            q += ssz
            ln = struct.unpack_from('<H', d, soff + 8)[0]
            ibm = d[soff + 10:soff + 10 + ln]
            if sid == 1:
                mine = ctry_info(rows[ctry], cp)
                key = 'info'
            elif sid == 2:
                mine = bytes(ucase(cp, ctry))
                key = 'ucase'
            elif sid == 6:
                mine = bytes(collate(cp, ctry))
                key = 'collate'
            elif sid == 7:
                mine = b''.join(bytes(r) for r in DBCS.get(cp, [])) + b'\0\0'
                key = 'dbcs'
            else:
                continue
            stats[key][1] += 1
            if mine == ibm:
                stats[key][0] += 1
            elif '-v' in sys.argv:
                diff = [i for i in range(min(len(ibm), len(mine))) if ibm[i] != mine[i]]
                print('%s %d/%d: %d bytes differ, first %s' % (key, ctry, cp, len(diff),
                      [(hex(i), hex(ibm[i]), hex(mine[i])) for i in diff[:8]]))
    for k, (ok, tot) in stats.items():
        print('%-8s %d of %d tables identical' % (k, ok, tot))



# ---- Keyboard layouts ----
#
# Keys in this order (scan code set 1): the key left of 1, the number row
# (1 .. the key left of Backspace), the Q row (12 keys), the A row (11),
# the key left of Enter on the A row (0x2B), the extra ISO key (0x56) and
# the Z row (10). Each level is a string of space-separated tokens: a
# character, "d" + a spacing accent for a dead key, or "_" for nothing.
KEY_SCANS = ([0x29] + list(range(0x02, 0x0e)) + list(range(0x10, 0x1c)) + list(range(0x1e, 0x29))
             + [0x2b, 0x56] + list(range(0x2c, 0x36)))

DEAD = {'´': '́', '`': '̀', '^': '̂', '¨': '̈', '~': '̃', 'ˇ': '̌',
        '°': '̊', '¸': '̧', '˛': '̨', '˝': '̋', '˘': '̆', '˙': '̇'}

# id, default code page, decimal key on the keypad, normal, shift, AltGr
LAYOUTS = [
    ('US', 437, '.',
     "` 1 2 3 4 5 6 7 8 9 0 - = q w e r t y u i o p [ ] a s d f g h j k l ; ' \\ \\ z x c v b n m , . /",
     '~ ! @ # $ % ^ & * ( ) _ + Q W E R T Y U I O P { } A S D F G H J K L : " | | Z X C V B N M < > ?',
     None),
    ('UK', 850, '.',
     "` 1 2 3 4 5 6 7 8 9 0 - = q w e r t y u i o p [ ] a s d f g h j k l ; ' # \\ z x c v b n m , . /",
     '¬ ! " £ $ % ^ & * ( ) _ + Q W E R T Y U I O P { } A S D F G H J K L : @ ~ | Z X C V B N M < > ?',
     '¦ _ _ _ € _ _ _ _ _ _ _ _ _ _ é _ _ _ ú í ó _ _ _ á _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _'),
    ('GR', 850, ',',
     'd^ 1 2 3 4 5 6 7 8 9 0 ß d´ q w e r t z u i o p ü + a s d f g h j k l ö ä # < y x c v b n m , . -',
     "° ! \" § $ % & / ( ) = ? d` Q W E R T Z U I O P Ü * A S D F G H J K L Ö Ä ' > Y X C V B N M ; : _",
     '_ _ ² ³ _ _ _ { [ ] } \\ _ @ _ € _ _ _ _ _ _ _ _ ~ _ _ _ _ _ _ _ _ _ _ _ _ | _ _ _ _ _ _ µ _ _ _'),
    ('FR', 850, '.',
     "² & é \" ' ( - è _ ç à ) = a z e r t y u i o p d^ $ q s d f g h j k l m ù * < w x c v b n , ; : !",
     '_ 1 2 3 4 5 6 7 8 9 0 ° + A Z E R T Y U I O P d¨ £ Q S D F G H J K L M % µ > W X C V B N ? . / §',
     '_ _ d~ # { [ | d` \\ ^ @ ] } _ _ € _ _ _ _ _ _ _ _ ¤ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _'),
    ('BE', 850, '.',
     "² & é \" ' ( § è ! ç à ) - a z e r t y u i o p d^ $ q s d f g h j k l m ù µ < w x c v b n , ; : =",
     '³ 1 2 3 4 5 6 7 8 9 0 ° _ A Z E R T Y U I O P d¨ * Q S D F G H J K L M % £ > W X C V B N ? . / +',
     '_ | @ # _ _ ^ _ _ { } _ _ _ _ € _ _ _ _ _ _ _ [ ] _ _ _ _ _ _ _ _ _ _ d´ d` \\ _ _ _ _ _ _ _ _ _ d~'),
    ('NL', 850, ',',
     '@ 1 2 3 4 5 6 7 8 9 0 / ° q w e r t y u i o p d¨ * a s d f g h j k l + d´ < ] z x c v b n m , . -',
     "§ ! \" # $ % & _ ( ) ' ? d~ Q W E R T Y U I O P d^ | A S D F G H J K L ± d` > [ Z X C V B N M ; : =",
     '¬ ¹ ² ³ ¼ ½ ¾ £ { } _ \\ d¸ _ _ € ¶ _ _ _ _ _ _ _ _ ß _ _ _ _ _ _ _ _ _ _ _ ¦ « » ¢ _ _ _ µ _ · _'),
    ('IT', 850, '.',
     "\\ 1 2 3 4 5 6 7 8 9 0 ' ì q w e r t y u i o p è + a s d f g h j k l ò à ù < z x c v b n m , . -",
     '| ! " £ $ % & / ( ) = ? ^ Q W E R T Y U I O P é * A S D F G H J K L ç ° § > Z X C V B N M ; : _',
     '_ _ _ _ _ € _ _ _ _ _ _ _ _ _ € _ _ _ _ _ _ _ [ ] _ _ _ _ _ _ _ _ _ @ # _ _ _ _ _ _ _ _ _ _ _ _'),
    ('SP', 850, '.',
     "º 1 2 3 4 5 6 7 8 9 0 ' ¡ q w e r t y u i o p d` + a s d f g h j k l ñ d´ ç < z x c v b n m , . -",
     'ª ! " · $ % & / ( ) = ? ¿ Q W E R T Y U I O P d^ * A S D F G H J K L Ñ d¨ Ç > Z X C V B N M ; : _',
     '\\ | @ # ~ € ¬ _ _ _ _ _ _ _ _ € _ _ _ _ _ _ _ [ ] _ _ _ _ _ _ _ _ _ _ { } _ _ _ _ _ _ _ _ _ _ _'),
    ('LA', 850, '.',
     "| 1 2 3 4 5 6 7 8 9 0 ' ¿ q w e r t y u i o p d´ + a s d f g h j k l ñ { } < z x c v b n m , . -",
     '° ! " # $ % & / ( ) = ? ¡ Q W E R T Y U I O P d¨ * A S D F G H J K L Ñ [ ] > Z X C V B N M ; : _',
     '¬ _ _ _ _ _ _ _ _ _ _ \\ _ @ _ _ _ _ _ _ _ _ _ _ ~ _ _ _ _ _ _ _ _ _ _ d^ d` _ _ _ _ _ _ _ _ _ _ _'),
    ('PO', 850, ',',
     "\\ 1 2 3 4 5 6 7 8 9 0 ' « q w e r t y u i o p + d´ a s d f g h j k l ç º d~ < z x c v b n m , . -",
     '| ! " # $ % & / ( ) = ? » Q W E R T Y U I O P * d` A S D F G H J K L Ç ª d^ > Z X C V B N M ; : _',
     '_ _ @ £ § € _ { [ ] } _ _ _ _ € _ _ _ _ _ _ _ d¨ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _'),
    ('SG', 850, '.',
     "§ 1 2 3 4 5 6 7 8 9 0 ' d^ q w e r t z u i o p ü d¨ a s d f g h j k l ö ä $ < y x c v b n m , . -",
     '° + " * ç % & / ( ) = ? d` Q W E R T Z U I O P è ! A S D F G H J K L é à £ > Y X C V B N M ; : _',
     '_ ¦ @ # ° § ¬ | ¢ _ _ d´ d~ _ _ € _ _ _ _ _ _ _ [ ] _ _ _ _ _ _ _ _ _ _ { } \\ _ _ _ _ _ _ _ _ _ _'),
    ('SF', 850, '.',
     "§ 1 2 3 4 5 6 7 8 9 0 ' d^ q w e r t z u i o p è d¨ a s d f g h j k l é à $ < y x c v b n m , . -",
     '° + " * ç % & / ( ) = ? d` Q W E R T Z U I O P ü ! A S D F G H J K L ö ä £ > Y X C V B N M ; : _',
     '_ ¦ @ # ° § ¬ | ¢ _ _ d´ d~ _ _ € _ _ _ _ _ _ _ [ ] _ _ _ _ _ _ _ _ _ _ { } \\ _ _ _ _ _ _ _ _ _ _'),
    ('SV', 850, ',',
     "§ 1 2 3 4 5 6 7 8 9 0 + d´ q w e r t y u i o p å d¨ a s d f g h j k l ö ä ' < z x c v b n m , . -",
     '½ ! " # ¤ % & / ( ) = ? d` Q W E R T Y U I O P Å d^ A S D F G H J K L Ö Ä * > Z X C V B N M ; : _',
     '_ _ @ £ $ € _ { [ ] } \\ _ _ _ € _ _ _ _ _ _ _ _ d~ _ _ _ _ _ _ _ _ _ _ _ _ | _ _ _ _ _ _ µ _ _ _'),
    ('SU', 850, ',',
     "§ 1 2 3 4 5 6 7 8 9 0 + d´ q w e r t y u i o p å d¨ a s d f g h j k l ö ä ' < z x c v b n m , . -",
     '½ ! " # ¤ % & / ( ) = ? d` Q W E R T Y U I O P Å d^ A S D F G H J K L Ö Ä * > Z X C V B N M ; : _',
     '_ _ @ £ $ € _ { [ ] } \\ _ _ _ € _ _ _ _ _ _ _ _ d~ _ _ _ _ _ _ _ _ _ _ _ _ | _ _ _ _ _ _ µ _ _ _'),
    ('DK', 850, ',',
     "½ 1 2 3 4 5 6 7 8 9 0 + d´ q w e r t y u i o p å d¨ a s d f g h j k l æ ø ' < z x c v b n m , . -",
     '§ ! " # ¤ % & / ( ) = ? d` Q W E R T Y U I O P Å d^ A S D F G H J K L Æ Ø * > Z X C V B N M ; : _',
     '_ _ @ £ $ € _ { [ ] } _ | _ _ € _ _ _ _ _ _ _ _ d~ _ _ _ _ _ _ _ _ _ _ _ _ \\ _ _ _ _ _ _ µ _ _ _'),
    ('NO', 850, ',',
     "| 1 2 3 4 5 6 7 8 9 0 + \\ q w e r t y u i o p å d¨ a s d f g h j k l ø æ ' < z x c v b n m , . -",
     '§ ! " # ¤ % & / ( ) = ? d` Q W E R T Y U I O P Å d^ A S D F G H J K L Ø Æ * > Z X C V B N M ; : _',
     '_ _ @ £ $ € _ { [ ] } _ d´ _ _ € _ _ _ _ _ _ _ _ d~ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ _ µ _ _ _'),
    ('IS', 850, ',',
     "d° 1 2 3 4 5 6 7 8 9 0 ö - q w e r t y u i o p ð ' a s d f g h j k l æ d´ + < z x c v b n m , . þ",
     'd¨ ! " # $ % & / ( ) = Ö _ Q W E R T Y U I O P Ð ? A S D F G H J K L Æ d´ * > Z X C V B N M ; : Þ',
     'd° _ _ _ _ € _ { [ ] } \\ _ @ _ € _ _ _ _ _ _ _ _ d~ _ _ _ _ _ _ _ _ _ _ d^ d` | _ _ _ _ _ _ µ _ _ _'),
    ('CF', 863, '.',
     '# 1 2 3 4 5 6 7 8 9 0 - = q w e r t y u i o p d^ d¸ a s d f g h j k l ; d` < « z x c v b n m , . é',
     '| ! " / $ % ? & * ( ) _ + Q W E R T Y U I O P d^ d¨ A S D F G H J K L : d` > » Z X C V B N M \' . É',
     '\\ ± @ £ ¢ ¤ ¬ ¦ ² ³ ¼ ½ ¾ _ _ _ _ _ _ _ _ § ¶ [ ] _ _ _ _ _ _ _ _ _ ~ { } ° _ _ _ _ _ _ µ ¯ _ d´'),
    ('TR', 857, ',',
     '" 1 2 3 4 5 6 7 8 9 0 * - q w e r t y u ı o p ğ ü a s d f g h j k l ş i , < z x c v b n m ö ç .',
     "é ! ' ^ + % & / ( ) = ? _ Q W E R T Y U I O P Ğ Ü A S D F G H J K L Ş İ ; > Z X C V B N M Ö Ç :",
     '< > £ # $ ½ _ { [ ] } \\ | @ _ € _ _ _ _ _ _ _ d¨ d~ æ ß _ _ _ _ _ _ _ d´ _ d` | _ _ _ _ _ _ _ _ _ _'),
    ('PL', 852, '.',
     "` 1 2 3 4 5 6 7 8 9 0 - = q w e r t y u i o p [ ] a s d f g h j k l ; ' \\ \\ z x c v b n m , . /",
     '~ ! @ # $ % ^ & * ( ) _ + Q W E R T Y U I O P { } A S D F G H J K L : " | | Z X C V B N M < > ?',
     '_ _ _ _ _ _ _ _ _ _ _ _ _ _ _ ę _ _ _ € _ ó _ _ _ ą ś _ _ _ _ _ _ ł _ _ _ _ ż ź ć _ _ ń _ _ _ _'),
]


def layout_keys(norm, shift, alt):
    def tok(t):
        if t == '_':
            return 0
        if len(t) == 2 and t[0] == 'd' and t[1] in DEAD:
            return 0x80000000 | ord(DEAD[t[1]])
        assert len(t) == 1, t
        return ord(t)
    rows = [norm.split(' '), shift.split(' '), (alt or '_').split(' ')]
    # AltGr rows may leave out trailing empty keys.
    rows[2] += ['_'] * (48 - len(rows[2]))
    for r in rows:
        assert len(r) == 48, (len(r), r)
    keys = []
    for i, scan in enumerate(KEY_SCANS):
        n, sh, a = (tok(rows[0][i]), tok(rows[1][i]), tok(rows[2][i]))
        # AltGr+Shift on a letter gives its capital.
        sa = 0
        if a and a < 0x80000000 and chr(a).isalpha() and len(chr(a).upper()) == 1:
            sa = ord(chr(a).upper())
        keys.append((scan, n, sh, a, sa))
    return keys


def compose_table():
    out = []
    for comb in sorted(set(DEAD.values())):
        for base in [chr(c) for c in range(0x20, 0x7f)] + ['ı']:
            r = unicodedata.normalize('NFC', base + comb)
            if len(r) == 1 and r != base:
                out.append((ord(comb), ord(base), ord(r)))
    return sorted(out)


def generate_layouts(path):
    o = ['//! Generated by tools/scripts/gen_nls.py: do not edit.', '//!',
         '//! Keyboard layouts: per key (scan code set 1) the characters for',
         '//! normal, Shift, AltGr and AltGr+Shift; 0x8000_0000 | combining',
         '//! accent marks a dead key.', '',
         'pub struct Layout {', "    pub id: &'static str,", '    pub codepage: u16,',
         '    pub decimal: u8,', '    pub altgr: bool,', "    pub keys: &'static [(u8, u32, u32, u32, u32)],", '}', '']
    o.append('pub static LAYOUTS: &[Layout] = &[')
    for (lid, cp, dec, n, sh, a) in LAYOUTS:
        keys = layout_keys(n, sh, a)
        o.append('    Layout {')
        o.append('        id: "%s",' % lid)
        o.append('        codepage: %d,' % cp)
        o.append("        decimal: b'%s'," % dec)
        o.append('        altgr: %s,' % ('true' if a else 'false'))
        o.append('        keys: &[')
        for k in keys:
            o.append('            (0x%02x, 0x%x, 0x%x, 0x%x, 0x%x),' % k)
        o.append('        ],')
        o.append('    },')
    o.append('];')
    o.append('')
    o.append('/// (combining accent, base character, composed character), sorted.')
    o.append('pub static COMPOSE: &[(u16, u16, u16)] = &[')
    t = compose_table()
    for i in range(0, len(t), 6):
        o.append('    ' + ' '.join('(0x%04x, 0x%04x, 0x%04x),' % e for e in t[i:i + 6]))
    o.append('];')
    o.append('')
    o.append('/// (combining accent, the spacing character typed for it alone).')
    o.append('pub static SPACING: &[(u16, u16)] = &[')
    for sp, comb in sorted(DEAD.items(), key=lambda kv: kv[1]):
        o.append('    (0x%04x, 0x%04x),' % (ord(comb), ord(sp)))
    o.append('];')
    open(path, 'w').write('\n'.join(o) + '\n')

if __name__ == '__main__':
    if sys.argv[1] == '--check':
        check(sys.argv[2])
    else:
        generate(sys.argv[1] + '/nls_data.rs')
        generate_layouts(sys.argv[1] + '/kbd/layouts.rs')
