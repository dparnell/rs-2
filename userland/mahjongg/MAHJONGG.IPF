.* MAHJONGG.HLP: help for rs2's Mahjongg Solitaire (userland/mahjongg,
.* OS2\APPS\MAHJONGG.EXE).
.*
.* Written for rs2. The res= numbers are the panels the program's help
.* table asks for (the numbers OS/2's Mahjongg uses). xtask compiles this
.* file with crates/ipf into OS2\HELP\MAHJONGG.HLP.
:userdoc.
:title.Mahjongg Solitaire Help
.*
:h1 res=1000 id=general.Mahjongg Solitaire
:i1.Mahjongg Solitaire
:i1.playing Mahjongg
:p.Mahjongg Solitaire is a game for one player, played with the 144 tiles
of a Mahjong set stacked into a layout. The aim is to clear the board by
taking the tiles off in pairs.
:p.Two tiles make a pair when they show the same face. The flowers and the
seasons are different&colon. each of the four flowers pairs with any other
flower, and each season with any other season.
:p.You can only take a tile that is free. A tile is free when&colon.
:ul.
:li.no other tile lies on top of it, not even partly, and
:li.its left side or its right side is open (no tile right next to it on
the same level).
:eul.
:p.Click a free tile to select it; it turns yellow. Then click a free tile
with a matching face, and both leave the board. Click the selected tile
again to change your mind.
:p.The box on the board shows how many pairs could be taken right now (the
:hp2.matches:ehp2.) and how long you have played. When no pair is left the
game tells you and lets you deal again, start this deal over or take moves
back.
:p.Every deal has a number. With :hp2.Deal only solvable games:ehp2. (in
the :link reftype=hd res=3600.settings:elink.) each deal can be played to
the end, although not every way of playing it gets there.
:p.See also :link reftype=hd res=1001.the keys:elink., the
:link reftype=hd res=1210.Game:elink., :link reftype=hd res=1220.Moves:elink.,
:link reftype=hd res=1230.View:elink. and :link reftype=hd res=1233.Options:elink.
menus.
.*
:h2 res=1001 id=keys.Keys
:i1.keys
:dl tsize=20.
:dt.Ctrl+N
:dd.A new deal with a random game number.
:dt.Ctrl+P
:dd.Auto play on or off.
:dt.Ctrl+T, Alt+Backspace
:dd.Take back the last move.
:dt.Ctrl+R
:dd.Start this deal over.
:dt.Ctrl+Y
:dd.Show the moves made so far, one after another.
:dt.Ctrl+C
:dd.Cheat with the next pair.
:dt.Ctrl+S
:dd.Show a pair that can be taken.
:dt.F1
:dd.Help.
:edl.
.*
:h2 res=1210 id=game.Game menu
:i1.Game menu
:p.The Game menu deals new games, chooses the layout and plays by itself&colon.
:dl tsize=16.
:dt.New deal
:dd.Deals again&colon. :hp2.Random:ehp2. picks a new game number,
:hp2.Same:ehp2. deals the same game again, :hp2.Select:ehp2. asks for the
number.
:dt.Default
:dd.Goes back to the built-in layout, the Turtle.
:dt.Open
:dd.Plays the layout of a Mahjongg game file.
:dt.Save, Save as
:dd.Saves the layout in a game file.
:dt.Auto play
:dd.Lets the computer take the pairs.
:edl.
.*
:h3 res=1211 id=newdeal.New deal
:i1.New deal
:i1.game number
:p.Deals the tiles again on the same layout.
:p.:hp2.Random:ehp2. picks a game number at random. :hp2.Same:ehp2. deals
the game with the same number again, so you can try it once more from the
beginning. :hp2.Select:ehp2. asks for a game number from 1 to 30000&colon.
the same number always gives the same game, so you can play the same deal
as a friend.
.*
:h3 res=1215 id=default.Default
:p.Plays the built-in layout, the Turtle&colon. five levels, a long middle
row with single tiles at its ends and one tile at the top.
.*
:h3 res=1216 id=open.Open
:i1.game files
:i1.layouts
:p.Plays the layout of a Mahjongg game file (a file whose name ends in
:hp2..MAH:ehp2.). Several come with the system in the Mahjongg folder; you
can make more with the :link reftype=hd res=1230.Edit view:elink..
:p.You can also open a game file from a folder&colon. Mahjongg starts with
it.
.*
:h3 res=1217 id=save.Save
:p.Saves the layout in its game file, or asks for a name when it has none.
A new file can get a shadow in the Mahjongg folder.
.*
:h3 res=1218 id=saveas.Save as
:p.Saves the layout in a game file under a new name.
.*
:h3 res=1219 id=autoplay.Auto play
:i1.Auto play
:p.The computer takes the pairs, one every moment, until the board is clear
or no pair is left. Choose Auto play again to stop it. Its speed is set in
the :link reftype=hd res=3600.settings:elink..
.*
:h2 res=1220 id=moves.Moves menu
:i1.Moves menu
:p.The Moves menu takes moves back, starts over and gives hints.
.*
:h3 res=1221 id=takeback.Take back
:i1.Take back
:p.Puts the last pair taken back on the board. You can take back every
move down to the start of the game.
.*
:h3 res=1222 id=replay.Replay
:p.Starts the same deal over from the beginning.
.*
:h3 res=1223 id=showreplay.Show replay
:p.Starts the deal over and takes the pairs off again, one after another,
as you took them.
.*
:h3 res=1224 id=cheat.Cheat
:i1.cheating
:p.Lets you take the next pair even if its tiles are not free. The box on
the board says :hp2.Cheat:ehp2. until you have taken it. At the end the game
counts how often you cheated.
.*
:h3 res=1225 id=show.Show
:i1.hints
:p.Shows a pair you could take now by colouring its tiles green for a
moment. Choose Show again for the next pair. Depending on the hints chosen
in the settings, Show colours both tiles, only one of them, or is not
available.
.*
:h3 res=1226 id=showsame.Show same
:p.Colours every tile with the same face as the selected one (or the one
under the pointer) for a moment. It is on the pop-up menu (mouse button 2).
.*
:h2 res=1230 id=view.View menu and Edit mode
:i1.Edit mode
:i1.making layouts
:p.:hp2.Play:ehp2. is the normal view. :hp2.Edit:ehp2. shows the layout
without faces so you can change it or build a new one.
:p.In Edit mode the Edit Mode window sets the size of the board (tiles
across and down) and the level you work on. Click an empty place to put a
tile there on that level; it snaps to whole and half tile positions. Click
a tile of that level to take it away. Tiles below the level are grey,
tiles above it are hidden. :hp2.Reset:ehp2. clears the board.
:p.A layout needs exactly 144 tiles, which may not overlap. :hp2.OK:ehp2.
plays the new layout (save it with Save as), :hp2.Cancel:ehp2. goes back to
the layout you had.
.*
:h2 res=1233 id=options.Options menu
:i1.Options menu
:dl tsize=16.
:dt.Timer
:dd.Shows the time played in the box on the board. The clock starts with
the first pair.
:dt.Matches
:dd.Shows how many pairs could be taken now.
:dt.Settings
:dd.Opens the :link reftype=hd res=3600.settings:elink..
:dt.Save settings
:dd.Keeps the settings for the next game (in MAHJONGG.INI).
:edl.
.*
:h3 res=1234 id=timer.Timer
:p.Shows or hides the time played.
.*
:h3 res=1235 id=matches.Matches
:p.Shows or hides the number of pairs that can be taken now.
.*
:h3 res=1236 id=settingsitem.Settings
:p.Opens the Mahjongg settings.
.*
:h2 res=3600 id=settings.Mahjongg Settings
:i1.settings
:dl tsize=24.
:dt.Warning beeps
:dd.Beep when a tile cannot be taken.
:dt.Deal only solvable games
:dd.Every deal can then be played to the end.
:dt.Show matching tiles
:dd.When you select a tile, the free tiles that match it turn green.
:dt.Hints
:dd.What :hp2.Show:ehp2. shows&colon. both tiles of a pair, only one of
them, or nothing.
:dt.Tiles seen from
:dd.Which sides of the tiles you see&colon. the lower right or the lower
left.
:dt.Auto play speed
:dd.How fast Auto play and Show replay take the pairs.
:dt.Background
:dd.Each game file has its own background color. Check :hp2.Use this color
for every game:ehp2. to use the color set here instead.
:edl.
:p.:hp2.Default:ehp2. puts back the settings Mahjongg starts with. The
settings last until Mahjongg ends; Options, Save settings keeps them.
.*
:h2 res=3540 id=nomoves.No more pairs
:p.No two free tiles match any more. Choose&colon.
:dl tsize=12.
:dt.New
:dd.A new deal.
:dt.Retry
:dd.This deal again from the beginning.
:dt.Exit
:dd.End Mahjongg.
:dt.Cancel
:dd.Look at the board; you can still take moves back.
:edl.
.*
:h2 res=3900 id=level.Choose your hints
:p.Mahjongg asks this the first time you play. Choose how much the
:hp2.Show:ehp2. command helps you&colon. it can point out a whole pair,
only one tile of a pair, or not help at all. You can change this later in
the settings.
.*
:h2 res=3910 id=select.Select Game
:p.Type or spin the number of the game to deal, from 1 to 30000. The same
number always gives the same game on the same layout.
.*
:h2 res=3720 id=gameinfo.Game Information
:p.The title, the author, a comment and a help file of the layout. The
title is shown in the title bar. Changes are kept when you save the game
file.
.*
:h2 res=1298 id=gameinfoitem.Game information
:p.Shows and changes the title, author and comment of the layout being
played.
.*
:h2 res=150 id=helpmenu.Help menu
:dl tsize=20.
:dt.Contents
:dd.The list of help topics.
:dt.General help
:dd.How to play.
:dt.Keys help
:dd.The keys of Mahjongg.
:dt.Using help
:dd.How to use the help.
:dt.Game information
:dd.The layout's title, author and comment.
:dt.Product information
:dd.The program's name and version.
:edl.
.*
:h3 res=152 id=contents.Contents
:p.Shows the list of help topics.
.*
:h3 res=1292 id=generalhelp.General help
:p.Explains how to play Mahjongg Solitaire.
.*
:h3 res=153 id=keyshelp.Keys help
:p.Lists the keys of Mahjongg.
.*
:h3 res=154 id=usinghelp.Using help
:p.Explains how to use the help windows.
.*
:h3 res=1999 id=product.Product information
:p.Shows the program's name and version, and the layout being played.
:euserdoc.
