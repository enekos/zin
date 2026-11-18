package zin_test

import "os/exec"

func runZin(binary string, args ...string) ([]byte, error) {
	return exec.Command(binary, args...).CombinedOutput()
}
