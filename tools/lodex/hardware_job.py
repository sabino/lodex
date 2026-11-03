#!/usr/bin/env python3
"""Exclude overlapping uploads and experiments from this Linux controller."""
import argparse
import fcntl
import os
from pathlib import Path
import signal
import subprocess


DEFAULT_LOCK = Path(f"/tmp/lodex-hardware-{os.getuid()}.lock")


class HardwareJob:
    """Advisory, fail-fast lock; all jobs for a device must use the same file.

    The default conservatively serializes all Lodex hardware jobs for this user.
    This does not coordinate other computers or commands that bypass the lock.
    Kernel ownership avoids a stale PID marker after a process exits. Inspect
    remote jobs after abnormal termination: this lock is not remote cleanup.
    Never unlink the file: waiters must continue to refer to the same inode.
    """

    def __init__(self, path=DEFAULT_LOCK):
        self.path = Path(path)
        self.fd = None

    def __enter__(self):
        fd = os.open(self.path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            os.close(fd)
            raise RuntimeError(f"another hardware job holds {self.path}") from error
        except BaseException:
            os.close(fd)
            raise
        self.fd = fd
        return self

    def __exit__(self, *_):
        os.close(self.fd)
        self.fd = None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock-file", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("supply command argv after --")
    with HardwareJob(args.lock_file) as job:
        # Keep exclusion if the wrapper dies while its command is still alive.
        child = subprocess.Popen(command, start_new_session=True, pass_fds=(job.fd,))

        def forward(signum, _frame):
            try:
                os.killpg(child.pid, signum)
            except ProcessLookupError:
                pass

        for signum in (signal.SIGTERM, signal.SIGINT):
            signal.signal(signum, forward)
        code = child.wait()
    raise SystemExit(code if code >= 0 else 128 - code)


if __name__ == "__main__":
    main()
