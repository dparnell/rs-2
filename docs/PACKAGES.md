# OS/2 package managers

The package managers of the OS/2 world are unmodified OS/2 programs too:
RPM and yum (ports by bitwiseworks and netlabs, in Python 2.7 and C with
kLIBC), WarpIN (the classic installer: WPI archives, a PM wizard, WPS
objects, CONFIG.SYS changes and a database of what it installed) and
Arca Noae's ANPM (a VX-REXX front end to yum). Nothing here is part of
the repository or of rs2's images: everything is downloaded from the
projects' own servers as test input, unpacked on the host with tools that
only read it (`bsdtar`), and run only inside QEMU.

| Program | Status | Notes |
|---------|--------|-------|
| RPM 4.15.1 | works | `--version`, `--initdb`, `-ivh` (Berkeley DB 4.8 in `/var/lib/rpm`), `-qa`, `-qf`, `-q`, `-V`, `--scripts`. |
| Python 2.7.18 | works | The interpreter yum needs, with its `.pyd` modules (bz2, sqlite3, pycurl, ...). |
| yum 3.4.3 | works | `repolist`, `list`, `search`, `info`, `install` and `remove` against `rpm.netlabs.org`, downloading over HTTP through rs2's network stack. See below. |
| WarpIN 1.0.19 | works | The self-installer unpacks itself and shows its wizard; `WARPIN.EXE` installs a WPI under the Workplace Shell (files, CONFIG.SYS with a backup, a program object on the desktop, its own folder and objects, its database). `WIC.EXE` builds, lists and unpacks archives. |
| ANPM 1.1.2 | fails | Needs WarpIN 1.0.24 to install; unpacked with WIC, its VX-REXX runtime loads and stops in SOM (see below). |

## The test disk

```
mkdir -p work/pk/meta work/pk/rpms work/pk/root
curl -O .../i386/repodata/repomd.xml             # rpm.netlabs.org/release/00/i386
curl -O .../repodata/<hash>-primary.sqlite.bz2   # named in repomd.xml; bunzip2 to primary.sqlite
python3 -I tools/scripts/rpmclosure.py work/pk/meta/primary.sqlite \
    yum python2-rpm python2.7 rpm rpm-libs bww-resources-rpm os2-rpm coreutils bash > work/pk/closure.txt
# fetch each `.../i386/<location>` into work/pk/rpms, then in work/pk/root:
bsdtar -xf ../rpms/X.rpm                         # for each of them
python3 -I tools/scripts/lximports.py work/pk/root/@unixroot work/pk/meta/primary.sqlite
```

The closure is 78 packages, 81 MB. `rpmclosure.py` does not compare
versions, so `lximports.py` shows what is left: the DLLs of `openssl1.1`,
`compat-openssl10`, `readline-legacy-6` and `libgcc-fwd`. Add those, and
`noarch/netlabs-rel-0.0.0-6.oc00.noarch.rpm` for the repository file
(`/etc/yum/repos.d/netlabs-rel.repo`; `$releasever` 00, `$basearch` i386).
`python-unversioned-command` brings Python 3.13 along; yum itself runs
under `python2`.

```
python3 -I tools/scripts/rpmdisk.py              # writes work/disk.txt
cargo xtask disk --hpfs --size 1000 -o work/pk.img
```

`work/cfg/config.sys` (with `work/cfg/config.sys -> CONFIG.SYS` in
work/modules.txt, as usual) has what an ArcaOS CONFIG.SYS has:

```
LIBPATH=.;C:\OS2\DLL;C:\OS2;C:\RS2;C:\;E:\USR\LIB;E:\WARPIN
SET PATH=E:\USR\BIN;E:\WARPIN;C:\IBM\APPS;C:\IBM;C:\TK;C:\OS2;C:\RS2;C:\
SET UNIXROOT=E:
SET HOME=E:\HOME
SET TMP=E:\TMP
SET TEMP=E:\TMP
SET TMPDIR=E:\TMP
SET TERM=dumb
SET LOGNAME=root
SET USER=root
SET LANG=en_US
SET TZ=UTC0
```

The disk is drive E: with `--hdd work/pk.img`, the ESP D:. Commands go in
a `.CMD` file on C: (`work/cfg/p1.cmd -> P1.CMD` in work/modules.txt, CRLF
line ends) run by `--init "CMD.EXE /C P1.CMD"`; a kLIBC program's output
reaches the serial log (`work/serial.log`). QEMU's user network works as it
is (`--net`: DHCP, DNS, outbound HTTP); about 100 KB/s under TCG with
yum's progress bars.

```
cargo xtask run --headless --mem 1G --smp 2 --hdd work/pk.img --net \
    --timeout 1800 --init "CMD.EXE /C P2.CMD" --screenshot work/p2.png
```

`--timeout` is what ends the run: stop QEMU when the serial log says
`ended, code`.

## RPM

The disk's files were unpacked on the host, so the RPM database knows
nothing of them yet. One run tells it, with the RPMs copied to `E:\rpms`:

```
rpm --initdb
rpm -ivh --justdb --nodeps --noscripts --notriggers --force E:/rpms/*.rpm
rpm -qa | wc -l                                  # 83
rpm -qf E:/usr/bin/bash.exe                      # bash-5.0.11-3.oc00.pentium4
```

## yum

```
yum repolist                                     # netlabs-rel, 8716 packages
yum list available 'zi*'
yum search compress
yum -y install zip                               # 322 KB, downloaded by yum
zip z.zip z.txt
yum -y install less wget tar unzip diffutils uclip
yum -y remove zip                                # and what depends on it
yum info zip
```

All of it runs in rs2: yum takes `primary.sqlite.bz2` from the repository
(sqlite through Python's `_sqlite3`, the file locks and the bz2 module),
fetches with pycurl, checks with RPM, installs with Berkeley DB and
`librpm`, and the programs installed then run (`zip`, `unzip`, `tar`,
`wget -O` from the same server, `less --version`, `diff --version`).
`rpm -V bash` reports the files `rpmdisk.py` left out (documentation,
locales) and modes and times the HPFS image does not keep.

`loader: module libproviders.dll not found` at each start is OpenSSL 3
looking for a provider DLL; it goes on without it.

## WarpIN

WarpIN's site (`warpin.netlabs.org`) and `ftp://ftp.netlabs.org/pub/warpin/`
stop at 1.0.19, `warpin-1-0-19.exe`; the project's Trac wiki points to
Hobbes for 1.0.24 and 1.0.26. It is not in the RPM repository (only
`wpi4rpm`, its database entry for RPMs).

The exe is a stub and a WPI archive. Run as the boot program it unpacks
`WPIRTL.DLL` and `WARPIN.EXE` to `%TMP%` and starts them:

```
cargo xtask run --headless --mem 1G --hdd work/pk.img --init 'C:\WARPIN19.EXE' --timeout 120 ...
```

The wizard's second page is the GPL with an "I agree" button. rs2's tests
do not press it (nobody here may agree to terms on the user's behalf), so
the install below takes WarpIN's own files out of the archive instead:
each file in a WPI is a bzip2 stream, and `WARPIN.EXE`, `WPIRTL.DLL`,
`WIC.EXE`, `WICPM.EXE`, `STUB.EXE`, `WARPIN.TMF` and `WARPIN.HLP` go to
`E:\WARPIN`. On its first start `WARPIN.EXE` registers itself in OS2.INI and
offers to create its Workplace Shell folder, and does (`WinCreateObject`
from the second process, into the running shell).

A WPI built by WIC inside rs2 from a script of ours (a package, a
`CREATEOBJECT` on the desktop, `WRITEPROFILE`, `CONFIGSYS`, no licence
page):

```
E:\WARPIN\WIC.EXE E:\DEMO.WPI -a 1 *.* -s E:\WARPIN\DEMO.WIS
E:\WARPIN\WIC.EXE E:\DEMO.WPI -l
```

`\STARTUP.CMD` (the shell runs it after the Startup folder) starts WarpIN
under the Workplace Shell, once to register and once with the archive:

```
E:
cd \WARPIN
E:\WARPIN\WARPIN.EXE
E:\WARPIN\WARPIN.EXE E:\DEMO.WPI
exit
```

```
cargo xtask run --headless --mem 1G --hdd work/pk.img --net --desktop --timeout 335 \
    --screenshot work/d5.png --type-at 155 --type '\n{sleep 15}\n{sleep 25}\n{sleep 12}{mouse_abs 13728 26026}{sleep 1}{mouse_abs 13728 26026}{sleep 1}{mouse_button 1}{sleep 1}{mouse_button 0}{sleep 12}{mouse_button 1}{sleep 1}{mouse_button 0}{sleep 10}{shot d5d}\n{sleep 20}{shot d5e}\n{sleep 15}{shot d5f}'
```

The keys: Yes, OK, Next; the clicks are the wizard's Next and Install
(Enter on the package container is CN_ENTER, not Next); Yes to "This will
install 1 package(s)"; OK. WarpIN ends with "Installation has completed":
the file in `E:\DEMO`, `SET WPIDEMO=E:\DEMO` in CONFIG.SYS with the old one
kept as `C:\CONFIG.001`, and the "WPI demo" icon on the desktop.
Screenshots: work/progress/pkgmgr.

Not looked at: Tab does not move the focus in WarpIN's message boxes (the
XWP helpers' hand-made dialogs); the wizard's text is dark blue where it
should be black; de-installing from WarpIN's database view; WPS class
registration (`REGISTERCLASS`).

## ANPM

`repos.arcanoae.com/anpm/anpm_1_1_2.exe` is a free download without a login,
and its script has no licence page. It says `<WARPIN VERSION="1.0.24">`,
newer than the official 1.0.19, so it was not installed but unpacked with
WIC inside rs2 (`WIC anpm.exe -x 1` to `-x 9`). `anpm.exe` is a VX-REXX
program: `PMEXE` and VROBJ.DLL (Watcom C, SOM 1 classes) with the REXX code in
its resources. Run with those on the LIBPATH it loads VROBJ.DLL, REXX and the
SOM classes, and ends with `SOM error 20169` (`somOverrideSMethod: no method
somClassReady in class M_VRCore`): `somInitClass` gets a parent (a 16-byte
object next to the class's own) that rs2's SOM does not take for a class, so
M_VRCore descends from SOMObject instead of SOMClass. The next step is to see
in Ghidra what VROBJ passes there and what IBM's SOMClass does with it.

## What rs2 needed

In the order the programs met them (each a separate commit):

1. **DosQuerySysInfo** takes indexes up to 32, as IBM's kernel does (its
   _VR32QuerySysInfo): Python's `os2` module reads 1 to the Toolkit's
   QSV_MAX, 31.
2. **The loader's module classes.** A DLL loaded by its path is a module of
   its own (OS/2's CLASS_SPECIFIC): imports and loads by name do not find
   it, a path finds the module of that file. `lib-dynload\bz2.pyd` has the
   internal name of the `BZ2.DLL` it imports. If the LIBPATH has no such
   DLL, a module of that name loaded by its path will do (an import
   OS/2 would refuse).
3. **16-bit entries keep the high halves** of EBX, ESI, EDI and EBP
   (thunk16): `zip` calls NETAPI's NETACCESSGETINFO for a file's ACLs
   through kLIBC's `__libc_thunk1`, which keeps a flat pointer in ESI.
4. **PMWIN, PMWP**: WinWindowFromDC, WinGetFrameTreePPSize,
   WinGetFrameTreePPs (the layout of IBM's GetWindowTreePPs: each window's
   PARAMs, then its children's with their ids), WinRestartWorkplace (FALSE),
   and for VX-REXX WinIsThreadActive and WinQuerySessionTitle.
5. **GpiIntersectClipRectangle and GpiExcludeClipRectangle** bring the
   visible region of a kept PS up to date before clipping from it: one
   made while the window was hidden clipped everything away (WarpIN's text
   view keeps a micro PS on the window's DC).
6. **WinSetWindowText** sends WM_SETWINDOWPARAMS to a window of the program's
   own class (the same text view keeps its text from there).
7. **Containers**: as in IBM's SetEmphasisBits, every emphasis change
   reaches the owner as CN_EMPHASIS, however it was made (not target
   emphasis or filtering), and as in CnrInsertRecord, the first record
   inserted into an empty container gets the cursor (and the selection,
   unless the container is CCS_MULTIPLESEL). WarpIN's package page reads
   its current record from that notification.
8. **WinProcessDlg** runs any frame as a dialog: the XWP helpers build
   their message boxes with WinCreateWindow and subclass the frame.
9. **\STARTUP.CMD** runs after the Startup folder.
10. **Exception handlers** start with the report in EAX and the registration
    in ECX, as DOSCALL1's dispatcher leaves them: Watcom's handlers (VROBJ)
    read their arguments from registers. They faulted on 0, and again in the
    nested dispatch, when the thread ended.

RPM, Berkeley DB, kLIBC's fork for scriptlets, sqlite's locks, pycurl and
HPFS needed nothing new.
