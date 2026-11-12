"""Talk to a guest's QEMU guest agent as Proxmox and libvirt do: through
the socket of its `org.qemu.guest_agent.0` port (`cargo xtask run
--agent` puts it on work/qemu-ga.sock).

usage: python3 -I qga.py SOCKET COMMAND [JSON-ARGUMENTS] [--no-reply]
       python3 -I qga.py SOCKET --exec PROGRAM [ARGUMENT...]

First a guest-sync-delimited (0xFF, then the reply after the guest's
0xFF), then the command; prints the reply. --no-reply: the command answers
nothing when it works (guest-shutdown); waits briefly for an error.
--exec: as `qm guest exec`: guest-exec with the output captured (standard
input from ours when it is not a terminal), then guest-exec-status until
the program ends; prints its output and ends with its exit code.
"""
import base64, json, os, random, socket, sys, time


def ask(s, f, cmd, args=None):
    msg = {'execute': cmd}
    if args is not None:
        msg['arguments'] = args
    s.sendall(json.dumps(msg).encode() + b'\n')
    r = json.loads(f.readline())
    if 'error' in r:
        sys.exit(f"{cmd}: {r['error']['desc']}")
    return r['return']


def run(s, f, argv):
    args = {'path': argv[0], 'arg': argv[1:], 'capture-output': True}
    if not sys.stdin.isatty():
        args['input-data'] = base64.b64encode(sys.stdin.buffer.read()).decode()
    pid = ask(s, f, 'guest-exec', args)['pid']
    while True:
        st = ask(s, f, 'guest-exec-status', {'pid': pid})
        if st['exited']:
            break
        time.sleep(0.2)
    for k, out in (('out', sys.stdout), ('err', sys.stderr)):
        out.buffer.write(base64.b64decode(st.get(k + '-data', '')))
        if st.get(k + '-truncated'):
            print(f'({k} truncated)', file=sys.stderr)
    sys.stdout.flush()
    if 'signal' in st:
        print(f'(ended by signal {st["signal"]})', file=sys.stderr)
        sys.exit(128 + st['signal'])
    sys.exit(st.get('exitcode', 0))


def main():
    a = [x for x in sys.argv[1:] if x != '--no-reply']
    no_reply = '--no-reply' in sys.argv
    if len(a) < 2 or (a[1] == '--exec' and len(a) < 3):
        sys.exit(__doc__)
    path, cmd = a[0], a[1]
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
    if cmd == '--exec':
        return run(s, f, a[2:])
    args = json.loads(a[2]) if len(a) > 2 else None
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
