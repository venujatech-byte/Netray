package tls_test

import (
    "crypto/tls"
    "net"
    "time"
    "net/http"
    "net/http/httptest"
    "testing"

    "github.com/xtls/xray-core/infra/conf"
    xraytls "github.com/xtls/xray-core/transport/internet/tls"
)

// Exercise a real TLS handshake with an untrusted, mismatched server certificate.
func TestNetrayAllowInsecure(t *testing.T) {
    if _, err := (&conf.TLSConfig{VerifyPeerCertByName: "netray-insecure-tls.invalid"}).Build(); err == nil {
        t.Fatal("imported configuration must not forge the internal bypass marker")
    }
    server := httptest.NewTLSServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {}))
    defer server.Close()
    for _, allow := range []bool{false, true} {
        cfg, err := (&conf.TLSConfig{AllowInsecure: allow, ServerName: "mismatch.invalid", Fingerprint: "chrome"}).Build()
        if err != nil { t.Fatal(err) }
        conn, err := tls.Dial("tcp", server.Listener.Addr().String(), cfg.(*xraytls.Config).GetTLSConfig())
        if conn != nil { conn.Close() }
        if (err == nil) != allow { t.Fatalf("allowInsecure=%v: handshake error=%v", allow, err) }
        raw, err := net.DialTimeout("tcp", server.Listener.Addr().String(), 3*time.Second)
        if err != nil { t.Fatal(err) }
        raw.SetDeadline(time.Now().Add(3*time.Second))
        chrome := xraytls.UClient(raw, cfg.(*xraytls.Config).GetTLSConfig(), xraytls.GetFingerprint("chrome"))
        err = chrome.(interface{ Handshake() error }).Handshake()
        chrome.Close()
        if (err == nil) != allow { t.Fatalf("Chrome allowInsecure=%v: handshake error=%v", allow, err) }
    }
}
