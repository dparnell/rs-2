"""Generate crates/uls/src/data.rs: rs2's system locales (what IBM keeps
in LOCALE.DLL), one UCS-2 string per locale item (LOCI_* 1-136).

usage: python3 -I tools/scripts/gen_locales.py crates/uls/src/data.rs

Sources: the country conventions (date order, separators, currency,
clock, code pages) are gen_nls.py's COUNTRIES, so the locales agree with
rs2's COUNTRY.SYS data; day and month names, language and country names,
ISO and Windows identifiers are general knowledge, written down here.
Nothing comes from IBM's locale files.
"""
import importlib.util, os, sys

sys.dont_write_bytecode = True

here = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location('gen_nls', os.path.join(here, 'gen_nls.py'))
gen_nls = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen_nls)
COUNTRY = {r[0]: r for r in gen_nls.COUNTRIES}

MAXITEM = 136

# Languages: English name, native name, Sunday-first day names,
# abbreviations, month names, abbreviations, yes, no, AM, PM, long date
# format (strftime), its Windows form, Windows language letters prefix.
LANGS = {
    'en': ('English', 'English',
           'Sunday Monday Tuesday Wednesday Thursday Friday Saturday',
           'Sun Mon Tue Wed Thu Fri Sat',
           'January February March April May June July August September October November December',
           'Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec',
           'yes', 'no', 'AM', 'PM'),
    'fr': ('French', 'français',
           'dimanche lundi mardi mercredi jeudi vendredi samedi',
           'dim lun mar mer jeu ven sam',
           'janvier février mars avril mai juin juillet août septembre octobre novembre décembre',
           'jan fév mar avr mai jun jul aoû sep oct nov déc',
           'oui', 'non', '', ''),
    'de': ('German', 'Deutsch',
           'Sonntag Montag Dienstag Mittwoch Donnerstag Freitag Samstag',
           'So Mo Di Mi Do Fr Sa',
           'Januar Februar März April Mai Juni Juli August September Oktober November Dezember',
           'Jan Feb Mär Apr Mai Jun Jul Aug Sep Okt Nov Dez',
           'ja', 'nein', '', ''),
    'it': ('Italian', 'italiano',
           'domenica lunedì martedì mercoledì giovedì venerdì sabato',
           'dom lun mar mer gio ven sab',
           'gennaio febbraio marzo aprile maggio giugno luglio agosto settembre ottobre novembre dicembre',
           'gen feb mar apr mag giu lug ago set ott nov dic',
           'sì', 'no', '', ''),
    'es': ('Spanish', 'español',
           'domingo lunes martes miércoles jueves viernes sábado',
           'dom lun mar mié jue vie sáb',
           'enero febrero marzo abril mayo junio julio agosto septiembre octubre noviembre diciembre',
           'ene feb mar abr may jun jul ago sep oct nov dic',
           'sí', 'no', 'a.m.', 'p.m.'),
    'pt': ('Portuguese', 'português',
           'domingo segunda-feira terça-feira quarta-feira quinta-feira sexta-feira sábado',
           'dom seg ter qua qui sex sáb',
           'janeiro fevereiro março abril maio junho julho agosto setembro outubro novembro dezembro',
           'jan fev mar abr mai jun jul ago set out nov dez',
           'sim', 'não', '', ''),
    'nl': ('Dutch', 'Nederlands',
           'zondag maandag dinsdag woensdag donderdag vrijdag zaterdag',
           'zo ma di wo do vr za',
           'januari februari maart april mei juni juli augustus september oktober november december',
           'jan feb mrt apr mei jun jul aug sep okt nov dec',
           'ja', 'nee', '', ''),
    'da': ('Danish', 'dansk',
           'søndag mandag tirsdag onsdag torsdag fredag lørdag',
           'søn man tir ons tor fre lør',
           'januar februar marts april maj juni juli august september oktober november december',
           'jan feb mar apr maj jun jul aug sep okt nov dec',
           'ja', 'nej', '', ''),
    'sv': ('Swedish', 'svenska',
           'söndag måndag tisdag onsdag torsdag fredag lördag',
           'sön mån tis ons tor fre lör',
           'januari februari mars april maj juni juli augusti september oktober november december',
           'jan feb mar apr maj jun jul aug sep okt nov dec',
           'ja', 'nej', '', ''),
    'no': ('Norwegian', 'norsk',
           'søndag mandag tirsdag onsdag torsdag fredag lørdag',
           'søn man tir ons tor fre lør',
           'januar februar mars april mai juni juli august september oktober november desember',
           'jan feb mar apr mai jun jul aug sep okt nov des',
           'ja', 'nei', '', ''),
    'fi': ('Finnish', 'suomi',
           'sunnuntai maanantai tiistai keskiviikko torstai perjantai lauantai',
           'su ma ti ke to pe la',
           'tammikuu helmikuu maaliskuu huhtikuu toukokuu kesäkuu heinäkuu elokuu syyskuu lokakuu marraskuu joulukuu',
           'tammi helmi maalis huhti touko kesä heinä elo syys loka marras joulu',
           'kyllä', 'ei', '', ''),
    'is': ('Icelandic', 'íslenska',
           'sunnudagur mánudagur þriðjudagur miðvikudagur fimmtudagur föstudagur laugardagur',
           'sun mán þri mið fim fös lau',
           'janúar febrúar mars apríl maí júní júlí ágúst september október nóvember desember',
           'jan feb mar apr maí jún júl ágú sep okt nóv des',
           'já', 'nei', '', ''),
    'pl': ('Polish', 'polski',
           'niedziela poniedziałek wtorek środa czwartek piątek sobota',
           'nie pon wto śro czw pią sob',
           'styczeń luty marzec kwiecień maj czerwiec lipiec sierpień wrzesień październik listopad grudzień',
           'sty lut mar kwi maj cze lip sie wrz paź lis gru',
           'tak', 'nie', '', ''),
    'cs': ('Czech', 'čeština',
           'neděle pondělí úterý středa čtvrtek pátek sobota',
           'ne po út st čt pá so',
           'leden únor březen duben květen červen červenec srpen září říjen listopad prosinec',
           'led úno bře dub kvě čvn čvc srp zář říj lis pro',
           'ano', 'ne', 'dop.', 'odp.'),
    'sk': ('Slovak', 'slovenčina',
           'nedeľa pondelok utorok streda štvrtok piatok sobota',
           'ne po ut st št pi so',
           'január február marec apríl máj jún júl august september október november december',
           'jan feb mar apr máj jún júl aug sep okt nov dec',
           'áno', 'nie', '', ''),
    'hu': ('Hungarian', 'magyar',
           'vasárnap hétfő kedd szerda csütörtök péntek szombat',
           'V H K Sze Cs P Szo',
           'január február március április május június július augusztus szeptember október november december',
           'jan febr márc ápr máj jún júl aug szept okt nov dec',
           'igen', 'nem', 'de.', 'du.'),
    'ru': ('Russian', 'русский',
           'воскресенье понедельник вторник среда четверг пятница суббота',
           'Вс Пн Вт Ср Чт Пт Сб',
           'январь февраль март апрель май июнь июль август сентябрь октябрь ноябрь декабрь',
           'янв фев мар апр май июн июл авг сен окт ноя дек',
           'да', 'нет', '', ''),
    'el': ('Greek', 'ελληνικά',
           'Κυριακή Δευτέρα Τρίτη Τετάρτη Πέμπτη Παρασκευή Σάββατο',
           'Κυρ Δευ Τρι Τετ Πεμ Παρ Σαβ',
           'Ιανουάριος Φεβρουάριος Μάρτιος Απρίλιος Μάιος Ιούνιος Ιούλιος Αύγουστος Σεπτέμβριος Οκτώβριος Νοέμβριος Δεκέμβριος',
           'Ιαν Φεβ Μαρ Απρ Μαϊ Ιουν Ιουλ Αυγ Σεπ Οκτ Νοε Δεκ',
           'ναι', 'όχι', 'πμ', 'μμ'),
    'tr': ('Turkish', 'Türkçe',
           'Pazar Pazartesi Salı Çarşamba Perşembe Cuma Cumartesi',
           'Paz Pzt Sal Çar Per Cum Cmt',
           'Ocak Şubat Mart Nisan Mayıs Haziran Temmuz Ağustos Eylül Ekim Kasım Aralık',
           'Oca Şub Mar Nis May Haz Tem Ağu Eyl Eki Kas Ara',
           'evet', 'hayır', 'ÖÖ', 'ÖS'),
    'ja': ('Japanese', '日本語',
           '日曜日 月曜日 火曜日 水曜日 木曜日 金曜日 土曜日',
           '日 月 火 水 木 金 土',
           '1月 2月 3月 4月 5月 6月 7月 8月 9月 10月 11月 12月',
           '1月 2月 3月 4月 5月 6月 7月 8月 9月 10月 11月 12月',
           'はい', 'いいえ', '午前', '午後'),
    'ko': ('Korean', '한국어',
           '일요일 월요일 화요일 수요일 목요일 금요일 토요일',
           '일 월 화 수 목 금 토',
           '1월 2월 3월 4월 5월 6월 7월 8월 9월 10월 11월 12월',
           '1월 2월 3월 4월 5월 6월 7월 8월 9월 10월 11월 12월',
           '예', '아니오', '오전', '오후'),
    'zh': ('Chinese', '中文',
           '星期日 星期一 星期二 星期三 星期四 星期五 星期六',
           '日 一 二 三 四 五 六',
           '一月 二月 三月 四月 五月 六月 七月 八月 九月 十月 十一月 十二月',
           '1月 2月 3月 4月 5月 6月 7月 8月 9月 10月 11月 12月',
           '是', '否', '上午', '下午'),
}

# Long date (strftime) per language; an entry per locale overrides it.
LONG_DATE = {
    'en': '%A, %d %B %Y', 'fr': '%A %d %B %Y', 'de': '%A, %d. %B %Y', 'it': '%A %d %B %Y',
    'es': '%A %d de %B de %Y', 'pt': '%A, %d de %B de %Y', 'nl': '%A %d %B %Y', 'da': '%A den %d. %B %Y',
    'sv': '%A den %d %B %Y', 'no': '%A %d. %B %Y', 'fi': '%A %d. %B %Y', 'is': '%A, %d. %B %Y',
    'pl': '%A, %d %B %Y', 'cs': '%A, %d. %B %Y', 'sk': '%A, %d. %B %Y', 'hu': '%Y. %B %d., %A',
    'ru': '%A, %d %B %Y', 'el': '%A, %d %B %Y', 'tr': '%d %B %Y %A', 'ja': '%Y年%m月%d日', 'ko': '%Y년 %m월 %d일',
    'zh': '%Y年%m月%d日',
}

# Per script: ISO code page name, Windows (ANSI), Mac and EBCDIC code
# pages, the locale whose names are in Latin 1.
SCRIPT = {
    'latin1': ('ISO8859-1', 1252, 10000),
    'latin2': ('ISO8859-2', 1250, 10029),
    'cyrillic': ('ISO8859-5', 1251, 10007),
    'greek': ('ISO8859-7', 1253, 10006),
    'turkish': ('ISO8859-9', 1254, 10081),
    'ja': ('IBM-943', 932, 10001),
    'ko': ('IBM-949', 949, 10003),
    'zh_CN': ('IBM-1386', 936, 10008),
    'zh_TW': ('IBM-950', 950, 10002),
}
LANG_SCRIPT = {'pl': 'latin2', 'cs': 'latin2', 'sk': 'latin2', 'hu': 'latin2', 'ru': 'cyrillic',
               'el': 'greek', 'tr': 'turkish', 'ja': 'ja', 'ko': 'ko'}

# Locales: name, ISO 3166 alpha-3, English country name, native country
# name, telephone country code (gen_nls.COUNTRIES), Windows LCID, Windows
# language letters, keyboard, international currency symbol, EBCDIC code
# page, overrides {item: value}.
LOCALES = [
    ('en_US', 'USA', 'United States', 'United States', 1, 0x0409, 'ENU', 'US', 'USD', 37,
     {1: '%a %e %b %H:%M:%S %Y', 2: '%m/%d/%y', 63: '/', 124: 'M/d/yy', 96: '%A, %B %d, %Y', 125: 'dddd, MMMM dd, yyyy', 70: 1, 71: 1, 81: 6}),
    ('en_GB', 'GBR', 'United Kingdom', 'United Kingdom', 44, 0x0809, 'ENG', 'UK', 'GBP', 285, {}),
    ('en_AU', 'AUS', 'Australia', 'Australia', 61, 0x0C09, 'ENA', 'US', 'AUD', 37, {}),
    ('en_CA', 'CAN', 'Canada', 'Canada', 1, 0x1009, 'ENC', 'US', 'CAD', 37,
     {2: '%d/%m/%y', 74: 1, 81: 6, 96: '%B %d, %Y'}),
    ('en_NZ', 'NZL', 'New Zealand', 'New Zealand', 64, 0x1409, 'ENZ', 'US', 'NZD', 37, {}),
    ('en_IE', 'IRL', 'Ireland', 'Ireland', 353, 0x1809, 'ENI', 'UK', 'IEP', 285, {}),
    ('en_ZA', 'ZAF', 'South Africa', 'South Africa', 27, 0x1C09, 'ENS', 'US', 'ZAR', 37, {}),
    ('fr_FR', 'FRA', 'France', 'France', 33, 0x040C, 'FRA', 'FR', 'FRF', 297, {}),
    ('fr_BE', 'BEL', 'Belgium', 'Belgique', 32, 0x080C, 'FRB', 'BE', 'BEF', 500, {}),
    ('fr_CA', 'CAN', 'Canada', 'Canada', 2, 0x0C0C, 'FRC', 'CF', 'CAD', 37, {}),
    ('fr_CH', 'CHE', 'Switzerland', 'Suisse', 41, 0x100C, 'FRS', 'SF', 'CHF', 500, {}),
    ('de_DE', 'DEU', 'Germany', 'Deutschland', 49, 0x0407, 'DEU', 'GR', 'DEM', 273, {}),
    ('de_AT', 'AUT', 'Austria', 'Österreich', 43, 0x0C07, 'DEA', 'GR', 'ATS', 273, {}),
    ('de_CH', 'CHE', 'Switzerland', 'Schweiz', 41, 0x0807, 'DES', 'SG', 'CHF', 500, {}),
    ('it_IT', 'ITA', 'Italy', 'Italia', 39, 0x0410, 'ITA', 'IT', 'ITL', 280, {}),
    ('it_CH', 'CHE', 'Switzerland', 'Svizzera', 41, 0x0810, 'ITS', 'SF', 'CHF', 500, {}),
    ('es_ES', 'ESP', 'Spain', 'España', 34, 0x0C0A, 'ESN', 'SP', 'ESP', 284, {}),
    ('es_MX', 'MEX', 'Mexico', 'México', 52, 0x080A, 'ESM', 'LA', 'MXN', 284, {}),
    ('es_AR', 'ARG', 'Argentina', 'Argentina', 54, 0x2C0A, 'ESS', 'LA', 'ARS', 284, {}),
    ('pt_PT', 'PRT', 'Portugal', 'Portugal', 351, 0x0816, 'PTG', 'PO', 'PTE', 37, {}),
    ('pt_BR', 'BRA', 'Brazil', 'Brasil', 55, 0x0416, 'PTB', 'BR', 'BRL', 37, {}),
    ('nl_NL', 'NLD', 'Netherlands', 'Nederland', 31, 0x0413, 'NLD', 'NL', 'NLG', 37, {}),
    ('nl_BE', 'BEL', 'Belgium', 'België', 32, 0x0813, 'NLB', 'BE', 'BEF', 500, {}),
    ('da_DK', 'DNK', 'Denmark', 'Danmark', 45, 0x0406, 'DAN', 'DK', 'DKK', 277, {}),
    ('sv_SE', 'SWE', 'Sweden', 'Sverige', 46, 0x041D, 'SVE', 'SV', 'SEK', 278, {}),
    ('no_NO', 'NOR', 'Norway', 'Norge', 47, 0x0414, 'NOR', 'NO', 'NOK', 277, {}),
    ('fi_FI', 'FIN', 'Finland', 'Suomi', 358, 0x040B, 'FIN', 'SU', 'FIM', 278, {}),
    ('is_IS', 'ISL', 'Iceland', 'Ísland', 354, 0x040F, 'ISL', 'IS', 'ISK', 871, {}),
    ('pl_PL', 'POL', 'Poland', 'Polska', 48, 0x0415, 'PLK', 'PL', 'PLN', 870, {}),
    ('cs_CZ', 'CZE', 'Czech Republic', 'Česká republika', 421, 0x0405, 'CSY', 'CZ', 'CZK', 870, {}),
    ('sk_SK', 'SVK', 'Slovakia', 'Slovensko', 422, 0x041B, 'SKY', 'SL', 'SKK', 870, {}),
    ('hu_HU', 'HUN', 'Hungary', 'Magyarország', 36, 0x040E, 'HUN', 'HU', 'HUF', 870, {}),
    ('ru_RU', 'RUS', 'Russia', 'Россия', 7, 0x0419, 'RUS', 'RU', 'RUR', 1025, {}),
    ('el_GR', 'GRC', 'Greece', 'Ελλάδα', 30, 0x0408, 'ELL', 'GK', 'GRD', 875, {}),
    ('tr_TR', 'TUR', 'Turkey', 'Türkiye', 90, 0x041F, 'TRK', 'TR', 'TRL', 1026, {}),
    ('ja_JP', 'JPN', 'Japan', '日本', 81, 0x0411, 'JPN', 'JP', 'JPY', 930, {48: '¥'}),
    ('ko_KR', 'KOR', 'Korea', '대한민국', 82, 0x0412, 'KOR', 'KO', 'KRW', 933, {48: '₩'}),
    ('zh_CN', 'CHN', 'China', '中国', 86, 0x0804, 'CHS', 'US', 'CNY', 935, {48: '¥'}),
    ('zh_TW', 'TWN', 'Taiwan', '台灣', 88, 0x0404, 'CHT', 'US', 'TWD', 937, {}),
]

# Windows / Java date pieces by date order (0 MDY, 1 DMY, 2 YMD).
ORDER = {0: ('%m', '%d', '%y', 'M', 'd', 'yy'), 1: ('%d', '%m', '%y', 'd', 'M', 'yy'),
         2: ('%y', '%m', '%d', 'yy', 'M', 'd')}


def lconv_neg(cf):
    """Windows negative currency format for a COUNTRY.SYS currency format."""
    return {0: 1, 1: 5, 2: 9, 3: 8, 4: 8}[cf]


def items(loc, posix=False):
    (name, iso3, eng_ctry, nat_ctry, code, lcid, wlang, kbd, intl, ebcdic, over) = loc
    lang, ctry = name[:2], name[3:5]
    (eng_lang, nat_lang, days, adays, months, amonths, yes, no, am, pm) = LANGS[lang]
    (_, date, cur, cur_ascii, th, dec, ds, ts, cf, dig, clock, ls, cp1, cp2) = COUNTRY[code]
    script = SCRIPT[name if name in ('zh_CN', 'zh_TW') else LANG_SCRIPT.get(lang, 'latin1')]
    if not am:
        am, pm = ('AM', 'PM') if clock == 0 else ('', '')
    a, b, c, wa, wb, wc = ORDER[date]
    i = {}
    i[1] = '%a %d %b %Y %H:%M:%S'
    i[2] = a + ds + b + ds + c
    i[3] = '%H' + ts + '%M' + ts + '%S'
    i[4], i[5] = am, pm
    d, ad = days.split(), adays.split()
    for k in range(7):
        i[6 + k] = ad[k]
        i[13 + k] = d[k]
    m, am_ = months.split(), amonths.split()
    for k in range(12):
        i[20 + k] = am_[k]
        i[32 + k] = m[k]
    i[44], i[45] = dec, th
    i[46], i[47] = yes, no
    i[48] = cur
    i[49] = 'IBM-%d' % cp1
    i[50] = '%08X' % ((LOCALES.index(loc) + 1) << 16)
    i[51] = '%04X' % lcid
    i[52] = LOCALES.index(loc) + 1
    i[53] = '0123456789'
    i[54] = MAXITEM
    i[55] = '%I' + ts + '%M' + ts + '%S %p'
    for k in (56, 57, 58, 59, 60, 97, 98, 99, 89):
        i[k] = ''
    i[61] = '^[%s%s]' % (yes[0].lower(), yes[0].upper())
    i[62] = '^[%s%s]' % (no[0].lower(), no[0].upper())
    i[63], i[64], i[65] = ds, ts, ls
    i[66], i[67] = dec, th
    i[68] = i[69] = '3;0'
    i[70] = 0  # iMeasure: 0 metric, 1 U.S. (GLOCALE's combo index)
    i[71] = 2
    i[72] = 2
    i[73] = clock
    i[74] = date
    i[75] = min(cf, 3)
    i[76] = dig
    i[77] = 1
    i[78] = 1
    i[79] = date
    i[80] = 1
    i[81] = 0
    i[82] = 0
    i[83] = 0 if code == 1 else lconv_neg(cf)
    i[84] = 1 if clock else 0
    i[85] = 0
    i[86] = 0
    i[87] = intl
    i[88] = wlang
    i[90] = 0
    i[91] = 0
    i[92], i[93] = '', '-'
    i[94], i[95] = '(', ')'
    i[96] = LONG_DATE[lang]
    i[100] = name
    i[101], i[102] = lang, ctry
    i[103], i[104] = eng_lang, nat_lang
    i[105], i[106], i[107] = eng_ctry, eng_ctry, nat_ctry
    i[108] = code
    i[109], i[110], i[113] = script
    i[111], i[112] = cp1, cp2
    i[114] = ebcdic
    i[115] = '%d' % cp2
    i[116] = '%d' % cp1
    i[117] = i[119] = kbd
    i[118] = ''
    i[120], i[121] = 'DB', 'CR'
    i[122] = name if script[1] == 1252 else 'en_US'
    i[123] = ('HH' if clock else 'h') + ts + 'mm' + ts + 'ss' + ('' if clock else ' tt')
    i[124] = wa + ds + wb + ds + wc
    i[125] = 'dddd, d MMMM yyyy'
    i[126] = iso3
    i[127] = '#,##0%'
    i[128] = '%'
    i[129] = 'E'
    i[130] = i[131] = ('HH' if clock else 'h') + ':mm:ss' + ('' if clock else ' a') + ' z'
    i[132] = ('HH' if clock else 'h') + ':mm' + ('' if clock else ' a')
    i[133] = 'EEEE, d MMMM yyyy'
    i[134] = 'd-MMM-yy'
    i[135] = '{1} {0}'
    i[136] = 'BC;AD'
    i.update(over)
    if posix:
        i.update({100: posix, 101: '', 102: '', 103: 'POSIX', 104: 'POSIX', 105: '', 106: '', 107: '',
                  1: '%a %b %e %H:%M:%S %Y', 2: '%m/%d/%y', 3: '%H:%M:%S', 45: '', 48: '', 66: '',
                  67: '', 68: '', 69: '', 87: '', 50: '00000000', 52: 0, 81: 6, 108: 0, 122: posix})
    assert sorted(i) == list(range(1, MAXITEM + 1)), sorted(set(range(1, MAXITEM + 1)) - set(i))
    return [str(i[k]) for k in range(1, MAXITEM + 1)]


def utf16(s):
    """UCS-2 code units (no surrogates expected)."""
    u = [ord(ch) for ch in s]
    assert all(c < 0x10000 for c in u), s
    return u


def generate(path):
    table = [('posix', items(LOCALES[0], 'posix')), ('univ', items(LOCALES[0], 'univ'))]
    table += [(loc[0], items(loc)) for loc in LOCALES]
    pool, index = [], {}

    def intern(s):
        if s not in index:
            index[s] = len(pool)
            pool.extend(utf16(s) + [0])
        return index[s]

    offs = [[intern(s) for s in its] for (_, its) in table]
    assert len(pool) < 0x10000
    o = ['//! Generated by tools/scripts/gen_locales.py: do not edit.',
         '//! rs2\'s own locale data (country conventions from gen_nls.py).', '',
         '#![allow(clippy::all)]', '',
         '/// Every item string, NUL-terminated UCS-2.',
         'pub static POOL: [u16; %d] = [' % len(pool)]
    for k in range(0, len(pool), 16):
        o.append('    ' + ', '.join('0x%04x' % v for v in pool[k:k + 16]) + ',')
    o.append('];')
    o.append('')
    o.append('/// (locale name, offset in POOL of items 1-%d).' % MAXITEM)
    o.append('pub static LOCALES: [(&str, [u16; %d]); %d] = [' % (MAXITEM, len(table)))
    for (name, _), of in zip(table, offs):
        o.append('    ("%s", [' % name)
        for k in range(0, len(of), 16):
            o.append('        ' + ', '.join('%d' % v for v in of[k:k + 16]) + ',')
        o.append('    ]),')
    o.append('];')
    open(path, 'w').write('\n'.join(o) + '\n')


if __name__ == '__main__':
    generate(sys.argv[1])
