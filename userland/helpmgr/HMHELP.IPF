.* HMHELP.HLP: rs2's help for using help (HM_DISPLAY_HELP 0, 0 when the
.* application has not replaced it with HM_REPLACE_HELP_FOR_HELP).
.*
.* Written for rs2, describing rs2's help window. The panels have no res=
.* numbers, so they never stand for an application's panels; the help
.* manager shows the first panel. xtask compiles this file with crates/ipf
.* into OS2\HELP\HMHELP.HLP.
:userdoc.
:title.Using Help
:h1 id=using.Using help
:i1.using help
:i1.help window
:p.Help shows information about the program you are using, in the help
window at the right of the screen. Press F1 in a program for help on the
window, field or menu choice you are working with, or choose a choice of
the program's :hp2.Help:ehp2. menu.
:p.The help window shows one topic at a time. Its title is the name of
the help and of the topic. Below the text are push buttons&colon.
:dl tsize=14.
:dt.Previous
:dd.Goes back to the topic you saw before (also Esc or Backspace).
:dt.Search...
:dd.Finds the topics that contain the words you type.
:dt.Index
:dd.Lists the subjects of the help in alphabetical order.
:dt.Contents
:dd.Lists the topics of the help by chapter.
:edl.
:p.More about&colon.
:ul.
:li.:link reftype=hd refid=reading.Reading a topic:elink.
:li.:link reftype=hd refid=links.Links:elink.
:li.:link reftype=hd refid=lists.The contents, the index and searching:elink.
:li.:link reftype=hd refid=keys.Keys of the help window:elink.
:li.:link reftype=hd refid=closing.Closing help:elink.
:eul.
.*
:h2 id=reading.Reading a topic
:i1.scrolling
:p.When a topic does not fit in the window, use the scroll bar, or the
Up and Down arrow keys, PgUp and PgDn, Home and End. Make the window
larger by dragging its border, or maximize it.
:p.Some topics show parts of their text in windows of their own inside
the help window. Each has a title bar and a scroll bar; Esc closes it.
.*
:h2 id=links.Links
:i1.links
:i1.hypertext
:p.Words shown in a different color are links to other topics. Click a
link, or press Tab and Shift+Tab to move the selection box from link to
link and press Enter, to see the topic it leads to.
:p.Some links open a small window (a footnote) over the help window
with a short explanation; press Esc to close it. Other links start a
program or send a message to the program you are using.
:p.To return to the topic you were reading, press Esc or choose
:hp2.Previous:ehp2..
.*
:h2 id=lists.The contents, the index and searching
:i1.contents
:i1.index
:i1.searching
:p.:hp2.Contents:ehp2. lists every topic, the topics of a chapter
indented under it. :hp2.Index:ehp2. lists the subjects in alphabetical
order. Select a line with the arrow keys or the mouse and press Enter,
or double click it, to see its topic.
:p.:hp2.Search...:ehp2. asks for words&colon. type one or more words and
press Enter. The list then shows the topics that contain all of them.
.*
:h2 id=keys.Keys of the help window
:i1.keys
:dl tsize=18.
:dt.F1
:dd.This information, when pressed in the help window.
:dt.Up, Down
:dd.Scroll the topic by a line.
:dt.PgUp, PgDn
:dd.Scroll the topic by a window.
:dt.Home, End
:dd.Go to the start or the end of the topic.
:dt.Tab, Shift+Tab
:dd.Select the next or the previous link.
:dt.Enter
:dd.Follow the selected link, or open the selected line of a list.
:dt.Esc
:dd.Go back to the previous topic; close the window when there is none.
:dt.Backspace
:dd.Go back to the previous topic.
:edl.
.*
:h2 id=closing.Closing help
:p.Close the help window with its system menu, Alt+F4, or Esc at the
first topic you saw. The program you are using stays open; F1 brings
the help back.
:euserdoc.
