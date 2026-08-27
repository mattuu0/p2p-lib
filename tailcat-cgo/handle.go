package main

import (
	"net"
	"sync"

	"github.com/tailscale/tailcat"
)

// Handle is an opaque, process-unique identifier for a Go object that has
// been exported across the cgo boundary. Handles are never reused, so a
// stale handle from a closed object simply fails lookups instead of
// silently referring to a different object.
type Handle int64

const invalidHandle Handle = 0

// registry maps Handle -> arbitrary Go value, keeping objects reachable for
// the garbage collector for as long as C/Rust code might still reference
// them by handle.
type registry struct {
	mu   sync.RWMutex
	next int64
	m    map[Handle]any
}

var reg = &registry{m: make(map[Handle]any)}

func (r *registry) put(v any) Handle {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.next++
	h := Handle(r.next)
	r.m[h] = v
	return h
}

func (r *registry) get(h Handle) (any, bool) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	v, ok := r.m[h]
	return v, ok
}

func (r *registry) delete(h Handle) {
	r.mu.Lock()
	defer r.mu.Unlock()
	delete(r.m, h)
}

// serverEntry bundles a *tailcat.Server with the channel its OnTCP/
// OnTCPForward handlers feed accepted connections into, since tailcat's
// callback-based accept model needs to be turned into the blocking
// tailcat_server_accept call our C ABI exposes.
type serverEntry struct {
	srv    *tailcat.Server
	accept chan net.Conn
	isExit bool // whether OnTCPForward (exit-node) is in use
}

func getServer(h Handle) (*serverEntry, bool) {
	v, ok := reg.get(h)
	if !ok {
		return nil, false
	}
	s, ok := v.(*serverEntry)
	return s, ok
}

func getClient(h Handle) (*tailcat.Client, bool) {
	v, ok := reg.get(h)
	if !ok {
		return nil, false
	}
	c, ok := v.(*tailcat.Client)
	return c, ok
}

func getConn(h Handle) (net.Conn, bool) {
	v, ok := reg.get(h)
	if !ok {
		return nil, false
	}
	c, ok := v.(net.Conn)
	return c, ok
}

// lastError stores the most recent error per handle so callers can recover
// a human-readable message after a call reports failure via its return
// value alone (a negative handle/error code). Errors not tied to a
// specific handle (e.g. during construction, before a handle exists) are
// stored under errGlobal.
const errGlobal Handle = -1

type errStore struct {
	mu sync.Mutex
	m  map[Handle]string
}

var lastErr = &errStore{m: make(map[Handle]string)}

func setLastError(h Handle, err error) {
	lastErr.mu.Lock()
	defer lastErr.mu.Unlock()
	if err == nil {
		delete(lastErr.m, h)
		return
	}
	lastErr.m[h] = err.Error()
}

func getLastError(h Handle) string {
	lastErr.mu.Lock()
	defer lastErr.mu.Unlock()
	return lastErr.m[h]
}
