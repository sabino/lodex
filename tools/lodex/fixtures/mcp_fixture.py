#!/usr/bin/env python3
"""Legacy MCP test fixture, not a production MCP implementation.

Only deterministic synthetic data. Supports stdio or stateless JSON HTTP so the
actual upstream client can be exercised without an account or model request.
"""
import argparse
import json
import sys

URI = "fixture://lodex/identity"
IDENTITY = "lodex-mcp-fixture"


def respond(request):
    if "id" not in request:
        return None
    method, params = request.get("method"), request.get("params", {})
    result = None
    error = None
    if method == "initialize":
        result = {"protocolVersion": "2025-11-25", "serverInfo": {"name": IDENTITY, "version": "1.0.0"},
                  "capabilities": {"tools": {}, "resources": {}, "prompts": {}}}
    elif method == "ping":
        result = {}
    elif method == "tools/list":
        result = {"tools": [{"name": "echo", "description": "Return synthetic fixture text",
                             "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}},
                                             "required": ["text"]},
                             "annotations": {"readOnlyHint": True, "destructiveHint": False}}]}
    elif method == "tools/call":
        if params.get("name") == "echo":
            text = params.get("arguments", {}).get("text")
            if not isinstance(text, str):
                error = {"code": -32602, "message": "text must be a string"}
            else:
                result = {"content": [{"type": "text", "text": text}], "isError": False}
        else:
            error = {"code": -32602, "message": "unknown fixture tool"}
    elif method == "resources/list":
        result = {"resources": [{"uri": URI, "name": "identity", "mimeType": "text/plain"}]}
    elif method == "resources/templates/list":
        result = {"resourceTemplates": []}
    elif method == "resources/read":
        if params.get("uri") == URI:
            result = {"contents": [{"uri": URI, "mimeType": "text/plain", "text": IDENTITY}]}
        else:
            error = {"code": -32602, "message": "unknown fixture resource"}
    elif method == "prompts/list":
        result = {"prompts": [{"name": "fixture", "description": "Synthetic fixture prompt"}]}
    elif method == "prompts/get" and params.get("name") == "fixture":
        result = {"messages": [{"role": "user", "content": {"type": "text", "text": IDENTITY}}]}
    else:
        error = {"code": -32601, "message": "method not supported by legacy fixture"}
    return {"jsonrpc": "2.0", "id": request["id"], **({"error": error} if error else {"result": result})}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--http", action="store_true")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=0)
    args = parser.parse_args()
    if not args.http:
        for line in sys.stdin:
            if len(line) > 65536:
                raise ValueError("fixture request too large")
            response = respond(json.loads(line))
            if response is not None:
                print(json.dumps(response), flush=True)
        return
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *unused):
            pass

        def do_POST(self):
            size = int(self.headers.get("Content-Length", "0"))
            if self.path != "/mcp" or size <= 0 or size > 65536:
                self.send_error(400)
                return
            try:
                response = respond(json.loads(self.rfile.read(size)))
            except (ValueError, TypeError):
                self.send_error(400)
                return
            data = json.dumps(response).encode() if response is not None else b""
            self.send_response(200 if response is not None else 202)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_GET(self):
            self.send_error(405)

        def do_DELETE(self):
            self.send_error(405)

    with ThreadingHTTPServer((args.host, args.port), Handler) as server:
        print(f"fixture_http=http://{args.host}:{server.server_port}/mcp", file=sys.stderr, flush=True)
        server.serve_forever()


if __name__ == "__main__":
    main()
