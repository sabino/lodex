import importlib.util
from pathlib import Path
import sys
import unittest

spec = importlib.util.spec_from_file_location("rpc_probe", Path(__file__).parents[1] / "rpc_probe.py")
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

FIXTURE = '''
import json,sys
for line in sys.stdin:
 d=json.loads(line)
 if 'id' not in d:continue
 print(json.dumps({'method':'fixture/event','params':{}}),flush=True)
 if d['method']=='bad':r={'error':{'code':-32601,'message':'unknown'}}
 else:r={'result':{'ok':True}}
 r['id']=d['id']; print(json.dumps(r),flush=True)
'''


class RpcProbeTests(unittest.IsolatedAsyncioTestCase):
    async def test_parallel_responses_may_arrive_in_reverse_order(self):
        code = '''import sys,json
requests=[json.loads(sys.stdin.readline()) for _ in range(2)]
print(json.dumps({'method':'fixture/event','params':{}}),flush=True)
for d in reversed(requests):
 print(json.dumps({'id':d['id'],'result':{'seen':d['method']}}),flush=True)
for line in sys.stdin: pass
'''
        result = await probe.run([sys.executable, "-c", code], [
            {"parallel": [
                {"method": name, "checks": [{"path": ["result", "seen"], "equals": name}]}
                for name in ["first", "second"]]},
            {"wait_notification": "fixture/event"},
        ], 2)
        self.assertTrue(result["passed"])
        self.assertEqual(result["requests"][0]["parallel_group_size"], 2)

    async def test_parallel_group_is_bounded(self):
        with self.assertRaises(ValueError):
            await probe.run([sys.executable, "-c", FIXTURE], [
                {"parallel": [{"method": "good"}] * 17},
            ], 2)

    async def test_notifications_and_expected_errors(self):
        result = await probe.run([sys.executable, "-c", FIXTURE], [
            {"method": "initialized", "notification": True},
            {"method": "good", "checks": [{"path": ["result", "ok"], "equals": True}]},
            {"method": "bad", "expect_error": True},
        ], 2)
        self.assertTrue(result["passed"])
        self.assertEqual(result["notifications"], 2)

    async def test_early_exit_is_not_a_pass(self):
        with self.assertRaises(RuntimeError):
            await probe.run([sys.executable, "-c", "pass"], [{"method": "good"}], 2)

    async def test_wrong_result_is_not_a_pass(self):
        with self.assertRaises(AssertionError):
            await probe.run([sys.executable, "-c", FIXTURE], [
                {"method": "good", "checks": [{"path": ["result", "ok"], "equals": False}]}
            ], 2)

    async def test_unexpected_rpc_error_is_not_a_pass(self):
        with self.assertRaises(AssertionError):
            await probe.run([sys.executable, "-c", FIXTURE], [{"method": "bad"}], 2)

    async def test_wrong_response_id_is_not_a_pass(self):
        code = "import sys; sys.stdin.readline(); print('{\"id\":99,\"result\":{}}',flush=True)"
        with self.assertRaisesRegex(RuntimeError, "response ID"):
            await probe.run([sys.executable, "-c", code], [{"method": "good"}], 2)

    async def test_captured_reference_is_supplied_to_later_request(self):
        code = '''import sys,json
for line in sys.stdin:
 d=json.loads(line)
 result={'thread':{'id':'test-thread'}} if d['method']=='start' else {'seen':d['params']['threadId']}
 print(json.dumps({'id':d['id'],'result':result}),flush=True)
'''
        result = await probe.run([sys.executable, "-c", code], [
            {"method": "start", "capture": {"thread": ["result", "thread", "id"]}},
            {"method": "read", "params": {"threadId": {"$ref": "thread"}},
             "checks": [{"path": ["result", "seen"], "equals": "test-thread"}]},
        ], 2)
        self.assertTrue(result["passed"])

    async def test_unknown_reference_fails_before_sending(self):
        with self.assertRaises(KeyError):
            await probe.run([sys.executable, "-c", FIXTURE], [
                {"method": "read", "params": {"threadId": {"$ref": "missing"}}},
            ], 2)

    async def test_notification_before_ack_is_not_lost(self):
        result = await probe.run([sys.executable, "-c", FIXTURE], [
            {"method": "good"},
            {"wait_notification": "fixture/event"},
        ], 2)
        self.assertTrue(result["passed"])

    async def test_missing_notification_times_out(self):
        with self.assertRaises(TimeoutError):
            await probe.run([sys.executable, "-c", FIXTURE], [
                {"method": "good"},
                {"wait_notification": "fixture/missing"},
            ], 0.1)
