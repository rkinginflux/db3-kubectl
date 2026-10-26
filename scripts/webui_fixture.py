#!/usr/bin/env python3
"""Export the real-HTTP Rust fixtures; optionally smoke the actual binary.

No Kubernetes context is contacted. Existing target directories are refused.
"""
import argparse
import html.parser
import json
import os
from pathlib import Path
import queue
import re
import signal
import socket
import subprocess
import threading
import urllib.request

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--binary", type=Path, help="also run a temporary real-HTTP smoke")
    args = parser.parse_args()
    source = (ROOT / "tests/webui.rs").read_text()
    args.directory.mkdir(parents=True, exist_ok=False)
    fixtures = re.findall(r'write_fixture\(\s*dir.path\(\),\s*"([\w.-]+)",\s*r#"(.*?)"#', source, re.S)
    assert fixtures, "Rust fixture extraction failed"
    for name, text in fixtures:
        json.loads(text)
        (args.directory / name).write_text(text)
    for name in ("kubectl", "helm"):
        match = re.search(r'fn ' + name + r'_script\(\).*?r#"(.*?)"#', source, re.S)
        assert match, f"{name} script extraction failed"
        path = args.directory / name
        path.write_text(match.group(1))
        path.chmod(0o700)
    print(f"Exported {len(fixtures)} synthetic JSON fixtures and kubectl/helm to {args.directory}")
    if not args.binary:
        return
    env = dict(os.environ, PATH=f"{args.directory.resolve()}:/usr/bin:/bin", KUBECONFIG=str(args.directory / "unused-kubeconfig"))
    env.pop("INFLUXDB3_TOKEN", None)
    with subprocess.Popen([str(args.binary.resolve()), "webui", "--port", "0", "--context", "fixture"], env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as child:
        try:
            assert child.stdout is not None
            lines = queue.Queue()
            def read_lines():
                for line in child.stdout:
                    lines.put(line)
            threading.Thread(target=read_lines, daemon=True).start()
            while True:
                line = lines.get(timeout=15)
                if "URL" in line:
                    url = line.split("URL", 1)[1].strip()
                    break
            class Meta(html.parser.HTMLParser):
                session = None
                def handle_starttag(self, tag, attrs):
                    attrs = dict(attrs)
                    if tag == "meta" and attrs.get("name") == "db3-session":
                        self.session = attrs["content"]
            page = urllib.request.urlopen(url, timeout=10).read().decode()
            meta = Meta()
            meta.feed(page)
            assert meta.session and meta.session != "__DB3_SESSION__"
            request = urllib.request.Request(url + "/api/snapshot", headers={"X-DB3-Session": meta.session})
            with urllib.request.urlopen(request, timeout=15) as response:
                value = json.load(response)
                assert response.headers["Cache-Control"] == "no-store"
            assert value["db3"][0]["object_store"] == "s3"
            assert value["secrets"] == []
            assert value["helm_enabled"] is False and value["helm"] == []
            assert not (args.directory / "helm-calls").exists()
            assert "super-secret-license-value" not in json.dumps(value)
            child.send_signal(signal.SIGTERM)
            assert child.wait(timeout=5) == 0
            port = int(url.rsplit(":", 1)[1])
            with socket.socket() as probe:
                assert probe.connect_ex(("127.0.0.1", port)) != 0
            print("PASS real embedded shell, authenticated original snapshot API, safe defaults, derived DB details, SIGTERM and port release")
        finally:
            if child.poll() is None:
                child.kill()
                child.wait()


if __name__ == "__main__":
    main()
