#!/usr/bin/env python3
"""Records `tacmon --demo` as an asciicast (docs/demo/demo.cast): runs it in a pseudo-terminal of a fixed size,
presses the keys in SCRIPT at their times, and writes what it drew. `make demo-video` turns that into a GIF."""
import fcntl, json, os, pty, select, struct, sys, termios, time

COLS, ROWS = 150, 38
# (seconds to wait, keys to press): the start screen plays for about 3.4 s before the first page
SCRIPT = [(7.5, "2"), (3.0, "w"), (2.0, "w"), (1.2, "3"), (3.5, "4"), (3.5, "5"), (4.5, "1"), (2.5, "q")]

def main(binary, out):
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.update(TERM="xterm-256color", COLORTERM="truecolor")
        os.execv(binary, [binary, "--demo"])
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
    start, events, steps = time.time(), [], list(SCRIPT)
    due = start + steps[0][0]
    while True:
        ready, _, _ = select.select([fd], [], [], 0.02)
        if ready:
            try:
                data = os.read(fd, 65536)
            except OSError:
                break  # the child has exited and closed its side
            if not data:
                break
            events.append([round(time.time() - start, 4), "o", data.decode("utf-8", "replace")])
        if steps and time.time() >= due:
            os.write(fd, steps.pop(0)[1].encode())
            if steps:
                due = time.time() + steps[0][0]
    os.waitpid(pid, 0)
    with open(out, "w") as f:
        f.write(json.dumps({"version": 2, "width": COLS, "height": ROWS, "env": {"TERM": "xterm-256color"}}) + "\n")
        f.writelines(json.dumps(e) + "\n" for e in events)
    print(f"{out}: {len(events)} frames, {events[-1][0]:.1f} s")

if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
