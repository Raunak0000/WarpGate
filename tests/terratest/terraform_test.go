package terratest

import (
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/gruntwork-io/terratest/modules/terraform"
)

func TestTerraformConfigurationValidates(t *testing.T) {
	sourceDir := "../../infra/terraform"
	tempDir := t.TempDir()

	// Copy Terraform configuration only.
	// Exclude state, plans, credentials and generated files.
	err := filepath.WalkDir(sourceDir, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}

		if entry.IsDir() {
			if entry.Name() == ".terraform" {
				return filepath.SkipDir
			}
			return nil
		}

		name := entry.Name()
		if filepath.Ext(name) != ".tf" && name != ".terraform.lock.hcl" {
			return nil
		}

		relative, err := filepath.Rel(sourceDir, path)
		if err != nil {
			return err
		}

		destination := filepath.Join(tempDir, relative)

		if err := os.MkdirAll(filepath.Dir(destination), 0755); err != nil {
			return err
		}

		src, err := os.Open(path)
		if err != nil {
			return err
		}
		defer src.Close()

		dst, err := os.Create(destination)
		if err != nil {
			return err
		}
		defer dst.Close()

		_, err = io.Copy(dst, src)
		return err
	})

	if err != nil {
		t.Fatalf("Failed to prepare isolated Terraform configuration: %v", err)
	}

	options := &terraform.Options{
		TerraformDir: tempDir,
		NoColor:      true,
	}

	terraform.InitAndValidate(t, options)
}
