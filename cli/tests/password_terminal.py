"""Exercise the Rust test fixture through a real PTY using synthetic credentials."""

import errno
import os
import pty
import re
import select
import subprocess
import sys
import termios
import time


def read_until(fd, marker, timeout=10):
    output = bytearray()
    deadline = time.monotonic() + timeout
    while marker not in output and time.monotonic() < deadline:
        if select.select([fd], [], [], 0.1)[0]:
            try:
                data = os.read(fd, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    break
                raise
            if not data:
                break
            output.extend(data)
    assert marker in output, f"terminal did not reach {marker!r}: {output!r}"
    return bytes(output)


def run_case(binary, case):
    master, slave = pty.openpty()
    original = termios.tcgetattr(slave)
    env = dict(os.environ, RWD_PASSWORD_TERMINAL_TEST=case, TERM="xterm-256color")
    child = subprocess.Popen(
        [binary, "config::interactive::terminal_tests::test_password_terminal_child",
         "--exact", "--nocapture"],
        stdin=slave, stdout=slave, stderr=slave, env=env,
    )
    try:
        read_until(master, b"TEST PASSWORD: ")
        secret = b"sk-test-" + b"PasteOnlySecret" * 32
        if case == "paste":
            chunks = [secret]
        elif case == "chunks":
            chunks = [secret[index:index + 17] for index in range(0, len(secret), 17)]
        elif case == "edit":
            chunks = [b"sk-test-wrong", b"\x7f" * 5, b"final"]
        elif case in ("escape", "interrupt"):
            chunks = [b"sk-test-cancelled"]
        else:
            chunks = []
        output = bytearray()
        for chunk in chunks:
            assert os.write(master, chunk) == len(chunk)
            # Allow output from both the driver and the password reader to accumulate.
            time.sleep(0.01)
        ending = {"escape": b"\x1b", "interrupt": b"\x03"}.get(case, b"\r")
        os.write(master, ending)
        output.extend(read_until(master, b"PASSWORD RESULT OK"))
        child.wait(timeout=5)
        assert child.returncode == 0, f"{case}: child failed"
        typed_output = re.sub(rb"\x1b\[[0-?]*[ -/]*[@-~]", b"", output.split(b"PASSWORD RESULT OK")[0])
        # No part of any credential may be emitted; only stars and terminal controls.
        assert not any(65 <= char <= 90 or 97 <= char <= 122 for char in typed_output), (
            f"{case}: plaintext leaked: {typed_output!r}"
        )
        if chunks:
            assert b"*" in typed_output, f"{case}: masking feedback missing"
        restored = termios.tcgetattr(slave)
        # PENDIN and other driver-managed flags can change as input is consumed.
        for index, flags in ((0, termios.ICRNL | termios.IXON), (1, termios.OPOST),
                             (3, termios.ECHO | termios.ICANON | termios.ISIG | termios.IEXTEN)):
            assert restored[index] & flags == original[index] & flags, (
                f"{case}: terminal settings not restored"
            )
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        os.close(master)
        os.close(slave)


if __name__ == "__main__":
    for test_case in ("paste", "chunks", "edit", "empty", "escape", "interrupt"):
        run_case(sys.argv[1], test_case)
        print(f"password terminal: {test_case} passed")
    result = subprocess.run(
        [sys.argv[1], "config::interactive::terminal_tests::test_password_terminal_child",
         "--exact", "--nocapture"],
        input=b"sk-test-never-read\n", capture_output=True, timeout=5,
        env=dict(os.environ, RWD_PASSWORD_TERMINAL_TEST="no-terminal"),
    )
    assert result.returncode == 0, "non-terminal input must fail without hanging"
    assert b"TEST PASSWORD:" not in result.stderr, "do not prompt without a secure terminal"
    print("password terminal: no-terminal passed")
