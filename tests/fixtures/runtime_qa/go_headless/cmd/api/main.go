package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"

	"github.com/google/uuid"
)

type runnerProtection struct {
	Protected bool   `json:"protected"`
	Evidence  string `json:"evidence,omitempty"`
	Error     string `json:"error,omitempty"`
}

func main() {
	protection := inspectParentRunnerProtection()
	host := os.Getenv("HOST")
	if host == "" {
		host = "0.0.0.0"
	}
	port := os.Getenv("PORT")
	if port == "" {
		port = "4173"
	}
	_ = http.ListenAndServe(host+":"+port, newHandler(protection))
}

func inspectParentRunnerProtection() runnerProtection {
	parentPID := os.Getppid()
	parentPath := filepath.Join("/proc", strconv.Itoa(parentPID))
	status, err := os.ReadFile(filepath.Join(parentPath, "status"))
	if err != nil {
		return runnerProtection{Error: fmt.Sprintf("read parent runner status: %v", err)}
	}
	runnerUID, err := effectiveUIDFromStatus(status)
	if err != nil || runnerUID != uint32(os.Geteuid()) {
		return runnerProtection{Error: "fixture app and parent runner do not share an effective UID"}
	}

	fdDirectory := filepath.Join(parentPath, "fd")
	entries, err := os.ReadDir(fdDirectory)
	if err != nil {
		if errors.Is(err, syscall.EACCES) || errors.Is(err, syscall.EPERM) {
			return runnerProtection{Protected: true, Evidence: "same_uid_proc_fd_denied"}
		}
		return runnerProtection{Error: fmt.Sprintf("read parent runner fd directory: %v", err)}
	}

	for _, entry := range entries {
		fdPath := filepath.Join(fdDirectory, entry.Name())
		target, err := os.Readlink(fdPath)
		if err != nil {
			if errors.Is(err, syscall.ENOENT) {
				continue
			}
			return runnerProtection{Error: fmt.Sprintf("read parent runner fd target: %v", err)}
		}
		if !strings.Contains(target, "memfd:koolade-runtime-qa") {
			continue
		}

		fd, err := syscall.Open(fdPath, syscall.O_WRONLY|syscall.O_TRUNC, 0)
		if err == nil {
			_ = syscall.Close(fd)
			return runnerProtection{Error: "same-UID O_WRONLY|O_TRUNC opened the readiness memfd"}
		}
		if errors.Is(err, syscall.EACCES) || errors.Is(err, syscall.EPERM) {
			return runnerProtection{Protected: true, Evidence: "same_uid_memfd_truncate_denied"}
		}
		return runnerProtection{Error: fmt.Sprintf("truncate readiness memfd: %v", err)}
	}

	return runnerProtection{Error: "parent runner fd directory was readable but its readiness memfd was absent"}
}

func effectiveUIDFromStatus(status []byte) (uint32, error) {
	for _, line := range strings.Split(string(status), "\n") {
		fields := strings.Fields(line)
		if len(fields) == 5 && fields[0] == "Uid:" {
			uid, err := strconv.ParseUint(fields[2], 10, 32)
			return uint32(uid), err
		}
	}
	return 0, errors.New("parent runner effective UID is unavailable")
}

func newHandler(protection runnerProtection) http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("healthy"))
	})
	mux.HandleFunc("GET /runner-protection", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		if !protection.Protected {
			w.WriteHeader(http.StatusInternalServerError)
		}
		_ = json.NewEncoder(w).Encode(protection)
	})
	mux.HandleFunc("POST /save", func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Message string `json:"message"`
		}
		if json.NewDecoder(r.Body).Decode(&input) != nil || input.Message == "" {
			http.Error(w, "message required", http.StatusBadRequest)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]any{"saved": true, "id": uuid.NewString()})
	})
	return mux
}
