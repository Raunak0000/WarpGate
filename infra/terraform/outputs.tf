output "instance_id" {
  description = "The EC2 Instance ID"
  value       = module.vpn_node.instance_id
}

output "public_ip" {
  description = "The Elastic IP attached to the VPN node"
  value       = module.vpn_node.public_ip
}

output "private_ip" {
  description = "The private IP of the VPN node"
  value       = module.vpn_node.private_ip
}

output "region" {
  description = "The AWS deployment region"
  value       = var.aws_region
}