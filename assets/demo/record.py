import os, pty, time, json, sys, select, struct, fcntl, termios

COLS, ROWS = 100, 32
OUT_CAST = sys.argv[1]

def set_winsize(fd, rows, cols):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

events = []
t_start = None

def emit(data):
    global t_start
    now = time.time()
    if t_start is None:
        t_start = now
    events.append([round(now - t_start, 6), "o", data.decode("utf-8", "replace")])

pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.environ["BASH_SILENCE_DEPRECATION_WARNING"] = "1"
    os.environ["PS1"] = "$ "
    os.environ["PATH"] = os.path.abspath("target/release") + ":" + os.environ["PATH"]
    os.chdir(os.path.dirname(os.path.abspath(__file__)) + "/../..")
    os.execv("/bin/bash", ["bash", "--norc", "--noprofile"])
else:
    set_winsize(fd, ROWS, COLS)

    def drain(seconds):
        end = time.time() + seconds
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.03)
            if r:
                try:
                    d = os.read(fd, 65536)
                except OSError:
                    return
                if d:
                    emit(d)

    def send(s, settle=0.05):
        os.write(fd, s.encode())
        drain(settle)

    drain(0.3)
    send("clear\n", 0.2)
    send("treeq assets/demo/sample.json\n", 1.2)
    for _ in range(5):
        send("\x1bOB", 0.15)  # Down arrow
    drain(0.6)
    send("\x1bOA", 0.15)  # Up
    send(" ", 0.4)        # Space: expand
    for _ in range(2):
        send("\x1bOB", 0.15)
    drain(0.6)
    send("1", 0.6)        # tag
    send("i", 1.0)         # inspect
    send("\x1b", 0.3)      # Esc
    send("F", 0.4)
    send("onca", 0.9)      # fuzzy query
    send("\r", 0.9)
    send("/", 0.2)
    send("p99", 0.9)
    send("\x1b", 0.4)
    send("c", 0.7)
    send("e", 0.7)
    send("q", 0.4)
    drain(0.3)
    try:
        os.kill(pid, 9)
    except OSError:
        pass
    os.waitpid(pid, 0)

with open(OUT_CAST, "w") as f:
    header = {"version": 2, "width": COLS, "height": ROWS, "timestamp": int(time.time()),
              "env": {"TERM": "xterm-256color", "SHELL": "/bin/bash"}}
    f.write(json.dumps(header) + "\n")
    for ev in events:
        f.write(json.dumps(ev) + "\n")
print(f"wrote {len(events)} events to {OUT_CAST}")
