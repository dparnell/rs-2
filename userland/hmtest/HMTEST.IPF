.* HMTEST.HLP: the help library of HMTEST.EXE, which checks the help
.* manager (compiled by crates/ipf when the image is built).
:userdoc.
:title.Help Manager Test
:h1 res=1.Dynamic data
:p.The text between the two lines below is the application's dynamic
data (:hp2.&colon.ddf res=1.:ehp2.)&colon.
:p.-----
:ddf res=1.
:p.-----
:p.Text of the panel after the dynamic data.
:h1 res=2.Fonts
:p.The default font.
:p.:font facename=Helv size=14x10.Helv 14 points.:font facename=default.
:p.:font facename='Tms Rmn' size=18x14.Tms Rmn 18 points.:font facename=default.
:p.:font facename=Courier size=10x8.Courier 10 points.:font facename=default.
:p.:hp1.Italic:ehp1., :hp2.bold:ehp2., :hp3.bold italic:ehp3., :hp5.underlined:ehp5..
:h1 res=3 id=target.Link target
:p.The panel the dynamic data links to by name.
:h1 res=4.Panel four
:p.The panel the dynamic data links to by number.
:h1 res=20.Message box help
:p.The help of the test message box (MB_HELP).
:h1 res=5.Windows
:p.Links to panels in windows of their own&colon.
:ul.
:li.:link reftype=hd res=6 vpx=right vpy=top vpcx=45% vpcy=60% group=1.Upper right, group 1:elink.
:li.:link reftype=hd res=7 vpx=right vpy=top vpcx=45% vpcy=60% group=1.Another panel in group 1:elink.
:li.:link reftype=hd res=8 vpx=left vpy=bottom vpcx=50% vpcy=35% dependent.Lower left, dependent:elink.
:li.:link reftype=hd res=10.A panel whose heading has its place and size:elink.
:eul.
:p.:link reftype=hd res=9 auto vpx=right vpy=bottom vpcx=45% vpcy=30%.:elink.
The panel at the lower right opened by itself (an :hp2.auto:ehp2. link).
:h1 res=6.Group one
:p.The first panel of viewport group 1.
:p.:link reftype=hd res=7.Next in the group:elink.
:h1 res=7.Group one again
:p.The second panel of group 1, in the same window.
:h1 res=8.Dependent
:p.A dependent window: it closes when the panel it was opened from changes.
:h1 res=9.Automatic
:p.Opened by an automatic link of the Windows panel.
:h1 res=10 x=center y=center width=60% height=40%.Placed by its heading
:p.This panel's heading gives its window (x=center y=center width=60% height=40%).
:h1 res=11.Metafiles
:p.The application's metafiles in its dynamic data (DdfMetafile), each
scaled to fit its rectangle&colon.
:ddf res=2.
:p.Text after the dynamic data.
:h1 res=12.A metafile filling the window
:ddf res=3.
:h1 res=13.Window controls
:p.Links to panels in windows with other controls&colon.
:ul.
:li.:link reftype=hd res=14 vpx=left vpy=top vpcx=50% vpcy=45% titlebar=none scroll=none rules=border.No title bar, no scroll bars, a thin border:elink.
:li.:link reftype=hd res=15 vpx=right vpy=top vpcx=50% vpcy=45%.A panel whose heading asks for a title bar alone and a horizontal scroll bar:elink.
:li.:link reftype=hd res=16 vpx=left vpy=bottom vpcx=50% vpcy=40% titlebar=sysmenu scroll=vertical.A system menu and a vertical scroll bar:elink.
:li.:link reftype=hd res=17.This window without a vertical scroll bar:elink.
:eul.
:h1 res=14.No controls
:p.A window without a title bar or scroll bars (titlebar=none scroll=none
rules=border on the link).
:h1 res=15 titlebar=yes scroll=horizontal rules=none.Horizontal scroll bar
:p.A title bar without its buttons, a horizontal scroll bar and no border,
from this panel's heading. A long word&colon.
:p.Abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz0123456789.
:h1 res=16.System menu
:p.A title bar with its system menu but no buttons and a vertical
scroll bar (titlebar=sysmenu scroll=vertical on the link).
:h1 res=17 scroll=none.No scroll bar
:p.The help window's own panel without a vertical scroll bar (scroll=none
on its heading).
:h1 res=18.Metafile artwork
:p.A metafile of the help library (&colon.artwork with a .MET file), played at
the help manager's size for it, centred&colon.
:artwork name='HMART.MET' align=center.
:p.And a run-in copy at the start of this line&colon.
:artwork name='HMART.MET' runin.
Text after it.
:euserdoc.
