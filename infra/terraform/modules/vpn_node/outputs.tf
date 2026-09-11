output "instance_id" {
  value = aws_instance.warpgate_node.id
}

output "public_ip" {
  value = aws_eip.warpgate_eip.public_ip
}

output "private_ip" {
  value = aws_instance.warpgate_node.private_ip
}