package main

import (
	"context"
	"crypto/tls"
	"encoding/json"
	"encoding/pem"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

// Runs the actual Electron provider against the actual Go gateway and isolated
// SQLite DAO. No production address, real credential or game process is used.
func TestBetaElectronAgainstActualSimpleHost(t *testing.T) {
	runActualSimpleHost(t)
}
func TestModRustAgainstActualSimpleHost(t *testing.T) {
	runActualSimpleHost(t)
}
func runActualSimpleHost(t *testing.T) {
	node, runner := os.Getenv("MEOW_BETA_NODE"), os.Getenv("MEOW_BETA_RUNNER")
	if node == "" || runner == "" {
		t.Skip("explicit local Node/provider runner required")
	}
	settings, fixture := simpleTestSettings(t)
	settings.allowLoopbackPlaintext = false
	serverTLS, _ := simpleTestTLS(t)
	gateway := &gameGateway{runtime: &gatewayRuntime{gameStore: fixture.Storage()}, simpleIdentity: settings}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	var workers sync.WaitGroup
	var sockets sync.Map
	workers.Add(1)
	go func() {
		defer workers.Done()
		for {
			raw, e := listener.Accept()
			if e != nil {
				return
			}
			sockets.Store(raw, true)
			workers.Add(1)
			go func() {
				defer workers.Done()
				defer sockets.Delete(raw)
				defer raw.Close()
				secure := tls.Server(raw, serverTLS)
				defer secure.Close()
				if secure.HandshakeContext(ctx) != nil {
					return
				}
				client, e := gateway.newConnectionWithContext(ctx, secure, 1)
				if e != nil {
					return
				}
				defer client.clearConnectionIdentity()
				// Authentication stays on this TLS stream; no second anonymous native socket.
				_ = secure.SetReadDeadline(time.Now().Add(10 * time.Second))
				if os.Getenv("MEOW_BETA_STREAM") == "1" {
					// Explicit synthetic transport sentinel, not a native game packet.
					if _, e = secure.Write([]byte("TRANSPORT-ONLY")); e != nil {
						return
					}
					var echo [14]byte
					if _, e = io.ReadFull(secure, echo[:]); e != nil || string(echo[:]) != "TRANSPORT-ONLY" {
						return
					}
					_, _ = secure.Write([]byte("ACK"))
				}
				var b [1]byte
				_, _ = secure.Read(b[:])
			}()
		}
	}()
	defer func() {
		cancel()
		_ = listener.Close()
		sockets.Range(func(k, v any) bool { _ = k.(net.Conn).Close(); return true })
		workers.Wait()
	}()
	dir := t.TempDir()
	ca := filepath.Join(dir, "fixture-ca.pem")
	caDER := filepath.Join(dir, "fixture-ca.der")
	if err = os.WriteFile(caDER, serverTLS.Certificates[0].Certificate[0], 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(ca, pem.EncodeToMemory(&pem.Block{Type: "CERTIFICATE", Bytes: serverTLS.Certificates[0].Certificate[0]}), 0600); err != nil {
		t.Fatal(err)
	}
	metadata := map[string]any{"fixtureOnly": true, "host": "127.0.0.1", "port": listener.Addr().(*net.TCPAddr).Port, "serverName": "simple-fixture", "authorityId": settings.authority, "realmId": settings.realm, "saveProfileId": settings.profile, "caFile": ca, "caDerFile": caDER}
	data, _ := json.Marshal(metadata)
	target := filepath.Join(dir, "fixture-target.json")
	if err = os.WriteFile(target, data, 0600); err != nil {
		t.Fatal(err)
	}
	command := exec.CommandContext(ctx, node, runner, target)
	output, err := command.CombinedOutput()
	if err != nil {
		t.Fatalf("actual Beta provider runner failed: %v\n%s", err, output)
	}
	t.Logf("actual Beta provider runner: %s", output)
}
