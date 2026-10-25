from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).parent / "dist" / "site-packages"))
from koolade_runtime_qa_probe import marker
assert marker() == "prepared pip wheel installed offline"
