"""Linux GUI-test process ownership. Never select kill targets by command-line text.

Pass a unique test-run scope into the child's launch environment. Detached app
windows inherit it. Match scope, display and exact executable, then signal via a
pidfd so PID reuse cannot redirect a signal to another process.
"""
import os
import signal

MARKER = "PACKET_GUI_TEST_SCOPE"


class OwnedGuiProcesses:
    def __init__(self, executable, scope, display):
        if not scope or not display:
            raise ValueError("A nonempty test-run scope and display are required")
        self.executable = os.path.realpath(executable)
        self.scope = scope
        self.display = display

    def environment(self):
        return {MARKER: self.scope, "DISPLAY": self.display}

    def owns(self, pid):
        if pid <= 1 or pid == os.getpid():
            return False
        try:
            # Even a mistakenly inherited marker never authorizes killing the
            # driver or the application/shell that launched this test session.
            ancestor = os.getpid()
            while ancestor > 1:
                if pid == ancestor:
                    return False
                with open(f"/proc/{ancestor}/stat") as stream:
                    ancestor = int(stream.read().rsplit(")", 1)[1].split()[1])
            if os.path.realpath(f"/proc/{pid}/exe") != self.executable:
                return False
            with open(f"/proc/{pid}/environ", "rb") as stream:
                values = stream.read().split(b"\0")
            return all(f"{key}={value}".encode() in values
                       for key, value in self.environment().items())
        except (OSError, ValueError, IndexError):
            return False

    def pids(self):
        return [int(name) for name in os.listdir("/proc")
                if name.isdigit() and self.owns(int(name))]

    def send(self, pid, sig=signal.SIGTERM):
        # Fail closed on platforms without pidfds; never fall back to broad
        # process searches or a potentially recycled numeric PID.
        fd = None
        try:
            fd = os.pidfd_open(pid)
            if not self.owns(pid):
                return False
            signal.pidfd_send_signal(fd, sig)
            return True
        except ProcessLookupError:
            return False
        finally:
            if fd is not None:
                os.close(fd)
