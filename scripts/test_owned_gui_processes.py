import os
import signal
import subprocess
import unittest
import uuid
from owned_gui_processes import OwnedGuiProcesses


class OwnershipTests(unittest.TestCase):
    def test_cleanup_terminates_only_owned_processes_with_the_same_executable(self):
        owner = OwnedGuiProcesses("/bin/sleep", str(uuid.uuid4()), ":51")
        owned_env = dict(os.environ, **owner.environment())
        unrelated_env = dict(os.environ, DISPLAY=":51")
        unrelated_env.pop("KOOLADE_GUI_TEST_SCOPE", None)
        other_run_env = dict(owned_env, KOOLADE_GUI_TEST_SCOPE=str(uuid.uuid4()))
        wrong_display_env = dict(owned_env, DISPLAY=":1")
        children = [subprocess.Popen(["/bin/sleep", "30"], env=env)
                    for env in (owned_env, unrelated_env, other_run_env, wrong_display_env)]
        try:
            self.assertEqual(owner.pids(), [children[0].pid])
            self.assertFalse(owner.send(os.getpid()))
            for child in children[1:]:
                self.assertFalse(owner.send(child.pid))
            self.assertTrue(owner.send(children[0].pid))
            self.assertEqual(children[0].wait(timeout=3), -signal.SIGTERM)
            self.assertTrue(all(child.poll() is None for child in children[1:]))
            self.assertFalse(owner.send(children[0].pid))
        finally:
            for child in children:
                if child.poll() is None:
                    child.terminate()
                child.wait(timeout=3)

    def test_wrong_executable_cannot_claim_a_process(self):
        owner = OwnedGuiProcesses("/bin/false", str(uuid.uuid4()), ":51")
        child = subprocess.Popen(["/bin/sleep", "30"], env=dict(os.environ, **owner.environment()))
        try:
            self.assertFalse(owner.owns(child.pid))
            self.assertFalse(owner.send(child.pid))
            self.assertIsNone(child.poll())
        finally:
            child.terminate()
            child.wait(timeout=3)


if __name__ == "__main__":
    unittest.main()
