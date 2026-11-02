.* GLOCALE.HLP: help for rs2's Locale program, the Country Palette
.* (userland/glocale, OS2\GLOCALE.EXE).
.*
.* Written for rs2. The res= numbers are the panels the program's help
.* table asks for: the windows' panels are the numbers OS/2's Locale
.* program uses (5010 the palette, 5020 the name window, 5030-5100 the
.* notebook's pages); 5011-5019 are rs2's own panels for the menu items.
.* xtask compiles this file with crates/ipf into OS2\HELP\GLOCALE.HLP.
:userdoc.
:title.Country Palette Help
.*
:h1 res=5010 id=palette.Country Palette
:i1.Country Palette
:i1.locale
:i1.Locale program
:p.A :hp2.locale:ehp2. is a set of conventions for a country or region and a
language&colon. how numbers, amounts of money, dates and times are
written, how text is sorted, which units of measurement are used and the
names of days and months. Programs that use the Unicode functions of
OS/2 format what they show by the locale.
:p.The Country Palette shows your :hp2.user locales:ehp2.&colon. locales
you can change. Each is based on one of the system's locales (en_US for
English in the United States, de_DE for German in Germany, and so on) and
keeps only what you changed. The first time the program starts, it makes
a user locale of the locale the system uses, so there is always at least
one.
:p.To see or change a locale, double-click its icon or select it and
choose :hp2.Properties:ehp2. from its pop-up menu. Its
:link reftype=hd res=5018.notebook:elink. opens.
:p.Click a locale with mouse button 2 for its pop-up menu&colon.
:ul.
:li.:link reftype=hd res=5018.Open as:elink. and :hp2.Properties:ehp2.
:li.:link reftype=hd res=5013.Create another:elink.
:li.:link reftype=hd res=5016.Create java locale:elink.
:li.:link reftype=hd res=5014.Delete:elink.
:li.:link reftype=hd res=5015.Mark as default:elink.
:eul.
:p.Your changes are kept in the file \LANGUAGE\LOCALE\USER.LOC on the
startup drive.
.*
:h2 res=5011 id=localemenu.Locale menu
:i1.Locale menu
:p.The :hp2.Locale:ehp2. menu has&colon.
:dl tsize=22.
:dt.Open as
:dd.:hp2.New locale:ehp2. makes a new locale (see
:link reftype=hd res=5013.Create another:elink.); :hp2.Locale:ehp2.
opens the selected locale's notebook.
:dt.Exit Country Palette
:dd.Closes the program. Changes you made are saved.
:edl.
.*
:h2 res=5012 id=actionmenu.Action menu
:i1.Action menu
:p.The :hp2.Action:ehp2. menu acts on the selected locale&colon.
:link reftype=hd res=5016.Create java locale:elink.,
:link reftype=hd res=5014.Delete:elink. and
:link reftype=hd res=5015.Mark as default:elink..
.*
:h2 res=5013 id=create.Create another
:i1.Create another
:i1.new locale
:p.:hp2.Create another:ehp2. makes a new user locale with the same base
locale as the selected one. Type a name for it in the window that opens
and choose :hp2.OK:ehp2.; the new locale appears in the palette and its
notebook opens, so you can change what it should do differently.
:p.If you give the name of a system locale (for example fr_FR), the new
locale changes that system locale itself&colon. programs asking for fr_FR
get your version.
.*
:h2 res=5020 id=namedlg.Locale Name
:i1.locale name
:p.Type the name of the new locale. A name is up to 31 letters, digits
and the characters _ - . and @, without blanks, and must not be the name
of a locale already in the palette.
:p.The usual form is a language and a country&colon. two small letters, an
underscore and two capital letters, for example en_GB. Add something of
your own to tell several versions apart (en_GB_office).
.*
:h2 res=5014 id=delete.Delete
:i1.deleting a locale
:p.:hp2.Delete:ehp2. removes the selected user locale. Programs using it
fall back to the system locale of that name or to the system's locale.
:p.When you delete the last locale, the program makes a new user locale of
the system's locale, as it does when it first starts.
.*
:h2 res=5015 id=markdefault.Mark as default
:i1.default locale
:i1.Mark as default
:p.:hp2.Mark as default:ehp2. makes the selected locale's conventions the
system's national settings&colon. the ones programs that do not use the
Unicode functions read from the user profile (OS2.INI)&colon. the date and
time format, the 12- or 24-hour clock, the decimal and thousands
separators, the currency symbol and its placement, and the list separator.
:p.They are also written to \LANGUAGE\LOCALE\DEFAULT.LOC and, when WIN-OS/2
is installed, to the [intl] section of its WIN.INI.
.*
:h2 res=5016 id=java.Create java locale
:i1.Java locale
:p.On OS/2 this item makes a Java version of the locale for the Java
runtime of the system. rs2 has no Java runtime, so the item only says that
no Java locale was created.
.*
:h2 res=5017 id=product.Product information
:p.Shows the name and version of the Country Palette.
.*
:h2 res=5018 id=notebook.Locale notebook
:i1.locale notebook
:p.A locale's notebook shows everything the locale defines, on these
pages&colon.
:ul.
:li.:link reftype=hd res=5030.Country/Region:elink.
:li.:link reftype=hd res=5040.Collate:elink.
:li.:link reftype=hd res=5050.Numeric:elink.
:li.:link reftype=hd res=5060.Monetary:elink.
:li.:link reftype=hd res=5070.Date:elink.
:li.:link reftype=hd res=5090.Time:elink.
:li.:link reftype=hd res=5080.Language:elink.
:li.:link reftype=hd res=5100.Advanced:elink.
:eul.
:p.A change takes effect as soon as you make it, and the samples on the
pages show its result. Programs see it after the notebook is closed, when
the locale is saved.
:p.The buttons below the pages&colon.
:dl tsize=12.
:dt.Undo
:dd.Puts back the values the locale had when you opened the notebook.
:dt.Default
:dd.Puts back the values of the locale's base locale, so the locale is
again the same as the system locale it is based on.
:dt.Help
:dd.Shows help for the page.
:edl.
.*
:h2 res=5019 id=helpmenu.Help menu
:i1.Help menu
:p.The :hp2.Help:ehp2. menu has :hp2.Help index:ehp2. (the help topics in
alphabetic order), :hp2.General help:ehp2. (what the Country Palette is
for), :hp2.Using help:ehp2. (how to use the help) and :hp2.Product
information:ehp2..
.*
:h2 res=5030 id=country.Country/Region page
:i1.Country/Region page
:i1.base locale
:i1.measurement
:i1.list separator
:p.:hp2.Locale name:ehp2. is the name of the locale; it cannot be changed
here.
:p.:hp2.Base locale:ehp2. is the system locale this locale is based on; the
lines below it name its language and its country or region. Choosing
another base locale starts the locale again from that one&colon. everything
you changed is dropped. A locale that changes a system locale of the same
name keeps that system locale as its base.
:p.:hp2.Measurement:ehp2. is the system of units&colon. metric, or U.S.
(inches, feet, pounds).
:p.:hp2.List separator:ehp2. is the character that separates items of a
list, for example in a spreadsheet's formulas.
.*
:h2 res=5040 id=collate.Collate page
:i1.Collate page
:i1.sorting
:p.:hp2.Sorting type:ehp2. chooses the rules by which text is sorted&colon.
the order of the letters and how accented letters, capitals and other
characters sort. :hp2.Unicode:ehp2. uses the order of the characters in
Unicode; :hp2.C:ehp2. sorts by character codes alone; the others follow
the rules of a language.
.*
:h2 res=5050 id=numeric.Numeric page
:i1.Numeric page
:i1.decimal symbol
:i1.numbers
:p.How numbers are written&colon.
:dl tsize=22.
:dt.Decimal symbol
:dd.The character between the whole part and the fraction (1.5 or 1,5).
:dt.Digit grouping symbol
:dd.The character between groups of thousands (1,000 or 1.000 or
1 000).
:dt.Positive sign
:dd.What is written before a positive number, usually nothing.
:dt.Decimal digits
:dd.How many digits after the decimal symbol.
:dt.Negative sign
:dd.The sign of negative numbers.
:dt.Format
:dd.Where the sign goes&colon. the list shows a sample of each way.
:edl.
.*
:h2 res=5060 id=monetary.Monetary page
:i1.Monetary page
:i1.currency
:p.How amounts of money are written&colon.
:dl tsize=22.
:dt.Symbol
:dd.The currency symbol ($, EUR, DM...).
:dt.Symbol placement
:dd.Where the symbol goes in positive amounts&colon. the list shows a
sample of each way.
:dt.Decimal symbol
:dd.The character before the fractions of the currency.
:dt.Decimal digits
:dd.How many digits the fractions have.
:dt.Digit grouping symbol
:dd.The character between groups of thousands.
:dt.Leading zero
:dd.Whether amounts below one are written with a zero before the decimal
symbol (0.50 rather than .50).
:dt.Negative amounts
:dd.Where the sign and the symbol go in negative amounts, with or without
parentheses.
:edl.
.*
:h2 res=5070 id=date.Date page
:i1.Date page
:i1.date format
:p.How dates are written in their short form&colon.
:dl tsize=22.
:dt.Date order
:dd.Which comes first&colon. the month, the day or the year.
:dt.Separator
:dd.The character between the parts (11/02/26, 02.11.26).
:dt.Format string
:dd.The format itself, made from the choices above. You can also type
one&colon. %d is the day, %m the month, %y the year in two digits, %Y in
four, %b the month's short name and %B its name.
:edl.
:p.The sample at the bottom shows today's date in the format.
.*
:h2 res=5090 id=time.Time page
:i1.Time page
:i1.time format
:i1.12-hour clock
:p.How times are written&colon.
:dl tsize=22.
:dt.12 hours, 24 hours
:dd.The clock&colon. 1&colon.30 PM, or 13&colon.30.
:dt.AM string, PM string
:dd.What follows a time before and after noon on the 12-hour clock.
:dt.Separator
:dd.The character between hours, minutes and seconds.
:dt.Format string
:dd.The format itself&colon. %H is the hour of the 24-hour clock, %I of
the 12-hour clock, %M the minutes, %S the seconds and %p the AM or PM
string. A format that cannot be used is not taken.
:edl.
:p.The sample at the bottom shows the time now, as it goes.
.*
:h2 res=5080 id=language.Language page
:i1.Language page
:p.:hp2.Language:ehp2. is the language of the locale&colon. the language
programs should use for their messages and in which names of days and
months are given. :hp2.User defined:ehp2. stands for a language the list
does not have.
.*
:h2 res=5100 id=advanced.Advanced page
:i1.Advanced page
:i1.locale items
:p.The :hp2.Item:ehp2. list has every item the locale defines, by the names
programs know them by (for example sDecimal for the decimal symbol,
sDayName1 for Monday). Select an item to see its value under
:hp2.Value:ehp2.; type there to change it.
:p.Change items here only when you know what they are for&colon. the other
pages change the usual ones and keep related items in step.
:euserdoc.
