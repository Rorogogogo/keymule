"""Website logins against the REAL browser MCP servers, acting as the agent:
navigate with the browser tool, sign in with keymule's sign_in, snapshot with
the browser tool. Needs Chrome, npx and the network (it fetches the servers
and signs in to the-internet.herokuapp.com, a public demo with published test
credentials), so it isn't part of CI. Run it before a release:

    cargo build && python3 tests/browser_e2e.py target/debug/keymule playwright
    python3 tests/browser_e2e.py target/debug/keymule devtools
"""
import http.server, json, os, subprocess, sys, tempfile, threading, time, urllib.parse

BIN, WHICH = os.path.abspath(sys.argv[1]), sys.argv[2]  # WHICH: playwright | devtools
PASSWORD = "s3cret-Pa55-word!"
got = {}

STEP1 = b"""<!doctype html><title>Sign in</title><form onsubmit="return next(event)">
<label>Email <input type=email name=email autocomplete=username></label><button type=submit>Next</button></form>
<script>function next(e){e.preventDefault();const u=document.querySelector('input').value;
setTimeout(()=>{document.body.innerHTML='<form method=post action=/login><input type=hidden name=email value="'+u+'"><label>Password <input type=password name=password autocomplete=current-password></label><button type=submit>Sign in</button></form>'},300);return false}</script>"""


class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.send_response(200); self.send_header("content-type", "text/html"); self.end_headers()
        self.wfile.write(STEP1)
    def do_POST(self):
        n = int(self.headers["content-length"])
        got.update(urllib.parse.parse_qsl(self.rfile.read(n).decode()))
        self.send_response(200); self.send_header("content-type", "text/html"); self.end_headers()
        # A careless site that echoes the password back: the snapshot must not show it.
        self.wfile.write(f"<title>Welcome</title><h1>Welcome {got.get('email')}</h1><p>debug: password={got.get('password')}</p>".encode())


srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

home = tempfile.mkdtemp(prefix="keymule-browser-e2e-")
vault = os.path.join(home, "vault.json")
open(os.path.join(home, "credentials.toml"), "w").write("")
json.dump({}, open(vault, "w"))
env = dict(os.environ, KEYMULE_HOME=home, KEYMULE_TEST_VAULT=vault, KEYMULE_BROWSER_HEADLESS="1")

fails = 0
def check(label, ok, detail=""):
    global fails
    fails += not ok
    print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"\n     {str(detail)[:600]}"), flush=True)


class Client:
    def __init__(self, argv, extra=None):
        # cwd: Playwright MCP writes its logs into .playwright-mcp/ there
        self.p = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
                                  env=dict(env, **(extra or {})), stderr=subprocess.DEVNULL, cwd=home)
        self.n = 0
        self.raw = []
        self.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    def send(self, m):
        self.p.stdin.write(json.dumps(m) + "\n"); self.p.stdin.flush()
    def rpc(self, method, params):
        self.n += 1
        self.send({"jsonrpc": "2.0", "id": self.n, "method": method, "params": params})
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("server closed")
            self.raw.append(line)
            m = json.loads(line)
            if m.get("id") == self.n:
                return m
    def call(self, tool, args):
        r = self.rpc("tools/call", {"name": tool, "arguments": args})
        if "error" in r:
            return True, json.dumps(r["error"])
        res = r["result"]
        return res.get("isError", False), "\n".join(c.get("text", "") for c in res["content"] if c.get("type") == "text")
    def close(self):
        self.p.stdin.close(); self.p.wait(timeout=30)


def keymule(answer, tool, args):
    c = Client([BIN, "mcp"], {"KEYMULE_TEST_DIALOG": answer})
    out = c.call(tool, args)
    c.close()
    return out


server = {"playwright": ["npx", "-y", "@playwright/mcp@latest"],
          "devtools": ["npx", "-y", "chrome-devtools-mcp@latest"]}[WHICH]
nav = {"playwright": ("browser_navigate", lambda u: {"url": u}),
       "devtools": ("navigate_page", lambda u: {"url": u, "type": "url"})}[WHICH]
snap = {"playwright": "browser_snapshot", "devtools": "take_snapshot"}[WHICH]

browser = Client([BIN, "browser", "--"] + server)
tools = [t["name"] for t in browser.rpc("tools/list", {})["result"]["tools"]]
check(f"{WHICH}: real server runs through keymule browser ({len(tools)} tools)", nav[0] in tools and snap in tools, tools)
snap_args = {}
if WHICH == "devtools":
    import re
    _, pages = browser.call("list_pages", {})
    m = re.search(r"(\d+):[^\n]*\[selected\]", pages) or re.search(r"(\d+):", pages)
    pid = int(m.group(1))
    print("devtools page id", pid, flush=True)
    nav = (nav[0], lambda u: {"url": u, "type": "url", "pageId": pid})
    snap_args = {"pageId": pid}

# ---- local two-step login ----
err, out = keymule(PASSWORD, "add_website_login", {"name": "local", "url": f"http://127.0.0.1:{port}/login", "username": "me@example.com"})
check("add_website_login", not err, out)
err, out = browser.call(nav[0], nav[1](f"http://127.0.0.1:{port}/login"))
check("browser tool opens the sign-in page", not err, out)
err, out = keymule("allow", "sign_in", {"credential": "local"})
check("sign_in fills the two-step form and submits", not err and "Signed in" in out, out)
check("site got the right username and password", got.get("email") == "me@example.com" and got.get("password") == PASSWORD, got)
time.sleep(0.5)
err, out = browser.call(snap, snap_args)
check("browser tool sees the signed-in page", "Welcome" in out, out)
check("password the page echoes is hidden in the snapshot", PASSWORD not in out and "hidden by keymule" in out, out)
check("password never appears anywhere the agent received", not any(PASSWORD in l for l in browser.raw))

# ---- a real https site (public demo credentials) ----
err, out = keymule("SuperSecretPassword!", "add_website_login", {"name": "demo", "url": "https://the-internet.herokuapp.com/login", "username": "tomsmith"})
check("add_website_login (https demo)", not err, out)
err, out = browser.call(nav[0], nav[1]("https://the-internet.herokuapp.com/login"))
check("browser tool opens the demo sign-in page", not err, out)
err, out = keymule("allow", "sign_in", {"credential": "demo"})
check("sign_in to the real https site", not err and "Signed in" in out and "/secure" in out, out)
err, out = browser.call(snap, snap_args)
check("snapshot shows the secure area", "Secure Area" in out or "logged into a secure area" in out, out)

# ---- wrong password: reported, not 'Signed in' ----
err, out = keymule("not-the-password", "add_website_login", {"name": "demo-bad", "url": "https://the-internet.herokuapp.com/login", "username": "tomsmith"})
browser.call(nav[0], nav[1]("https://the-internet.herokuapp.com/login"))
err, out = keymule("allow", "sign_in", {"credential": "demo-bad"})
check("wrong password: says the form is still there", "still there" in out or "may be wrong" in out, out)

browser.close()
print("FAILS:", fails)
sys.exit(1 if fails else 0)
