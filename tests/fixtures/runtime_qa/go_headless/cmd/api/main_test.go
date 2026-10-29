package main

import (
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestSaveRequiresMessage(t *testing.T) {
	request := httptest.NewRequest(http.MethodPost, "/save", strings.NewReader(`{"message":"ok"}`))
	response := httptest.NewRecorder()
	newHandler(runnerProtection{Protected: true, Evidence: "fixture"}).ServeHTTP(response, request)
	if response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"saved":true`) || !strings.Contains(response.Body.String(), `"id":`) {
		t.Fatalf("unexpected save response: %d %s", response.Code, response.Body.String())
	}
}

func TestBrokeredModuleCacheVerifiesWithoutNetwork(t *testing.T) {
	env := make([]string, 0, len(os.Environ())+8)
	for _, value := range os.Environ() {
		if !strings.HasPrefix(value, "GOPROXY=") && !strings.HasPrefix(value, "GOSUMDB=") &&
			!strings.HasPrefix(value, "GOTOOLCHAIN=") && !strings.HasPrefix(value, "GOENV=") &&
			!strings.HasPrefix(value, "GOWORK=") && !strings.HasPrefix(value, "GOPRIVATE=") &&
			!strings.HasPrefix(value, "GONOPROXY=") && !strings.HasPrefix(value, "GONOSUMDB=") &&
			!strings.HasPrefix(value, "GOVCS=") {
			env = append(env, value)
		}
	}
	env = append(env,
		"GOPROXY=off",
		"GOSUMDB=off",
		"GOTOOLCHAIN=local",
		"GOENV=off",
		"GOWORK=off",
		"GOPRIVATE=",
		"GONOPROXY=",
		"GONOSUMDB=",
		"GOVCS=*:off",
	)
	command := exec.Command(filepath.Join(runtime.GOROOT(), "bin", "go"), "mod", "verify")
	command.Env = env
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("brokered Go module cache did not verify offline: %v: %s", err, output)
	}
}
