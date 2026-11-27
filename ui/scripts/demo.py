"""A demo keymule home for working on the page: credentials of every kind,
MCP server secrets, and 30 days of activity. Nothing in it is real.

    cargo build
    python3 -I ui/scripts/demo.py /tmp/keymule-demo
    KEYMULE_HOME=/tmp/keymule-demo KEYMULE_TEST_VAULT=/tmp/keymule-demo/vault.json \\
      KEYMULE_NO_UPDATE_CHECK=1 target/debug/keymule ui

KEYMULE_TEST_VAULT only works in debug builds.
"""
import json, os, random, sys, time

home = sys.argv[1] if len(sys.argv) > 1 else "/tmp/keymule-demo"
os.makedirs(home, exist_ok=True)

open(os.path.join(home, "credentials.toml"), "w").write('''\
[credentials.github-ci]
service = "github"
url = "https://api.github.com"
description = "CI for the keymule repo"
auth = { type = "bearer" }
hosts = ["api.github.com", "github.com", "uploads.github.com"]
added_by = "cli"

[credentials.linear]
service = "generic"
url = "https://api.linear.app"
description = "Linear workspace"
auth = { type = "bearer" }
hosts = ["api.linear.app"]
added_by = "mcp"

[credentials.stripe-test]
service = "stripe"
url = "https://api.stripe.com"
description = "Stripe test mode"
auth = { type = "bearer" }
hosts = ["api.stripe.com"]
added_by = "mcp"

[credentials.db-pass]
service = "password"
description = "Postgres on the staging box"
auth = { type = "bearer" }
added_by = "mcp"

[credentials.vps-sydney]
service = "ssh"
description = "Production VPS"
auth = { type = "bearer" }
added_by = "mcp"
ssh = { host = "203.0.113.7", port = 22, username = "root", auth = "password" }

[credentials.github-login]
service = "login"
url = "https://github.com"
description = "GitHub, personal account"
auth = { type = "basic", username = "octo-demo" }
hosts = ["github.com"]
added_by = "mcp"

[servers.notion]
env = ["NOTION_TOKEN"]
used_by = ["~/.claude.json · notion", "~/.cursor/mcp.json · notion"]
description = "Notion workspace for the docs team"

[servers.old-db]
env = ["DATABASE_URL"]
used_by = []
''')

names = ["github-ci", "linear", "stripe-test", "db-pass", "vps-sydney", "github-login", "notion", "old-db"]
json.dump({n: f"demo-secret-for-{n}-0123456789" for n in names}, open(os.path.join(home, "vault.json"), "w"))

# 30 days of activity, more on weekdays, a few failures.
random.seed(7)
now = int(time.time())
kinds = [
    ("github-ci", "GET", "https://api.github.com/repos/acme/app/actions/runs", 200),
    ("github-ci", "POST", "https://api.github.com/repos/acme/app/issues", 201),
    ("linear", "POST", "https://api.linear.app/graphql", 200),
    ("stripe-test", "GET", "https://api.stripe.com/v1/charges", 200),
    ("stripe-test", "GET", "https://api.stripe.com/v1/customers", 401),
    ("db-pass", "EXEC", "psql", 0),
    ("vps-sydney", "SSH", "root@203.0.113.7", 0),
    ("notion", "START", "npx @notionhq/notion-mcp-server", 0),
    ("github-login", "SIGN-IN", "https://github.com", 200),
    ("github-ci", "REVEAL", "-", 0),
]
lines = []
for day in range(30, -1, -1):
    n = random.randint(2, 9) if day % 7 not in (5, 6) else random.randint(0, 2)
    for _ in range(n):
        name, method, url, status = random.choice(kinds)
        ts = now - day * 86400 - random.randint(0, 80000 if day else 3000)
        lines.append((ts, name, method, url, status))
lines.append((now - 120, "-", "PAIR", "Chrome on Mac", 0))
lines.sort()
with open(os.path.join(home, "audit.log"), "w") as f:
    for ts, name, method, url, status in lines:
        f.write(f"{ts}\t{name}\t{method}\t{url}\t{status}\n")
print(f"demo home at {home}: {len(names)} secrets, {len(lines)} activity lines")
