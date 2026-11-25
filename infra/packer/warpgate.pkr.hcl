packer {
  required_plugins {
    amazon = {
      version = ">= 1.3.0"
      source  = "github.com/hashicorp/amazon"
    }
    ansible = {
      version = ">= 1.1.0"
      source  = "github.com/hashicorp/ansible"
    }
  }
}

variable "aws_region" {
  type        = string
  default     = "us-east-1"
  description = "Target AWS region in which to build and register the WarpGate Golden AMI"
}

variable "ami_prefix" {
  type        = string
  default     = "warpgate-golden"
  description = "AMI name prefix used by Terraform golden AMI lookup"
}

source "amazon-ebs" "warpgate_ubuntu" {
  ami_name      = "${var.ami_prefix}-${formatdate("YYYYMMDD-hhmmss", timestamp())}"
  instance_type = "t3.micro"
  region        = var.aws_region
  ssh_username  = "ubuntu"

  source_ami_filter {
    filters = {
      name                = "ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*"
      root-device-type    = "ebs"
      virtualization-type = "hvm"
    }
    most_recent = true
    owners      = ["099720109477"]
  }

  launch_block_device_mappings {
    device_name           = "/dev/sda1"
    volume_size           = 8
    volume_type           = "gp3"
    encrypted             = true
    delete_on_termination = true
  }

  tags = {
    Name    = "${var.ami_prefix}-ubuntu-22.04"
    Project = "WarpGate"
    Role    = "vpn-golden-ami"
  }
}

build {
  name    = "warpgate-golden-ami"
  sources = ["source.amazon-ebs.warpgate_ubuntu"]

  provisioner "ansible" {
    playbook_file = "${path.root}/prepare.yml"
  }

  provisioner "shell" {
    inline = [
      "sudo rm -f /etc/wireguard/server_private.key /etc/wireguard/wg0.conf",
      "sudo rm -f /tmp/AdGuardHome_linux_amd64.tar.gz",
      "sudo apt-get clean",
      "sudo rm -rf /var/lib/apt/lists/*"
    ]
  }
}

