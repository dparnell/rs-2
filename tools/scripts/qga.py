"""Talk to a guest's QEMU guest agent as Proxmox and libvirt do: through
the socket of its `org.qemu.guest_agent.0` port (`cargo xtask run
--agent` puts it on work/qemu-ga.sock).

usage: python3 -I qga.py SOCKET COMMAND [JSON-ARGUMENTS] [--no-reply]

First a guest-sync-delimited (0xFF, then the reply after the guest's
0xFF), then the command; prints the reply. --no-reply: the command answers
nothing when it works (guest-shutdown); waits briefly for an error.
"""
import json, random, socket, sys


def main():
    a = [x for x in sys.argv[1:] if x != '--no-reply']
    no_reply = '--no-reply' in sys.argv
    if len(a) < 2:
        sys.exit(__doc__)
    path, cmd = a[0], a[1]
    args = json.loads(a[2]) if len(a) > 2 else None
    s = socket.socket(socket.AF_UNIX)
    s.connect(path)
    s.settimeout(10)
    f = s.makefile('rb')
    n = random.randrange(1, 1 << 31)
    s.sendall(b'\xff' + json.dumps({'execute': 'guest-sync-delimited', 'arguments': {'id': n}}).encode() + b'\n')
    # Skip to the guest's 0xFF, then the reply line.
    while f.read(1) != b'\xff':
        pass
    r = json.loads(f.readline())
    assert r.get('return') == n, r
    msg = {'execute': cmd}
    if args is not None:
        msg['arguments'] = args
    s.sendall(json.dumps(msg).encode() + b'\n')
    if no_reply:
        s.settimeout(3)
    try:
        line = f.readline()
    except socket.timeout:
        line = b''
    if line:
        print(json.dumps(json.loads(line), indent=2, ensure_ascii=False))
    elif no_reply:
        print('(no reply: done)')
    else:
        sys.exit('no reply')


main()
