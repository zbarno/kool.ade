package main

import (
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"syscall"
	"time"
	"unsafe"
)

const (
	attempts        = 40
	memfdCreateFlag = 2
	mfdExec         = 0x10
	fAddSeals       = 1033
	fSealSeal       = 0x01
	fSealShrink     = 0x02
	fSealGrow       = 0x04
	fSealWrite      = 0x08
	fSealAll        = fSealSeal | fSealShrink | fSealGrow | fSealWrite
	fSetFd          = 2
	fdCloseExec     = 1
	prSetDumpable   = 4
	prGetDumpable   = 3
)

func main() {
	if len(os.Args) > 1 && os.Args[1] == "--sealed" {
		if err := runSealed(); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}
	if err := sealAndReexec(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func sealAndReexec() error {
	if len(os.Args) < 7 {
		return fmt.Errorf("invalid readiness runner arguments")
	}
	name, err := syscall.BytePtrFromString("koolade-runtime-qa")
	if err != nil {
		return err
	}
	fd, errno := createMemfd(name, memfdCreateFlag|mfdExec)
	if errno == syscall.EINVAL {
		fd, errno = createMemfd(name, memfdCreateFlag)
	}
	if errno != 0 {
		return fmt.Errorf("create sealed readiness runner: %w", errno)
	}
	sealed := os.NewFile(fd, "koolade-runtime-qa")
	source, err := os.Open(os.Args[0])
	if err != nil {
		return err
	}
	if _, err := io.Copy(sealed, source); err != nil {
		_ = source.Close()
		return err
	}
	if err := source.Close(); err != nil {
		return err
	}
	if err := sealed.Chmod(0500); err != nil {
		return err
	}
	if _, _, errno = syscall.Syscall(syscall.SYS_FCNTL, fd, fAddSeals, fSealAll); errno != 0 {
		return fmt.Errorf("seal readiness runner: %w", errno)
	}
	if err := sealed.Chmod(0700); err != nil {
		return err
	}
	reopened, err := syscall.Open("/proc/self/fd/"+strconv.Itoa(int(fd)), syscall.O_WRONLY|syscall.O_TRUNC, 0)
	if err == nil {
		_ = syscall.Close(reopened)
		return fmt.Errorf("F_SEAL_WRITE did not block a writable reopen")
	}
	if err != syscall.EPERM {
		return fmt.Errorf("F_SEAL_WRITE did not block a writable reopen: %v", err)
	}
	if err := sealed.Chmod(0500); err != nil {
		return err
	}
	readOnlyFD, err := syscall.Open("/proc/self/fd/"+strconv.Itoa(int(fd)), syscall.O_RDONLY, 0)
	if err != nil {
		return fmt.Errorf("reopen sealed readiness runner read-only: %w", err)
	}
	if err := sealed.Close(); err != nil {
		_ = syscall.Close(readOnlyFD)
		return err
	}
	fd = uintptr(readOnlyFD)
	args := append([]string{"koolade-runtime-qa", "--sealed", strconv.Itoa(int(fd))}, os.Args[1:]...)
	if err := removeBuildFiles(os.Args[1]); err != nil {
		return err
	}
	return syscall.Exec("/proc/self/fd/"+strconv.Itoa(int(fd)), args, os.Environ())
}

func createMemfd(name *byte, flags uintptr) (uintptr, syscall.Errno) {
	var number uintptr
	switch runtime.GOARCH {
	case "amd64":
		number = 319
	case "arm64":
		number = 279
	default:
		return 0, syscall.ENOSYS
	}
	fd, _, errno := syscall.Syscall(number, uintptr(unsafe.Pointer(name)), flags, 0)
	runtime.KeepAlive(name)
	return fd, errno
}

func removeBuildFiles(directory string) error {
	for _, name := range []string{"runner", "runner.go"} {
		if err := os.Remove(filepath.Join(directory, name)); err != nil {
			return err
		}
	}
	return os.Remove(directory)
}

func runSealed() error {
	if len(os.Args) < 9 {
		return fmt.Errorf("invalid sealed readiness runner arguments")
	}
	fd, err := strconv.Atoi(os.Args[2])
	if err != nil {
		return err
	}
	if _, _, errno := syscall.Syscall(syscall.SYS_FCNTL, uintptr(fd), fSetFd, fdCloseExec); errno != 0 {
		return fmt.Errorf("protect readiness runner descriptor: %w", errno)
	}
	if _, _, errno := syscall.Syscall6(syscall.SYS_PRCTL, prSetDumpable, 0, 0, 0, 0, 0); errno != 0 {
		return fmt.Errorf("protect readiness runner process: %w", errno)
	}
	dumpable, _, errno := syscall.Syscall6(syscall.SYS_PRCTL, prGetDumpable, 0, 0, 0, 0, 0)
	if errno != 0 || dumpable != 0 {
		return fmt.Errorf("PR_GET_DUMPABLE did not confirm a protected readiness runner: %v", errno)
	}
	workdir, executable := os.Args[4], os.Args[5]
	port, err := strconv.Atoi(os.Args[6])
	if err != nil || port < 1 || port > 65535 {
		return fmt.Errorf("invalid readiness port")
	}
	path, statusText := os.Args[7], os.Args[8]
	status, err := strconv.Atoi(statusText)
	if err != nil || status < 0 || status > 599 {
		return fmt.Errorf("invalid readiness status")
	}
	app, err := startApplication(workdir, executable, os.Args[9:])
	if err != nil {
		return err
	}
	ready, waitErr := awaitReadiness(app, port, path, status)
	if ready {
		if err := app.Release(); err != nil {
			return err
		}
		fmt.Println("readiness probe passed")
		return nil
	}
	_ = app.Kill()
	_, _ = app.Wait()
	if waitErr != nil {
		return fmt.Errorf("start process exited before readiness: %w", waitErr)
	}
	return fmt.Errorf("readiness probe timed out")
}

func startApplication(workdir, executable string, arguments []string) (*os.Process, error) {
	command := exec.Command(executable, arguments...)
	command.Dir = workdir
	null, err := os.OpenFile(os.DevNull, os.O_WRONLY, 0)
	if err != nil {
		return nil, err
	}
	command.Stdout = null
	command.Stderr = null
	if err := command.Start(); err != nil {
		_ = null.Close()
		return nil, err
	}
	_ = null.Close()
	return command.Process, nil
}

func awaitReadiness(process *os.Process, port int, path string, expectedStatus int) (bool, error) {
	client := &http.Client{
		Timeout:       1500 * time.Millisecond,
		Transport:     &http.Transport{Proxy: nil},
		CheckRedirect: func(_ *http.Request, _ []*http.Request) error { return http.ErrUseLastResponse },
	}
	url := fmt.Sprintf("http://127.0.0.1:%d%s", port, path)
	for attempt := 0; attempt < attempts; attempt++ {
		var status syscall.WaitStatus
		pid, err := syscall.Wait4(process.Pid, &status, syscall.WNOHANG, nil)
		if err != nil && err != syscall.EINTR {
			return false, err
		}
		if err == syscall.EINTR {
			continue
		}
		if pid == process.Pid {
			return false, fmt.Errorf("exit status %d", status.ExitStatus())
		}
		response, err := client.Get(url)
		if err == nil {
			response.Body.Close()
			if expectedStatus == 0 && response.StatusCode >= 200 && response.StatusCode < 400 || response.StatusCode == expectedStatus {
				return true, nil
			}
		}
		time.Sleep(250 * time.Millisecond)
	}
	return false, nil
}
