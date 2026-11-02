.* KLONDIKE.HLP: help for rs2's Klondike Solitaire (userland/klondike,
.* OS2\APPS\KLONDIKE.EXE).
.*
.* Written for rs2. The res= numbers are the panels the game's help table
.* asks for (the numbers OS/2's Klondike Solitaire uses, so the shell's
.* HELPPANEL references work). xtask compiles this file with crates/ipf
.* into OS2\HELP\KLONDIKE.HLP.
:userdoc.
:title.Klondike Solitaire Help
.*
:h1 res=1000 id=general.Klondike Solitaire
:i1.Klondike Solitaire
:i1.solitaire
:p.Klondike is the best known of the patience games&colon. one player, one
deck of 52 cards, and the aim of building all four suits up from the ace
to the king.
:p.The game is laid out like this&colon.
:dl tsize=16.
:dt.Talon
:dd.The face-down deck at the top left. Click it to turn up cards.
:dt.Waste pile
:dd.Beside the talon&colon. the cards turned up from it. Only its top card
can be played.
:dt.Foundations
:dd.The four places at the top right, one for each suit. Each starts with
an ace and is built up in suit&colon. ace, 2, 3 and so on to the king.
:dt.Tableau
:dd.The seven piles below. They start with one to seven cards, the top
card of each face up. On the tableau you build down in alternating
colors&colon. a black 9 on a red 10, a red 8 on that.
:edl.
:p.See :link reftype=hd refid=rules.How to play:elink.,
:link reftype=hd refid=scoring.Scoring:elink. and
:link reftype=hd res=1120.Keys:elink..
:p.The menus are&colon.
:ul compact.
:li.:link reftype=hd res=1150.Game:elink. to deal and to let the computer play
:li.:link reftype=hd res=1200.Moves:elink. to take moves back and finish a game
:li.:link reftype=hd res=1300.Options:elink. for the timer, the score, the
colors and the settings
:li.:link reftype=hd res=64022.Help:elink.
:eul.
.*
:h2 id=rules.How to play
:i1.rules
:p.Move cards with the mouse&colon. point at a card, hold mouse button 1
down, drag the card to its new place and let go. If the move is not
allowed, the card goes back and you hear a beep. (With :hp2.Drag cards:ehp2.
turned off in the settings, click the card and then click the place.)
:p.What you can move&colon.
:ul.
:li.the top card of the waste pile, onto the tableau or a foundation;
:li.any face-up cards of a tableau pile, together with the cards on top of
them, onto another tableau pile;
:li.the top card of a tableau pile onto a foundation;
:li.the top card of a foundation back onto the tableau.
:eul.
:p.Only a king (with the cards on it) can go to an empty tableau pile.
:p.When the last face-up card of a tableau pile has been moved away, the
face-down card below it turns over by itself (unless you turned
:hp2.Auto-flip cards:ehp2. off; then click it).
:p.Double-click a card to send it to its foundation if it can go there.
:p.Click the talon to turn up cards onto the waste pile&colon. three at a
time or one, depending on the :link reftype=hd res=1350.game
variation:elink.. When the talon is empty, click its place to turn the
waste pile over and go through it again, if the variation allows another
pass. The empty place shows a circle when you can, and a cross when you
cannot.
:p.You win when all 52 cards are on the foundations.
.*
:h2 id=scoring.Scoring
:i1.score
:p.The score counts across games, like a bank&colon.
:ul.
:li.it starts at 500;
:li.each deal costs 52;
:li.each card you put on a foundation earns 5 (taking it off again costs
the 5 back);
:li.each time you turn the waste pile over to go through the talon again
costs 25.
:eul.
:p.So a deal pays off once you bring home more than ten cards. Use
:hp2.Reset:ehp2. in the :link reftype=hd res=1320.Score:elink. menu to start
again at 500.
.*
:h2 res=1120 id=keys.Keys
:i1.keys
:p.These keys work in the Klondike window&colon.
:dl tsize=20.
:dt.Ctrl+N
:dd.New deal.
:dt.Ctrl+P
:dd.Auto play (again to stop it).
:dt.Ctrl+T, Alt+Backspace
:dd.Take back the last move.
:dt.Ctrl+R
:dd.Replay a move taken back.
:dt.Ctrl+C
:dd.Cheat mode on or off.
:dt.Ctrl+F
:dd.Auto finish.
:dt.Left, Right
:dd.Move the keyboard cursor (a white frame) from pile to pile.
:dt.Up, Down
:dd.Move the cursor between the top row and the tableau.
:dt.Space, Enter
:dd.On the talon&colon. turn up cards. On another pile&colon. pick up its
cards (they get a yellow frame); on the place they should go&colon. put
them there. A face-down card on top of a tableau pile is turned up.
:dt.Esc
:dd.Put the picked-up cards back.
:dt.F1
:dd.Help.
:edl.
.*
:h2 res=1150 id=game.Game menu
:i1.Game menu
:i1.New deal
:i1.Auto play
:dl tsize=16.
:dt.New deal
:dd.Shuffles the cards and deals a new game. The deal costs 52 points of
the score.
:dt.Auto play
:dd.Lets the computer play the game from where it is, one move at a time,
at the speed set in the settings. It stops when it cannot get any further
or when the game is won; choose it again, click the board or press a key
to stop it yourself.
:edl.
.*
:h2 res=1200 id=moves.Moves menu
:i1.Moves menu
:i1.Take back
:i1.Cheat
:dl tsize=16.
:dt.Take back
:dd.Takes back the last move. You can take back every move of the game,
one after another; the score follows.
:dt.Replay
:dd.Makes a move you took back again. A new move drops the moves you took
back.
:dt.Cheat
:dd.Turns cheat mode on or off. In cheat mode any face-up card (with the
cards on it) can go anywhere, rules or not. The game counts your
cheats and reminds you of them
when you win.
:dt.Auto finish
:dd.Moves every card that can go to the foundations there, one after
another. Near the end of a game this saves a lot of dragging.
:edl.
.*
:h2 res=1300 id=options.Options menu
:i1.Options menu
:dl tsize=16.
:dt.Timer
:dd.Shows or hides the time the game has taken (bottom right).
:dt.Score
:dd.Shows or hides the score, or resets it.
:dt.Color scheme
:dd.Four colorings of the board&colon. Spring, Summer, Autumn and Winter.
:dt.Settings
:dd.Opens the settings notebook.
:dt.Save settings
:dd.Keeps the settings for the next time.
:edl.
.*
:h2 res=1330 id=timer.Timer
:p.Choose :hp2.Timer:ehp2. to show or hide the time. The clock starts with
your first move and stops when you win.
.*
:h2 res=1320 id=score.Score
:i1.Score menu
:p.:hp2.On:ehp2. shows the score (bottom left), :hp2.Off:ehp2. hides it
(the game keeps counting), :hp2.Reset:ehp2. sets it back to 500.
:p.See :link reftype=hd refid=scoring.Scoring:elink..
.*
:h2 res=1301 id=colors.Color scheme
:i1.colors
:p.Choose one of the four colorings of the board&colon. Spring (light
green), Summer (deep green), Autumn (brown) or Winter (blue).
.*
:h2 res=3990 id=settings.Settings
:i1.settings
:p.Choose :hp2.Settings:ehp2. to open the settings notebook. It has two
pages&colon. :link reftype=hd res=2210.Options:elink. and
:link reftype=hd res=2240.Card backs:elink.. A change takes effect at once.
Close the notebook with its system menu or Esc.
.*
:h2 res=1500 id=save.Save settings
:p.Choose :hp2.Save settings:ehp2. to keep the settings for the next time
you play. They are kept in the file KLONDIKE.INI beside the program. When
you close the game with changed settings that are not saved, the game asks
whether to save them. The window's place is kept in any case.
.*
:h2 res=2210 id=optpage.Settings - Options
:i1.options
:dl tsize=18.
:dt.Opening video
:dd.Not available&colon. this Klondike has no opening video.
:dt.Drag cards
:dd.On&colon. move cards by dragging them. Off&colon. click the cards, then
click their new place.
:dt.Auto-flip cards
:dd.Turn up the face-down card a move uncovers.
:dt.Animate cards
:dd.Show the moves of Auto finish one at a time. Off&colon. Auto finish
moves all the cards at once.
:dt.Animation speed
:dd.How fast Auto finish and Auto play make their moves.
:dt.Game variations
:dd.See :link reftype=hd res=1350.Game variations:elink..
:edl.
:p.:hp2.Undo:ehp2. takes back the changes made on this page since the
notebook was opened; :hp2.Default:ehp2. sets the page's original
settings.
.*
:h2 res=1350 id=variations.Game variations
:i1.game variations
:i1.draw 3
:dl tsize=22.
:dt.Draw 3, Flip none
:dd.The talon turns up three cards at a time, and you go through it once.
:dt.Draw 3, Flip 3
:dd.Three cards at a time; you may turn the waste pile over three times.
:dt.Draw 3, Flip many
:dd.Three cards at a time, as often as you like (each pass still costs 25
points).
:dt.Draw 1, Flip many
:dd.One card at a time, as often as you like.
:edl.
:p.With a limited number of passes, the bottom line shows how many are
left.
.*
:h2 res=1310 id=variation1310.Variations
:p.See :link reftype=hd res=1350.Game variations:elink..
.*
:h2 res=1340 id=anim.Animation
:p.See :link reftype=hd res=2210.Settings - Options:elink..
.*
:h2 res=2240 id=backpage.Settings - Card backs
:i1.card backs
:p.Click one of the nine designs (or use the cursor keys) to put it on the
back of the cards; the talon and the face-down cards of the tableau show it
at once. :hp2.Undo:ehp2. goes back to the design the notebook was opened
with, :hp2.Default:ehp2. to the blue lattice.
.*
:h2 res=1450 id=backs.Card backs
:p.See :link reftype=hd res=2240.Settings - Card backs:elink..
.*
:h2 res=1400 id=deck.The cards
:p.The cards are drawn by the program at the size the window allows&colon.
make the window larger for larger cards.
.*
:h2 res=2220 id=sounds.Sounds
:p.This Klondike plays no sounds.
.*
:h2 res=2230 id=colorpage.Colors
:p.The board's colors are chosen with :link reftype=hd res=1301.Color
scheme:elink. in the Options menu.
.*
:h2 res=2250 id=bgpage.Background
:p.The board is a plain color&colon. see :link reftype=hd res=1301.Color
scheme:elink..
.*
:h2 res=2260 id=facepage.Card faces
:p.The card faces are the program's own and cannot be replaced.
.*
:h2 res=64022 id=helpmenu.Help menu
:i1.Help menu
:dl tsize=22.
:dt.Help index
:dd.An alphabetic list of the help topics.
:dt.General help
:dd.What Klondike Solitaire is and how it is played.
:dt.Using help
:dd.How to use the help itself.
:dt.Keys help
:dd.The keys of the game.
:dt.Product information
:dd.The program's name and version.
:edl.
.*
:h2 res=64026 id=helpindex.Help index
:p.Choose :hp2.Help index:ehp2. to see the help topics in alphabetic
order.
.*
:h2 res=64024 id=generalhelp.General help
:p.Choose :hp2.General help:ehp2. for an overview of the game.
.*
:h2 res=64023 id=usinghelp.Using help
:p.Choose :hp2.Using help:ehp2. to learn how the help windows work.
.*
:h2 res=64025 id=keyshelp.Keys help
:p.Choose :hp2.Keys help:ehp2. for the keys of the game.
.*
:h2 res=64096 id=product.Product information
:p.Shows the name and version of Klondike Solitaire.
.*
:h2 res=3500 id=productdlg.Product information window
:p.The program's name and version. Choose :hp2.OK:ehp2. to close the
window.
:euserdoc.
