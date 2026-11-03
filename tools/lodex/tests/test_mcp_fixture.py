import json
from pathlib import Path
import subprocess
import sys
import unittest


class McpFixtureTests(unittest.TestCase):
    def test_legacy_stdio_discovery_call_resource_and_prompt(self):
        methods = [
            ("server/discover", {}),
            ("initialize", {"protocolVersion": "2025-11-25"}),
            ("tools/list", {}),
            ("tools/call", {"name": "echo", "arguments": {"text": "synthetic-canary"}}),
            ("resources/read", {"uri": "fixture://lodex/identity"}),
            ("prompts/get", {"name": "fixture"}),
            ("tools/call", {"name": "echo", "arguments": {"text": 12}}),
        ]
        messages = [{"jsonrpc": "2.0", "id": i, "method": method, "params": params}
                    for i, (method, params) in enumerate(methods)]
        messages.insert(2, {"jsonrpc": "2.0", "method": "notifications/initialized"})
        script = Path(__file__).parents[1] / "fixtures/mcp_fixture.py"
        result = subprocess.run([sys.executable, str(script)],
                                input="".join(json.dumps(m) + "\n" for m in messages),
                                capture_output=True, text=True, timeout=5, check=True)
        replies = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(replies), len(methods))
        self.assertEqual(replies[0]["error"]["code"], -32601)
        self.assertEqual(replies[1]["result"]["protocolVersion"], "2025-11-25")
        self.assertEqual(replies[2]["result"]["tools"][0]["name"], "echo")
        self.assertEqual(replies[3]["result"]["content"][0]["text"], "synthetic-canary")
        self.assertEqual(replies[4]["result"]["contents"][0]["text"], "lodex-mcp-fixture")
        self.assertEqual(replies[5]["result"]["messages"][0]["role"], "user")
        self.assertEqual(replies[6]["error"]["code"], -32602)
