.* PMFORMAT.HLP: help for rs2's Format window (userland/pmformat,
.* OS2\PMFORMAT.EXE).
.*
.* Written for rs2. The res= numbers are the panels the program's help
.* table asks for (the numbers OS/2's Format uses). xtask compiles this
.* file with crates/ipf into OS2\HELP\PMFORMAT.HLP.
:userdoc.
:title.Format Help
.*
:h1 res=8063 id=disk.Format Disk
:i1.Format
:i1.formatting a disk
:p.Formatting prepares a drive to hold files by writing a new, empty file
system on it. :hp2.Everything on the drive is lost:ehp2.&colon. copy the
files you want to keep elsewhere first. You are asked to confirm before a
hard disk is formatted.
:p.Open this window from a drive's pop-up menu (:hp2.Format disk:ehp2.) or
type :hp2.PMFORMAT:ehp2. followed by the drive letter, for example
:hp2.PMFORMAT D&colon.:ehp2..
:dl tsize=22.
:dt.Volume label
:dd.A name for the drive, up to 11 characters, shown in its folder and
by commands such as DIR. It may be left empty. Some characters, such as
* ? . , ; &colon. and \, cannot be used.
:dt.File System Type
:dd.Choose how the files are organized&colon.
:ul compact.
:li.:hp2.HPFS:ehp2. (High Performance File System)&colon. long file names,
large drives, little fragmentation.
:li.:hp2.JFS:ehp2. (Journaled File System)&colon. very large drives and
quick recovery after the system was stopped without shutting down.
:li.:hp2.FAT:ehp2. (File Allocation Table)&colon. the file system DOS and
other systems also read; names of up to 8 characters plus a 3-character
extension.
:eul.
The drive's current file system is selected when the window opens.
:dt.Format
:dd.Starts formatting; the progress is shown in another window.
:dt.Cancel
:dd.Closes the window without formatting.
:edl.
:p.The drive the system was started from, and drives whose files are in
use, cannot be formatted.
:p.The formatting itself is done by the FORMAT command, which you can also
use at a command prompt.
.*
:h1 res=8069 id=diskette.Format Diskette
:i1.diskettes
:p.Formatting a diskette erases everything on it.
:dl tsize=22.
:dt.Volume label
:dd.A name for the diskette, up to 11 characters (it may be left
empty).
:dt.Capacity
:dd.How much the diskette is to hold. The list has the sizes the drive can
write; choose the one the diskette is made for (it is printed on most
diskettes, for example :hp2.1.44MB:ehp2. on high-density 3.5-inch
diskettes).
:dt.Format
:dd.Starts formatting.
:dt.Cancel
:dd.Closes the window without formatting.
:edl.
.*
:h1 res=8677 id=removable.Format a removable disk
:p.Formatting a removable disk erases everything on it. Type a volume label
of up to 11 characters (or leave it empty) and choose :hp2.Format:ehp2..
.*
:h1 res=8678 id=optical.Format Optical Disk
:p.Formatting an optical disk erases everything on it. Type a volume label
of up to 11 characters (or leave it empty).
:p.Select :hp2.Long format:ehp2. to have every part of the disk checked
while it is formatted&colon. this finds damaged areas but can take a long
time.
.*
:h1 res=8072 id=progress.Format Progress
:i1.format progress
:p.This window follows the formatting&colon. the bar shows how much is done,
and when the file system has been written the total space of the drive
and the space available for files are shown.
:dl tsize=12.
:dt.OK
:dd.Closes the window once formatting is complete.
:dt.Stop
:dd.Stops formatting. A drive whose formatting was stopped has no usable
file system&colon. format it again before using it.
:edl.
:p.If formatting fails, a message says why.
:euserdoc.
