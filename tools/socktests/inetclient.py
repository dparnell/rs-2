"""Connect to 127.0.0.1:PORT (retrying), send two lines, print what comes back."""
import socket, sys, time
port = int(sys.argv[1])
deadline = time.time() + float(sys.argv[2]) if len(sys.argv) > 2 else time.time() + 120
while time.time() < deadline:
    try:
        s = socket.create_connection(('127.0.0.1', port), timeout=5)
        s.settimeout(5)
        banner = s.recv(1000)
        if not banner:
            s.close(); time.sleep(1); continue
        print('banner:', banner, flush=True)
        for line in (b'hello inetd\r\n', b'second line\r\n'):
            s.sendall(line)
            time.sleep(1)
            print('reply:', s.recv(1000), flush=True)
        s.close()
        break
    except OSError as e:
        time.sleep(1)
print('done', flush=True)
