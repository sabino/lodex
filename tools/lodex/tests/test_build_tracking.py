"""Exercise the vendor helper itself: Linux builds must not watch Git refs."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class BuildTrackingTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "native Unix build-script behavior")
    def test_windows_resource_helpers_are_git_independent_on_unix(self):
        root = Path(__file__).resolve().parents[3]
        helper = root / "vendor/mxc/src/mxc-sdk/build/build_mxc_build_common.rs"
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            source = directory / "probe.rs"
            source.write_text(
                '#[allow(dead_code)]\n#[path = ' + json.dumps(str(helper)) + '] mod helper;\n'
                'fn main() { helper::embed_version_info("test", "test.exe"); '
                'helper::embed_version_info_for_binary("test", "test", "test.exe").expect("no-op"); }\n')
            binary = directory / "probe"
            env = {**os.environ, "RUSTUP_TOOLCHAIN": "1.98.0", "CARGO_CFG_TARGET_OS": "linux"}
            subprocess.run(["rustc", "--edition", "2024", str(source), "-o", str(binary)],
                           env=env, check=True, capture_output=True, text=True, timeout=30)
            result = subprocess.run([str(binary)], cwd=root, env=env, check=True,
                                    capture_output=True, text=True, timeout=10)
            self.assertIn("cargo:rerun-if-changed=build.rs", result.stdout)
            self.assertNotIn(".git/", result.stdout)
