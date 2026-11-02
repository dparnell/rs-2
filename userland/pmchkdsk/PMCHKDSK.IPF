.* PMCHKDSK.HLP: help for rs2's Check Disk window (userland/pmchkdsk,
.* OS2\PMCHKDSK.EXE).
.*
.* Written for rs2. The res= numbers are the panels the program's help
.* table asks for (the numbers OS/2's Check Disk uses). xtask compiles
.* this file with crates/ipf into OS2\HELP\PMCHKDSK.HLP.
:userdoc.
:title.Check Disk Help
.*
:h1 res=8061 id=check.Check Disk
:i1.Check Disk
:i1.checking a disk
:p.Check Disk looks through the file system of a drive for damage&colon.
files whose space is recorded wrongly, lost pieces of data that belong to
no file, damaged directories and extended attributes. When it is done it
shows how the space on the drive is used.
:p.Open it from a drive's pop-up menu (:hp2.Check disk:ehp2.) or type
:hp2.PMCHKDSK:ehp2. followed by the drive letter, for example
:hp2.PMCHKDSK C&colon.:ehp2..
:dl tsize=24.
:dt.Write corrections to disk
:dd.Select this to have errors that are found repaired. The drive must
not be in use by other programs while it is repaired&colon. close the
programs that have files open on it first. A drive the system was
started from usually cannot be repaired while the system runs.
:p.Without it, the drive is only checked and nothing on it is changed.
:dt.Check
:dd.Starts checking the drive. When lost data is found and corrections are
to be written, you are asked whether to turn it into files; such files
are named FILEnnnn.CHK and are placed in the drive's root directory.
:dt.Cancel
:dd.Closes the window (and stops a check that is running).
:dt.Help
:dd.Shows this information.
:edl.
:p.The checking itself is done by the CHKDSK command, which you can also
use at a command prompt.
.*
:h1 res=8068 id=results.Check Disk - Results
:i1.disk usage
:p.This window shows what the check found&colon.
:dl tsize=24.
:dt.Type of file system
:dd.The file system of the drive&colon. FAT, HPFS or JFS.
:dt.Total disk space
:dd.The size of the drive, in bytes.
:dt.Space available for use
:dd.The space not yet used by files, in bytes.
:edl.
:p.The chart shows how the used space is divided&colon. directories, user
files, unusable (bad) blocks, extended attributes and space the file
system reserves for itself. The part cut out of the circle is the free
space. Only the parts the drive has are named beside the chart, each in
the color of its slice.
:p.Choose :hp2.Cancel:ehp2. to close the window.
:euserdoc.
