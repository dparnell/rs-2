.* OS2CHESS.HLP: help for rs2's OS/2 Chess (userland/os2chess,
.* OS2\APPS\OS2CHESS.EXE).
.*
.* Written for rs2. The res= numbers are the panels the program's help
.* table asks for (the numbers OS/2 Chess uses). xtask compiles this file
.* with crates/ipf into OS2\HELP\OS2CHESS.HLP.
:userdoc.
:title.Help for OS/2 Chess
.*
:h1 res=8200 id=general.OS/2 Chess
:i1.OS/2 Chess
:i1.chess
:p.OS/2 Chess is the game of chess on your screen. You can play against
the computer, let two people play on one board, or watch the computer play
against itself.
:p.When OS/2 Chess starts, it asks who plays each side
(:link reftype=hd res=8108.Set players:elink.). White moves first. To move
a piece, click it (it is outlined), then click the square it goes to. To
choose another piece, click it instead; click the outlined piece again to
put it back. You can also type moves (:link reftype=hd res=8114.Keyboard
input:elink.) or choose them from a list (:link reftype=hd res=8120.Valid
moves:elink.).
:p.The line at the bottom of the window says whose move it is, whether a
king is in check and how the game ended. When the computer is thinking,
it says so; the board waits until the computer has moved.
:p.The board is shown in perspective (3-D) or from above (2-D); see
:link reftype=hd res=8117.View:elink. and
:link reftype=hd res=8130.Settings:elink..
:p.The menus are&colon.
:ul compact.
:li.:link reftype=hd res=8100.Game:elink.
:li.:link reftype=hd res=8106.Moves:elink.
:li.:link reftype=hd res=8117.View:elink.
:li.:link reftype=hd res=8107.Options:elink.
:li.:link reftype=hd res=8125.Network:elink.
:eul.
:p.Click the board with mouse button 2 for a menu of the commands used
most.
.*
:h2 res=8228 id=keys.Keys
:i1.keys
:p.These keys work in the OS/2 Chess window&colon.
:dl tsize=12.
:dt.Ctrl+N
:dd.Start a new game.
:dt.Ctrl+P
:dd.Print the board and the game record.
:dt.Ctrl+T
:dd.Take back a move.
:dt.Ctrl+S
:dd.Show the Move Status window.
:dt.Ctrl+R
:dd.Show the Game Record window.
:dt.Ctrl+V
:dd.List the valid moves.
:dt.Ctrl+C
:dd.List the captured pieces.
:dt.Ctrl+L
:dd.Label the board's rows and columns, or take the labels away.
:dt.F1
:dd.Help.
:edl.
.*
:h2 res=8100 id=game.Game menu
:i1.Game menu
:p.The Game menu starts, keeps and prints games&colon.
:dl tsize=12.
:dt.New
:dd.:link reftype=hd res=8101.Starts a new game.:elink.
:dt.Load
:dd.:link reftype=hd res=8102.Continues a saved game.:elink.
:dt.Save
:dd.:link reftype=hd res=8103.Saves the game.:elink.
:dt.Delete
:dd.:link reftype=hd res=8104.Deletes a saved game.:elink.
:dt.Print
:dd.:link reftype=hd res=8105.Prints the board and the moves.:elink.
:edl.
.*
:h3 res=8101 id=new.New
:p.Choose :hp2.New:ehp2. to set the pieces up for a new game. If a game is
being played, you are asked whether to save it first.
.*
:h3 res=8102 id=load.Load
:i1.loading a game
:p.:hp2.Load:ehp2. lists the games saved in OS/2 Chess's directory, with
the players and the date each was saved. Select one and choose
:hp2.OK:ehp2. to continue it from where it was saved.
.*
:h3 res=8103 id=save.Save
:i1.saving a game
:p.:hp2.Save:ehp2. keeps the game in a file in OS/2 Chess's directory&colon.
the players' names, the date and all the moves. A game loaded or saved
before is saved into the same file again; a new game gets a new file
(GAME0001.PMC, GAME0002.PMC and so on).
.*
:h3 res=8104 id=delete.Delete
:p.:hp2.Delete:ehp2. lists the saved games. Select one and choose
:hp2.Delete:ehp2.; you are asked to confirm.
.*
:h3 res=8105 id=print.Print
:i1.printing
:p.:hp2.Print:ehp2. prints the board as it is now and the moves of the game
on the default printer.
.*
:h2 res=8106 id=moves.Moves menu
:i1.Moves menu
:dl tsize=18.
:dt.Take back
:dd.:link reftype=hd res=8116.Takes back the last move.:elink.
:dt.Move status
:dd.:link reftype=hd res=8118.Shows what the computer is doing.:elink.
:dt.Game record
:dd.:link reftype=hd res=8119.Lists the moves made.:elink.
:dt.Valid moves
:dd.:link reftype=hd res=8120.Lists the moves allowed now.:elink.
:dt.Captured pieces
:dd.:link reftype=hd res=8122.Lists the pieces taken.:elink.
:edl.
.*
:h3 res=8116 id=takeback.Take back
:i1.taking back a move
:p.:hp2.Take back:ehp2. undoes the last move. When you play against the
computer, the computer's answer is taken back too, so it is your move
again.
.*
:h3 res=8118 id=status.Move Status
:p.The Move Status window shows the last move, and while the computer is
thinking, how long it has been thinking. When it has moved, the window
shows how many positions it looked at and how many moves ahead.
.*
:h3 res=8119 id=record.Game Record
:i1.game record
:p.The Game Record window lists the moves of the game, White's on the left
and Black's on the right, under the players' names. It stays open while
you play and follows the game.
:p.Moves are written with the squares a piece moves from and to, for
example :hp2.e2-e4:ehp2. or :hp2.Ng1-f3:ehp2. (K king, Q queen, R rook,
B bishop, N knight; no letter for a pawn). An x means a capture,
O-O and O-O-O are castling, =Q a pawn promoted, + check and # checkmate.
.*
:h3 res=8120 id=valid.Valid Moves
:p.The Valid Moves window lists every move the side to move may make, and
how many there are. Select one and choose :hp2.Move:ehp2. to make it.
.*
:h3 res=8122 id=captured.Captured Pieces
:p.Lists the pieces each side has lost.
.*
:h2 res=8117 id=view.View menu
:i1.View menu
:p.:hp2.Rotate 3-D board:ehp2. turns the board in the 3-D view
(:link reftype=hd res=8123.Rotate Board:elink.).
:p.:hp2.Flip board:ehp2. turns the board around, so that Black's side is
nearest to you.
:p.:hp2.Label board:ehp2. shows the letters of the columns (a to h) and the
numbers of the rows (1 to 8) beside the board.
.*
:h3 res=8113 id=label.Label board
:p.Shows or hides the letters and numbers that name the squares.
.*
:h3 res=8123 id=rotate.Rotate Board
:p.Move the slider to turn the 3-D board around its middle. The board turns
as you move it. :hp2.Default:ehp2. puts it back straight;
:hp2.Cancel:ehp2. keeps the earlier angle.
.*
:h3 res=8124 id=flip.Flip board
:p.Turns the board around so the other side is at the bottom.
.*
:h3 res=8148 id=showmoves.Show possible moves
:p.When this is on, the squares a piece you pick up can move to are
outlined.
.*
:h2 res=8107 id=options.Options menu
:i1.Options menu
:dl tsize=18.
:dt.Timer
:dd.:link reftype=hd res=8115.Shows the time each side has used.:elink.
:dt.Keyboard input
:dd.:link reftype=hd res=8114.Moves by typing squares.:elink.
:dt.Set players
:dd.:link reftype=hd res=8108.Who plays each side.:elink.
:dt.Set position
:dd.:link reftype=hd res=8109.Sets pieces on the board.:elink.
:dt.Settings
:dd.:link reftype=hd res=8130.Messages, the view and the colors.:elink.
:dt.Save settings
:dd.Keeps the players, the playing level, the view and the colors for the
next time OS/2 Chess starts.
:edl.
.*
:h3 res=8115 id=timer.Time Elapsed
:i1.timer
:p.The Time Elapsed window shows how long each side has taken for its moves
since the game started.
.*
:h3 res=8114 id=keyboard.Keyboard
:i1.keyboard input
:p.Type the square a piece moves from in :hp2.From:ehp2. and the square it
goes to in :hp2.To:ehp2. (for example e2 and e4) and choose
:hp2.Move:ehp2.. Castling is the king's move two squares to the side.
.*
:h3 res=8108 id=players.Set Players
:i1.players
:i1.computer level
:p.For each side, choose :hp2.Human:ehp2. or :hp2.Computer:ehp2. and type a
name if you like. When both sides are human, two people play on the same
board; when both are the computer, it plays against itself.
:p.:hp2.More:ehp2. sets how well the computer plays that side&colon. from
:hp2.Beginner:ehp2., which looks only at the next move and sometimes plays
carelessly, to :hp2.Advanced:ehp2., which looks several moves ahead and
takes longer. With :hp2.Use book openings:ehp2. the computer plays the
first moves of well-known openings.
.*
:h3 res=8109 id=setpos.Set position
:i1.setting up a position
:p.Choose :hp2.Set position:ehp2. to set up a position of your own. Click a
square and choose the piece to put there, or :hp2.Empty:ehp2. to clear it.
Each side must keep its king, and the kings may not stand next to each
other.
:p.When the position is ready, choose :hp2.Set position:ehp2. again and say
which side moves first (:link reftype=hd res=8160.Player to Move:elink.).
Castling is allowed where a king and rook stand on their first squares.
.*
:h3 res=8160 id=tomove.Player to Move
:p.Choose the side that makes the first move in the position you set up.
.*
:h3 res=8204 id=promote.Pawn Promotion
:i1.promotion
:p.A pawn that reaches the far side of the board becomes another piece.
Choose the queen, a rook, a bishop or a knight.
.*
:h3 res=8130 id=settings.Settings
:i1.settings
:p.:hp2.Warning messages:ehp2.&colon. when you try a move that is not
allowed, a message says why. Without it the computer only beeps.
:p.:hp2.Show possible moves:ehp2.&colon. outlines the squares a piece can
move to when you pick it up.
:p.:hp2.View:ehp2.&colon. the board in perspective (3-dimensional) or seen
from above (2-dimensional).
:p.:hp2.Colors:ehp2.&colon. the colors of the board and pieces.
.*
:h3 res=8140 id=book.Opening book
:p.The computer knows the first moves of some well-known openings and plays
them when :hp2.Use book openings:ehp2. is on.
.*
:h3 res=8145 id=colors.Colors
:p.Choose a set of colors for the board, its edge, its labels and the
pieces.
.*
:h3 res=8233 id=colorpage.Colors page
:p.See :link reftype=hd res=8145.Colors:elink..
.*
:h2 res=8125 id=network.Network menu
:i1.network
:p.OS/2 Chess on OS/2 could play against a person at another computer. On
rs2 this is not there&colon. the Network menu's items are not available.
.*
:h3 res=8126 id=sendmsg.Send message
:p.Not available on rs2.
.*
:h3 res=8127 id=disconnect.Disconnect
:p.Not available on rs2.
.*
:h3 res=8128 id=users.List users
:p.Not available on rs2.
.*
:h3 res=8220 id=ok.OK
:p.Accepts the choices made in the window and closes it.
.*
:h3 res=8221 id=cancel.Cancel
:p.Closes the window without changing anything.
.*
:h3 res=8222 id=default.Default
:p.Puts back the values OS/2 Chess starts with.
.*
:h3 res=8223 id=deletebtn.Delete
:p.Deletes the game selected.
.*
:h3 res=8231 id=undo.Undo
:p.Puts back the values the window had when it was opened.
.*
:h2 res=64027 id=contents.Help contents
:p.The help for OS/2 Chess covers&colon.
:ul compact.
:li.:link reftype=hd res=8200.Playing OS/2 Chess:elink.
:li.:link reftype=hd res=8228.Keys:elink.
:li.:link reftype=hd res=8100.The Game menu:elink.
:li.:link reftype=hd res=8106.The Moves menu:elink.
:li.:link reftype=hd res=8117.The View menu:elink.
:li.:link reftype=hd res=8107.The Options menu:elink.
:eul.
.*
:h2 res=64026 id=index.Help index
:p.Choose :hp2.Index:ehp2. in the Help menu to see the help topics in
alphabetic order.
.*
:h2 res=64023 id=usinghelp.Using help
:p.Choose :hp2.Using help:ehp2. to learn how the help windows work.
.*
:h2 res=8224 id=product.Product information
:p.Shows the name and version of OS/2 Chess.
:euserdoc.
