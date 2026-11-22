import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))


def pytest_configure(config):
    # Discover the targets once. With pytest-xdist the controller does it and
    # the workers (which inherit the environment) read the file.
    if not hasattr(config, "workerinput") and "AI_RE_DISCOVERY_FILE" not in os.environ:
        d = tempfile.mkdtemp(prefix="ai_re-nixtests-")
        os.environ["AI_RE_DISCOVERY_FILE"] = os.path.join(d, "targets.json")
        config._ai_re_tmp = d


def pytest_unconfigure(config):
    d = getattr(config, "_ai_re_tmp", None)
    if d:
        import shutil
        shutil.rmtree(d, ignore_errors=True)
        os.environ.pop("AI_RE_DISCOVERY_FILE", None)
