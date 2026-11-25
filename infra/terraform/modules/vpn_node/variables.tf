variable "vpc_cidr" {
  description = "CIDR block for the VPC"
  type        = string
  default     = "10.0.0.0/16"
}

variable "subnet_cidr" {
  description = "CIDR block for the public subnet"
  type        = string
  default     = "10.0.1.0/24"
}

variable "admin_cidr" {
  description = "CIDR block allowed to access the EC2 instance over SSH"
  type        = string
}

variable "ssh_public_key" {
  description = "SSH public key used to access the VPN instance"
  type        = string
}

variable "deployment_id" {
  description = "Unique identifier for each WarpGate deployment"
  type        = string

  validation {
    condition     = can(regex("^warp-[0-9a-f]{12}$", var.deployment_id))
    error_message = "Deployment ID must start with 'warp-' followed by exactly 12 lowercase hexadecimal characters."
  }
}

variable "use_golden_ami" {
  description = "Whether to look up and launch a pre-baked Packer WarpGate Golden AMI"
  type        = bool
  default     = false
}

variable "golden_ami_id" {
  description = "Explicit pre-baked Golden AMI ID (overrides automatic lookup when non-empty)"
  type        = string
  default     = ""
}
