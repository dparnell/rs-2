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
:euserdoc.
