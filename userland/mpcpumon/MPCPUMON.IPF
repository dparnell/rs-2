.* MPCPUMON.HLP: help for rs2's CPU Monitor, Pulse (userland/mpcpumon,
.* OS2\MPCPUMON.EXE).
.*
.* Written for rs2. The res= numbers are the panels the monitor's help
.* table asks for (the numbers OS/2's Pulse uses). xtask compiles this
.* file with crates/ipf into OS2\HELP\MPCPUMON.HLP.
:userdoc.
:title.CPU Monitor Help
.*
:h1 res=9000 id=general.CPU Monitor
:i1.CPU Monitor
:i1.Pulse
:i1.processor load
:p.The CPU Monitor shows how busy each processor of the computer is, as a
graph that moves as time goes by. A processor is busy while it runs any
program or the system itself, and idle while it has nothing to do; the
graph shows the share of each sample period it was busy, from 0 to 100
percent.
:p.The window shows&colon.
:ul.
:li.the title of the graph, :hp2.% Processor Utilization:ehp2.;
:li.a scale from 0 to 100 along the left side (the
:link reftype=hd res=1064.vertical labels:elink.);
:li.the graph itself, either a line for each processor or a bar for each
processor and one for their average (see
:link reftype=hd res=1061.Graph Mode:elink.);
:li.the processors along the bottom with the colour of each one's graph
(the :link reftype=hd res=1062.horizontal labels:elink.).
:eul.
:p.Double-click the graph to hide the title bar and the menu bar, so the
window takes less room on the screen; double-click it again to bring them
back. Click mouse button 2 on the graph for a pop-up menu of the most used
choices.
:p.You can drag a colour from the Color Palette onto a processor's line or
bar to give it that colour, or onto an empty part of the graph to make it
the background colour.
:p.Only one CPU Monitor can run at a time. It remembers its window position
and its options for the next time.
.*
:h2 res=6000 id=keys.Keys
:i1.keys
:p.These keys work in the CPU Monitor's window&colon.
:dl tsize=14.
:dt.Ctrl+B
:dd.Show bars.
:dt.Ctrl+L
:dd.Show lines.
:dt.Ctrl+S
:dd.Show and change the processors' status.
:dt.Ctrl+R
:dd.Show or hide the horizontal labels.
:dt.Ctrl+V
:dd.Show or hide the vertical labels.
:dt.Ctrl+F
:dd.Freeze the graph, or let it move again.
:dt.Ctrl+G
:dd.Show or hide the grid.
:dt.Ctrl+A
:dd.Set the sample rate.
:dt.F1
:dd.Help.
:edl.
.*
:h2 res=1061 id=mode.Graph Mode
:i1.Graph Mode
:p.The Graph Mode menu chooses how the load is drawn&colon. as
:link reftype=hd res=1000.bars:elink. or as
:link reftype=hd res=1010.lines:elink..
.*
:h3 res=1000 id=bar.Bar
:i1.bar graph
:p.Choose :hp2.Bar:ehp2. (Ctrl+B) to show each processor's load at this
moment as a bar, with the percentage over it, and a last bar for the
average load of all the processors that are online.
.*
:h3 res=1010 id=line.Line
:i1.line graph
:p.Choose :hp2.Line:ehp2. (Ctrl+L) to show the load of each processor over
the last hundred samples as a line in the processor's colour. The newest
sample is at the right edge and the line moves to the left as new samples
come in.
.*
:h2 res=1030 id=status.Status
:i1.processor status
:i1.online
:i1.offline
:p.Choose :hp2.Status:ehp2. (Ctrl+S) to see whether each processor is
online (it runs programs) or offline (it does not).
:p.To change a processor's status, select it in the list and press Enter;
its status in the list changes between Online and Offline. Then choose
:hp2.OK:ehp2. to ask the system for the changes, or :hp2.Cancel:ehp2. to
leave everything as it was. If the system cannot change a processor's
status, a message says so; the first processor always stays online.
:p.A processor that is offline is drawn at 0 and does not count in the
average.
.*
:h2 res=1040 id=options.Options menu
:i1.Options menu
:p.The Options menu sets how the graph looks&colon. its
:link reftype=hd res=1100.background colour:elink., each
:link reftype=hd res=1200.processor's colour:elink., the
:link reftype=hd res=1062.horizontal:elink. and
:link reftype=hd res=1064.vertical labels:elink., the
:link reftype=hd res=1068.grid:elink., whether the graph is
:link reftype=hd res=1066.frozen:elink., and the
:link reftype=hd res=1070.sample rate:elink..
.*
:h3 res=1100 id=bgnd.Background Color
:i1.background colour
:p.Choose a colour from the :hp2.Background Color:ehp2. menu to paint the
graph's area in it. The colour in use is checked.
.*
:h3 res=4110 id=bgndc.Background colours
:p.Choose this colour for the background of the graph.
.*
:h3 res=1200 id=cpuc.Processor Graph Color
:i1.processor colour
:p.The :hp2.Processor Graph Color:ehp2. menu lists the processors. Choose
one to pick the colour of its line or bar.
.*
:h3 res=4210 id=cpucn.A processor's colour
:p.Choose the processor whose colour you want to change; a window lists the
colours to choose from.
.*
:h3 res=4310 id=colordlg.Choosing a processor's colour
:p.Select the colour for the processor's line or bar and choose
:hp2.OK:ehp2. (or double-click the colour). :hp2.Cancel:ehp2. keeps the
colour it has.
.*
:h3 res=1062 id=hlabels.Horizontal Labels
:i1.labels
:p.Choose :hp2.Horizontal Labels:ehp2. (Ctrl+R) to show or hide the
processors' numbers and colours under the graph. A check mark means they
are shown.
.*
:h3 res=1064 id=vlabels.Vertical Labels
:p.Choose :hp2.Vertical Labels:ehp2. (Ctrl+V) to show or hide the scale
from 0 to 100 percent along the left side of the graph. A check mark means
it is shown.
.*
:h3 res=1066 id=freeze.Freeze Screen
:i1.freeze
:p.Choose :hp2.Freeze Screen:ehp2. (Ctrl+F) to stop the graph moving, so
you can look at it at your leisure. The monitor keeps measuring
meanwhile; choose Freeze Screen again to see the graph as it is now.
.*
:h3 res=1068 id=grid.Grid
:i1.grid
:p.Choose :hp2.Grid:ehp2. (Ctrl+G) to draw a line across the graph at every
10 percent, or to take the lines away again.
.*
:h3 res=1070 id=rate.Sample Rate
:i1.sample rate
:p.Choose :hp2.Sample Rate:ehp2. (Ctrl+A) to set how often the monitor
measures the processors, in milliseconds&colon. from 50 to 30000; it starts
at 150. A short time shows quick changes; a long one shows the load over a
longer period, and the line graph then covers more time.
:p.Type the number and choose :hp2.OK:ehp2..
.*
:h2 res=1055 id=product.Product information
:p.Shows the name and version of the CPU Monitor.
:euserdoc.
