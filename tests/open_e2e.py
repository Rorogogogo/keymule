"""keymule browser --open (for Claude in Chrome) against a real Chrome: the
window outlives the command, the profile is set up for the extension, and
sign_in fills into a tab the command opened. Needs Chrome; not part of CI.

    cargo build && python3 tests/open_e2e.py target/debug/keymule
"""
import glob, http.server, json, os, signal, subprocess, sys, tempfile, threading, time, urllib.parse, urllib.request

BIN = os.path.abspath(sys.argv[1])
PASSWORD = "s3cret-Pa55-word!"
got = {}

PAGE = b"""<!doctype html><title>Sign in</title><form method=post action=/login>
<input type=email name=email autocomplete=username><input type=password name=password autocomplete=current-password>
<button type=submit>Sign in</button></form>"""


class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.send_response(200); self.send_header("content-type", "text/html"); self.end_headers()
        self.wfile.write(PAGE)
    def do_POST(self):
        n = int(self.headers["content-length"])
        got.update(urllib.parse.parse_qsl(self.rfile.read(n).decode()))
        self.send_response(200); self.send_header("content-type", "text/html"); self.end_headers()
        self.wfile.write(b"<title>Welcome</title><h1>Welcome</h1>")


srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
port = srv.server_address[1]
threading.Thread(target=srv.serve_forever, daemon=True).start()

home = tempfile.mkdtemp(prefix="keymule-open-e2e-")
vault = os.path.join(home, "vault.json")
open(os.path.join(home, "credentials.toml"), "w").write("")
json.dump({}, open(vault, "w"))
env = dict(os.environ, KEYMULE_HOME=home, KEYMULE_TEST_VAULT=vault, KEYMULE_BROWSER_HEADLESS="1",
           KEYMULE_NO_UPDATE_CHECK="1")
profile = os.path.join(home, "browser", "profile")

fails = 0
def check(label, ok, detail=""):
    global fails
    fails += not ok
    print(("PASS " if ok else "FAIL ") + label + ("" if ok else f"\n     {str(detail)[:600]}"), flush=True)


def keymule(*args):
    p = subprocess.run([BIN, *args], env=env, capture_output=True, text=True, timeout=60, cwd=home)
    return p.returncode, p.stdout + p.stderr


def mcp(answer, tool, args):
    p = subprocess.Popen([BIN, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
                         env=dict(env, KEYMULE_TEST_DIALOG=answer), stderr=subprocess.DEVNULL, cwd=home)
    msgs = [{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": args}}]
    for m in msgs:
        p.stdin.write(json.dumps(m) + "\n"); p.stdin.flush()
    while True:
        m = json.loads(p.stdout.readline())
        if m.get("id") == 2:
            break
    p.stdin.close(); p.wait(timeout=30)
    res = m["result"]
    return res.get("isError", False), "\n".join(c.get("text", "") for c in res["content"])


def pages(state):
    with urllib.request.urlopen(f"http://127.0.0.1:{state['port']}/json/list", timeout=5) as r:
        return [t["url"] for t in json.load(r) if t["type"] == "page"]


url = f"http://127.0.0.1:{port}/login"
code, out = keymule("browser", "--open", url)
check("--open returns once the window is up", code == 0 and "keymule's Chrome is open" in out, out)
check("first time: explains adding Claude in Chrome", "Claude in Chrome" in out and "/chrome" in out, out)
state = json.load(open(os.path.join(home, "browser", "state.json")))
try:
    os.kill(state["pid"], 0); alive = True
except OSError:
    alive = False
check("Chrome is still running after the command exited", alive, state)
check("the tab is on the address given", url in pages(state), pages(state))

sources = glob.glob(os.path.expanduser("~/Library/Application Support/Google/Chrome/NativeMessagingHosts/com.anthropic.*.json"))
copied = sorted(os.listdir(os.path.join(profile, "NativeMessagingHosts"))) if os.path.isdir(os.path.join(profile, "NativeMessagingHosts")) else []
check("Claude's native messaging hosts are in the profile (and only those)",
      copied == sorted(os.path.basename(s) for s in sources) and all(c.startswith("com.anthropic.") for c in copied),
      (copied, sources))
if not sources:
    print("     (no Claude native hosts on this machine to copy)")

err, out = mcp(PASSWORD, "add_website_login", {"name": "local", "url": url, "username": "me@example.com"})
check("add_website_login", not err, out)
err, out = mcp("allow", "sign_in", {"credential": "local"})
check("sign_in fills the tab --open opened", not err and "Signed in" in out, out)
check("site got the username and password", got.get("email") == "me@example.com" and got.get("password") == PASSWORD, got)

code, out = keymule("browser", "--open", "https://example.com/")
state2 = json.load(open(os.path.join(home, "browser", "state.json")))
check("--open again reuses the same Chrome", code == 0 and state2["pid"] == state["pid"], (out, state2))
time.sleep(1)
check("... and opens the address in a new tab", any(u.startswith("https://example.com") for u in pages(state)), pages(state))
code, out = keymule("browser", "--open", "file:///etc/passwd")
check("--open only takes http(s) addresses", code != 0, out)

os.kill(state["pid"], signal.SIGTERM)
time.sleep(3)
prefs = json.load(open(os.path.join(profile, "Default", "Preferences")))
check("Chrome kept 'never offer to save passwords' after running",
      prefs.get("credentials_enable_service") is False and prefs.get("profile", {}).get("password_manager_enabled") is False,
      {k: prefs.get(k) for k in ("credentials_enable_service",)})
print("FAILS:", fails)
sys.exit(1 if fails else 0)
