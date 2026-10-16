"""Drives `asides mcp` the way a coding agent does, for scripts/e2e.sh.

Usage: e2e-mcp.py <asides CLI> <repo> <fixtures dir> <config.toml> <listeners dir> <another repo>
"""

import json
import os
import queue
import re
import subprocess
import sys
import threading
import time

cli, repo, fixtures, config_path, listeners, other = sys.argv[1:7]


def passed(what):
    print(f"  \033[32m✓\033[0m {what}", flush=True)


def failed(what, detail=""):
    print(f"  \033[31m✗\033[0m {what}", flush=True)
    if detail:
        print(f"--- {detail}", flush=True)
    sys.exit(1)


def check(what, ok, detail=""):
    passed(what) if ok else failed(what, detail)


class Client:
    def __init__(self):
        self.proc = subprocess.Popen(
            [cli, "-C", repo, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1
        )
        self.messages = queue.Queue()
        self.last_id = 0
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self.proc.stdout:
            self.messages.put(json.loads(line))

    def send(self, message):
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", **message}) + "\n")
        self.proc.stdin.flush()

    def request(self, method, params=None):
        self.last_id += 1
        self.send({"id": self.last_id, "method": method, **({"params": params} if params is not None else {})})
        return self.last_id

    def response(self, request, timeout):
        deadline = time.time() + timeout
        while time.time() < deadline:
            try:
                message = self.messages.get(timeout=max(0.05, deadline - time.time()))
            except queue.Empty:
                break
            if message.get("id") == request:
                return message
        return None

    def call(self, method, params=None, timeout=10):
        message = self.response(self.request(method, params), timeout)
        if message is None:
            failed(f"{method} answered")
        return message.get("result", message.get("error"))


def text(result):
    return "\n".join(c.get("text", "") for c in result["content"] if c["type"] == "text")


def tool(name, **arguments):
    return {"name": name, "arguments": arguments}


def simulate(wav):
    subprocess.run([cli, "-C", repo, "simulate", os.path.join(fixtures, wav)], check=True, capture_output=True)


def entries():
    note = subprocess.run([cli, "-C", repo, "path"], check=True, capture_output=True, text=True).stdout.strip()
    with open(note) as f:
        return sum(1 for line in f if line.startswith("## ") and line[3:4].isdigit())


def wait_for_entries(count):
    for _ in range(240):
        if entries() >= count:
            return
        time.sleep(0.5)
    failed(f"entry {count} written")


client = Client()
init = client.call("initialize", {
    "protocolVersion": "2025-06-18",
    "capabilities": {},
    "clientInfo": {"name": "e2e-agent", "version": "1"},
})
client.send({"method": "notifications/initialized"})
check("initialize: server info and protocol version",
      init["serverInfo"]["name"] == "asides" and init["protocolVersion"] == "2025-06-18", init)
names = sorted(t["name"] for t in client.call("tools/list")["tools"])
check("tools: wait_for_asides, get_asides, bind_asides", names == ["bind_asides", "get_asides", "wait_for_asides"], names)
names = sorted(p["name"] for p in client.call("prompts/list")["prompts"])
check("prompts: asides, listen", names == ["asides", "listen"], names)
note = text(client.call("tools/call", tool("get_asides")))
check("get_asides reads the latest note with its metadata", 'Note "' in note and "branch:" in note and "## 1 ·" in note, note)
prompt = client.call("prompts/get", {"name": "asides"})["messages"][0]["content"]["text"]
check("the asides prompt carries the note", prompt.startswith("Work through my asides") and "## 1 ·" in prompt, prompt)

# Bound elsewhere: the wait says so at once, and bind_asides points the app here.
subprocess.run([cli, "-C", other], check=True, capture_output=True)
started = time.time()
elsewhere = text(client.call("tools/call", tool("wait_for_asides", timeout_seconds=60), timeout=10))
check("waiting while Asides is bound elsewhere returns at once",
      time.time() - started < 5 and "bind_asides" in elsewhere and os.path.basename(other) in elsewhere, elsewhere)
bound = text(client.call("tools/call", tool("bind_asides"), timeout=30))
check("bind_asides binds the app to the agent's repo", bound.startswith("Asides is now bound to"), bound)
status = subprocess.run([cli, "-C", repo, "status"], check=True, capture_output=True, text=True).stdout
check("as the app confirms", re.search(rf"bound to .*/{re.escape(os.path.basename(repo))}$", status, re.M), status)
check("binding again is a no-op", "already bound" in text(client.call("tools/call", tool("bind_asides"), timeout=30)))

# Each aside, as soon as it's transcribed.
before = entries()
waiting = client.request("tools/call", tool("wait_for_asides", timeout_seconds=180))
time.sleep(1.5)
record = os.path.join(listeners, f"{client.proc.pid}.json")
with open(record) as f:
    listener = json.load(f)
check("listening shows in the app's listener records", listener["waiting"] and listener["client"] == "e2e-agent", listener)
simulate("en_bug.wav")
answer = client.response(waiting, 180)
result = answer and answer["result"]
check("a new recording reaches the waiting agent", result and "padding on the summary card" in text(result),
      result and text(result))
check("as one new aside", text(result).startswith("A new aside from the user."), text(result))
check("nothing is delivered twice",
      "No new asides yet" in text(client.call("tools/call", tool("wait_for_asides", timeout_seconds=2), timeout=10)))

# Batches: held until Send to Agent.
with open(config_path) as f:
    config = re.sub(r"(?m)^agent_delivery = .*\n", "", f.read())
with open(config_path, "w") as f:
    f.write('agent_delivery = "batch"\n' + config)
simulate("ro_bug.wav")
wait_for_entries(before + 2)
held = text(client.call("tools/call", tool("wait_for_asides", timeout_seconds=3), timeout=10))
check("in batch mode, a transcribed aside waits", "No new asides yet" in held and "Send to Agent" in held, held)
waiting = client.request("tools/call", tool("wait_for_asides", timeout_seconds=60))
sent = subprocess.run([cli, "-C", repo, "send"], check=True, capture_output=True, text=True).stdout
check("asides send", sent.startswith("Sent"), sent)
answer = client.response(waiting, 30)
result = answer and answer["result"]
check("Send to Agent delivers the batch", result and "Butonul de salvare" in text(result), result and text(result))

# Cancelling a wait, then hanging up.
waiting = client.request("tools/call", tool("wait_for_asides", timeout_seconds=60))
client.send({"method": "notifications/cancelled", "params": {"requestId": waiting, "reason": "user"}})
check("a cancelled wait gets no answer", client.response(waiting, 2) is None)
check("and the server keeps serving", client.call("ping") == {})
client.proc.stdin.close()
client.proc.wait(timeout=10)
check("hanging up removes the listener record", not os.path.exists(record))
