"""Headless HTTP/API checks. No browser, DFO, installed launcher or production DB.

Optional argument: metadata emitted by the temporary Go MEOWSIMPLE fixture.
MEOWLINK_EXE points to the companion built from this package.
"""
import copy
import http.client
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import tempfile
from urllib.parse import urlsplit


def main():
    metadata = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8")) if len(sys.argv) == 2 else None
    if metadata:
        assert metadata["fixtureOnly"] is True and metadata["host"] == "127.0.0.1"
    exe = Path(os.environ["MEOWLINK_EXE"]).resolve(strict=True)
    checks = []
    managed = os.environ.get("MEOWLINK_MANAGED_TEST") == "1"
    def launch(root):
        args = [str(exe), "--launcher-mod" if managed else "--no-open", "--root", str(root)]
        return subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    def endpoint(process):
        for _ in range(4):
            line = process.stdout.readline().decode("utf-8", errors="replace").strip()
            if line.startswith("http://127.0.0.1:"):
                return urlsplit(line)
            if line.startswith("LAUNCHER_MOD_READY_V1 "):
                ready = json.loads(line.split(" ", 1)[1])
                assert ready["schema"] == "launcher-mod-ready-v1" and ready["pid"] == process.pid
                return urlsplit(ready["url"])
        raise AssertionError("companion readiness missing")
    with tempfile.TemporaryDirectory(prefix="meowlink-companion-http-") as directory:
        root = Path(directory)
        process = launch(root)
        try:
            url = endpoint(process)
            assert url and not url.fragment and not url.query, "companion did not publish a plain local URL"
            access_token = ""

            def request(path, body=None, extra=None, token=True, host=None):
                connection = http.client.HTTPConnection("127.0.0.1", url.port, timeout=30)
                headers = {"Content-Type": "application/json"}
                if token:
                    headers["X-MeowLink-Token"] = access_token
                if host:
                    headers["Host"] = host
                headers.update(extra or {})
                payload = None if body is None else json.dumps(body).encode("utf-8")
                connection.request("GET" if body is None else "POST", path, payload, headers)
                response = connection.getresponse()
                data = response.read()
                status = response.status
                content_type = response.getheader("Content-Type")
                connection.close()
                return status, json.loads(data) if "json" in content_type else data.decode("utf-8")

            code, page = request("/", token=False)
            assert code == 200
            access_token = re.search(r'name="meowlink-access" content="([0-9a-f]{64})"', page).group(1)
            assert request("/api/status", token=False)[0] == 403
            assert request("/api/status", host="evil.example")[0] == 403
            assert request("/api/config", {}, {"Origin": "https://evil.example"})[0] == 403
            assert request("/api/config", {}, {"Content-Type": "text/plain"})[0] == 403
            assert request("/", token=False, extra={"Sec-Fetch-Site": "cross-site"})[0] == 403
            checks.extend(["page_available", "missing_token_rejected", "host_rebinding_rejected", "cross_origin_rejected", "form_content_rejected", "cross_site_bootstrap_rejected", "no_secret_in_launch_url"])
            status = request("/api/status")[1]
            assert not status["running"] and not status["identity"] and status["config"] is None
            assert status["launcherModified"] is False and status["embeddedInLauncher"] is managed
            if managed:
                assert request("/", token=False, extra={"Origin": "http://tauri.localhost", "Sec-Fetch-Site": "cross-site", "Sec-Fetch-Dest": "iframe"})[0] == 200
                assert request("/api/status", extra={"Origin": "http://tauri.localhost"})[0] == 403
                assert request("/", token=False, extra={"Origin": "https://evil.example", "Sec-Fetch-Site": "cross-site", "Sec-Fetch-Dest": "iframe"})[0] == 403
                checks.extend(["launcher_frame_bootstrap", "parent_origin_cannot_call_mod_api", "foreign_origin_bootstrap_rejected"])
            target = {"host": "192.0.2.50", "port": 28000, "serverName": "game.example.com", "authorityId": "fixture-owner", "realmId": "fixture-realm", "saveProfileId": "fixture-profile", "caCertificate": None}
            if metadata:
                target = {key: metadata[key] for key in ["host", "port", "serverName", "authorityId", "realmId", "saveProfileId"]}
                target["caCertificate"] = metadata["caDerFile"]
            config = {"schemaVersion": 1, "target": target, "playerTargetFile": str(root / "not-supplied-player-target.json"), "hostExecutable": str(root / "never-started-host.exe"), "clientDirectory": str(root / "no-game-client")}
            assert request("/api/config", config)[0] == 200
            stored = json.loads((root / "config.json").read_text(encoding="utf-8"))
            assert stored == config and request("/api/status")[1]["config"] == config
            checks.append("remote_or_local_config_roundtrip")
            invalid = copy.deepcopy(config)
            invalid["target"]["host"] = "0.0.0.0"
            assert request("/api/config", invalid)[0] == 400
            assert json.loads((root / "config.json").read_text(encoding="utf-8")) == config
            checks.append("invalid_config_preserves_previous")
            assert request("/api/check", {})[0] == 400
            assert request("/api/start", {})[1]["code"] == "login_required"
            checks.extend(["missing_descriptor_rejected", "unauthenticated_start_rejected"])
            if metadata:
                identities = set()
                # Synthetic fixture password, never a real account credential.
                fixture_password = "isolated-companion-test-password"
                for username in ["CompanionActualA", "CompanionActualB"]:
                    credentials = {"username": username, "password": fixture_password}
                    assert request("/api/register", credentials)[0] == 200
                    code, result = request("/api/login", credentials)
                    assert code == 200 and result["identity"]["username"] == username
                    identities.add(result["identity"]["nativeAccountId"])
                    current = request("/api/status")[1]
                    assert current["identity"]["username"] == username and not current["running"]
                assert len(identities) == 2
                assert request("/api/login", {"username": "CompanionActualA", "password": "wrong-fixture-password"})[0] == 400
                assert request("/api/status")[1]["identity"] is None
                wrong_tls = copy.deepcopy(config)
                wrong_tls["target"]["serverName"] = "invalid-fixture"
                assert request("/api/config", wrong_tls)[0] == 200
                assert request("/api/login", {"username": "CompanionActualA", "password": fixture_password})[0] == 400
                assert request("/api/status")[1]["identity"] is None
                foreign = copy.deepcopy(config)
                foreign["target"]["realmId"] = "foreign-fixture-realm"
                assert request("/api/config", foreign)[0] == 200
                assert request("/api/login", {"username": "CompanionActualA", "password": fixture_password})[0] == 400
                assert request("/api/status")[1]["identity"] is None
                assert fixture_password not in (root / "config.json").read_text(encoding="utf-8")
                assert request("/api/config", config)[0] == 200
                checks.extend(["two_real_gateway_accounts", "account_switch_identity_isolation", "wrong_password_clears_session", "foreign_scope_rejected", "wrong_tls_name_rejected", "password_not_persisted"])
            assert request("/api/logout", {})[0] == 200
            assert request("/api/status")[1]["identity"] is None
            assert request("/api/shutdown", {})[0] == 200
            process.wait(timeout=15)
            assert process.returncode == 0
            process.stdin.close()
            process.stdout.close()
            process.stderr.close()
            checks.append("graceful_shutdown")
            # A new process must load the saved target but must never restore an account.
            restarted = launch(root)
            process = restarted
            url = endpoint(process)
            access_token = re.search(r'name="meowlink-access" content="([0-9a-f]{64})"', request("/", token=False)[1]).group(1)
            current = request("/api/status")[1]
            assert current["config"] == config and current["identity"] is None and not current["running"]
            assert request("/api/shutdown", {})[0] == 200
            process.wait(timeout=15)
            assert process.returncode == 0
            checks.append("restart_preserves_config_without_account")
            print(json.dumps({"companionHttp": True, "managedProtocol": managed, "actualGoGateway": bool(metadata), "temporarySQLite": bool(metadata), "checks": checks, "gameStarted": False, "browserOpened": False, "launcherModified": False}))
        finally:
            if process.poll() is None:
                process.kill()
                process.wait(timeout=10)
            process.stdout.close()
            process.stderr.close()
            process.stdin.close()


if __name__ == "__main__":
    main()
