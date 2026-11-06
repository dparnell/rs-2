"""rs2's Package Manager's window onto yum.

On a pipe yum shows no progress. With RS2_YUM_PROGRESS=1 in the environment
this plugin gives it a download meter, and follows the RPM transaction, and
prints what it sees as lines the Package Manager reads:

    @DL read total name       bytes of all the downloads, the current file
    @TX n m permille name     the transaction's step n of m, how far overall

Anywhere else it does nothing. PKGMGR.EXE installs it.
"""
import os
import sys

from urlgrabber.progress import BaseMeter
from yum.plugins import TYPE_CORE

requires_api_version = '2.3'
plugin_type = (TYPE_CORE,)

ON = os.environ.get('RS2_YUM_PROGRESS') == '1'


def say(text):
    sys.stdout.write('\n@%s\n' % text)
    sys.stdout.flush()


class Meter(BaseMeter):
    """Bytes of all the files, not of one."""

    def __init__(self, total):
        BaseMeter.__init__(self)
        self.total = total
        self.done = 0

    def show(self, read):
        name = self.basename or self.text or ''
        say('DL %d %d %s' % (self.done + (read or 0), self.total, name))

    def _do_start(self, now=None):
        self.show(0)

    def _do_update(self, amount_read, now=None):
        self.show(amount_read)

    def _do_end(self, amount_read, now=None):
        self.done += amount_read or 0
        self.show(0)


def to_fetch(po):
    """The bytes of a package, unless it is in the cache already."""
    path = po.localPkg()
    if os.path.exists(path) and os.path.getsize(path) == po.size:
        return 0
    return po.size


def predownload_hook(conduit):
    if ON:
        total = sum(to_fetch(po) for po in conduit.getDownloadPackages())
        conduit._base.repos.setProgressBar(Meter(total))


def follow_transaction():
    """Wrap the command line's RPM callback: yum has no hook for it."""
    output = sys.modules.get('output')
    if output is None or not hasattr(output, 'YumCliRPMCallBack'):
        return
    event = output.YumCliRPMCallBack.event
    last = [None]

    def follow(self, package, action, te_current, te_total, ts_current, ts_total):
        if ts_total:
            part = te_total and float(te_current) / te_total or 0
            permille = int(1000 * (ts_current - 1 + part) / ts_total)
            if permille != last[0]:
                last[0] = permille
                say('TX %d %d %d %s' % (ts_current, ts_total, permille, package))
        return event(self, package, action, te_current, te_total, ts_current, ts_total)

    output.YumCliRPMCallBack.event = follow


def init_hook(conduit):
    if ON:
        follow_transaction()
