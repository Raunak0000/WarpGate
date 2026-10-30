package terratest

import (
	"testing"

	"github.com/gruntwork-io/terratest/modules/terraform"
)

func TestTerraformConfigurationValidates(t *testing.T) {
	t.Parallel()

	options := &terraform.Options{
		TerraformDir: "../../infra/terraform",
		NoColor:      true,
		Vars: map[string]interface{}{
			"ssh_public_key": "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOfFNypsgLNoYhhTmLgWNne8hBnQKPUJ5wanjnXu7FD8 warpgate-admin",
			"admin_cidr":     "127.0.0.1/32",
		},
	}

	terraform.Init(t, options)
	terraform.Validate(t, options)
}

func TestTerraformPlan(t *testing.T) {
	t.Parallel()

	options := &terraform.Options{
		TerraformDir: "../../infra/terraform",
		NoColor:      true,
		Vars: map[string]interface{}{
			"ssh_public_key": "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOfFNypsgLNoYhhTmLgWNne8hBnQKPUJ5wanjnXu7FD8 warpgate-admin",
			"admin_cidr":     "127.0.0.1/32",
		},
	}

	terraform.InitAndPlan(t, options)
}
