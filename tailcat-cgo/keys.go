package main

import (
	"encoding/json"
	"fmt"

	"github.com/tailscale/tailcat"
	"tailscale.com/types/key"
)

// privateKeyJSON is the JSON wire shape for a tailcat.PrivateKey, mirroring
// the plain encoding/json format the tailcat CLI's genkey command uses for
// its *.private.json files (see cmd/tailcat/tailcat.go). It intentionally
// does not touch the CBOR ConnBlob wire format, which is unrelated.
type privateKeyJSON struct {
	Private string       `json:"private"` // key.NodePrivate.MarshalText() form
	Public  connInfoJSON `json:"public"`
}

type connInfoJSON struct {
	ServerPublic string `json:"serverPublic"` // key.NodePublic.MarshalText() form
	RegionID     int    `json:"regionID,omitempty"`
}

func marshalPrivateKey(pk *tailcat.PrivateKey) (string, error) {
	privText, err := pk.Private.MarshalText()
	if err != nil {
		return "", fmt.Errorf("marshal private key: %w", err)
	}
	pubText, err := pk.Public.ServerPublic.MarshalText()
	if err != nil {
		return "", fmt.Errorf("marshal public key: %w", err)
	}
	w := privateKeyJSON{
		Private: string(privText),
		Public: connInfoJSON{
			ServerPublic: string(pubText),
			RegionID:     pk.Public.RegionID,
		},
	}
	b, err := json.Marshal(w)
	if err != nil {
		return "", err
	}
	return string(b), nil
}

func unmarshalPrivateKey(s string) (*tailcat.PrivateKey, error) {
	var w privateKeyJSON
	if err := json.Unmarshal([]byte(s), &w); err != nil {
		return nil, fmt.Errorf("parse private key JSON: %w", err)
	}
	var priv key.NodePrivate
	if err := priv.UnmarshalText([]byte(w.Private)); err != nil {
		return nil, fmt.Errorf("parse private key: %w", err)
	}
	var pub key.NodePublic
	if err := pub.UnmarshalText([]byte(w.Public.ServerPublic)); err != nil {
		return nil, fmt.Errorf("parse public key: %w", err)
	}
	return &tailcat.PrivateKey{
		Private: priv,
		Public: tailcat.ConnInfo{
			ServerPublic: tailcat.NodePublic{NodePublic: pub},
			RegionID:     w.Public.RegionID,
		},
	}, nil
}

// serverStateJSON is the shape returned by tailcat_server_state: everything
// a caller needs to persist to resume this server's identity and access
// list later, bundled into a single JSON string per this project's
// no-file-IO-in-Go policy (see plan: persistence is JSON-in/JSON-out only).
type serverStateJSON struct {
	PrivateKey     privateKeyJSON `json:"privateKey"`
	AllowedClients []string       `json:"allowedClients,omitempty"` // NodePublic.MarshalText() form
}

func jsonMarshal(v any) (string, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return "", err
	}
	return string(b), nil
}
