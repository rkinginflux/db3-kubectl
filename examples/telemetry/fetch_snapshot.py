#!/usr/bin/env python3
"""Cross-platform reference client. Fetch evidence only; never throttle or write to InfluxDB."""
import argparse
import ipaddress
import json
import os
from pathlib import Path
import urllib.parse
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def fetch(access_path):
    if os.name != "nt" and access_path.stat().st_mode & 0o077:
        raise ValueError("Protect the credential file with mode 0600")
    config = json.loads(access_path.read_text(encoding="utf-8"))
    url = urllib.parse.urlsplit(config["url"])
    address = ipaddress.ip_address(url.hostname)
    if (url.scheme != "http" or url.port != 9091 or url.username or url.password
            or url.query or url.fragment or url.path not in {"", "/"}
            or not (address.is_loopback or address in ipaddress.ip_network("100.64.0.0/10"))):
        raise ValueError("Expected the private Tailscale telemetry URL")
    request = urllib.request.Request(config["url"].rstrip("/") + "/v1/snapshot",
        headers={"Authorization": "Bearer " + config["token"], "Accept": "application/json"})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    with opener.open(request, timeout=15) as response:
        body = response.read(1024 * 1024 + 1)
    if len(body) > 1024 * 1024:
        raise ValueError("Snapshot response exceeded 1 MiB")
    snapshot = json.loads(body)
    if snapshot.get("schema_version") != "1.0":
        raise ValueError("Unsupported telemetry contract")
    return snapshot


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--access", type=Path, required=True, help="Private credential JSON path, not a token")
    parser.add_argument("--output", type=Path, required=True, help="New local JSON evidence file")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Output already exists; preserve previous evidence")
    snapshot = fetch(args.access)
    with open(args.output, "x", encoding="utf-8", opener=lambda p, f: os.open(p, f, 0o600)) as file:
        json.dump(snapshot, file, indent=2, allow_nan=False)
        file.write("\n")
    print("Saved sanitized evidence:", args.output.resolve())
    print("Collection status:", snapshot["status"], "(check each section and metric before use)")


if __name__ == "__main__":
    main()
